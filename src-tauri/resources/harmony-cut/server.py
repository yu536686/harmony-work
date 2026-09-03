import json
import mimetypes
import os
import re
import traceback
from pathlib import Path

from starlette.applications import Starlette
from starlette.concurrency import run_in_threadpool
from starlette.responses import FileResponse, JSONResponse, PlainTextResponse
from starlette.routing import Mount, Route
from starlette.staticfiles import StaticFiles
import uvicorn

from harmony_cut.draft import create_jianying_draft
from harmony_cut.edl_io import write_marker_files
from harmony_cut.media import scan_media
from harmony_cut.preview import render_preview
from harmony_cut.roughcut import build_initial_edl
from harmony_cut.store import create_project, list_projects, load_project, save_project


ROOT = Path(__file__).resolve().parent
WEB = ROOT / "web"
APP_BASE_URL = "http://127.0.0.1:8765"


def json_body(request):
    return request.json()


def error_response(exc: Exception, status_code=500):
    return JSONResponse({
        "ok": False,
        "error": str(exc),
        "trace": traceback.format_exc(),
    }, status_code=status_code)


async def index(request):
    return FileResponse(WEB / "index.html")


async def api_projects(request):
    if request.method == "GET":
        return JSONResponse({"ok": True, "projects": list_projects()})
    try:
        body = await json_body(request)
        media_dir = body.get("media_dir", "")
        if media_dir and not Path(media_dir).exists():
            return error_response(Exception(f"media_dir not found: {media_dir}"), 400)
        project = create_project(
            media_dir,
            body.get("title", ""),
            body.get("requirement", ""),
        )
        project["review_url"] = f"{APP_BASE_URL}/?project={project['id']}"
        project["workflow_status"] = "created"
        write_marker_files(project)
        save_project(project)
        return JSONResponse({"ok": True, "project": project})
    except Exception as exc:
        return error_response(exc)


def _drive_roots():
    roots = []
    for letter in "ABCDEFGHIJKLMNOPQRSTUVWXYZ":
        root = Path(f"{letter}:\\")
        if root.exists():
            roots.append({"name": f"{letter}:\\", "path": str(root), "kind": "drive"})
    return roots


async def api_browse(request):
    try:
        raw_path = request.query_params.get("path", "")
        if not raw_path:
            return JSONResponse({"ok": True, "path": "", "parent": "", "items": _drive_roots()})
        path = Path(raw_path)
        if not path.exists() or not path.is_dir():
            return error_response(Exception(f"folder not found: {raw_path}"), 400)
        items = []
        for child in sorted(path.iterdir(), key=lambda p: (not p.is_dir(), p.name.lower())):
            if child.is_dir():
                items.append({"name": child.name, "path": str(child), "kind": "folder"})
        parent = str(path.parent) if path.parent != path else ""
        return JSONResponse({"ok": True, "path": str(path), "parent": parent, "items": items[:300]})
    except PermissionError as exc:
        return error_response(exc, 403)
    except Exception as exc:
        return error_response(exc)


async def api_project(request):
    try:
        project = load_project(request.path_params["project_id"])
        return JSONResponse({"ok": True, "project": project})
    except FileNotFoundError:
        return error_response(Exception("project not found"), 404)
    except Exception as exc:
        return error_response(exc)


async def api_scan(request):
    try:
        project = load_project(request.path_params["project_id"])
        body = await json_body(request)
        media_dir = body.get("media_dir") or project.get("media_dir")
        if not Path(media_dir).exists():
            return error_response(Exception(f"media_dir not found: {media_dir}"), 400)
        requirement = body.get("requirement")
        if requirement is not None:
            project["requirement"] = requirement
        project["media_dir"] = media_dir
        project["assets"] = await run_in_threadpool(
            scan_media,
            media_dir,
            bool(body.get("auto_rotate_landscape", True)),
        )
        project = build_initial_edl(project)
        project["workflow_status"] = "agent_roughcut_ready"
        project["review_url"] = f"{APP_BASE_URL}/?project={project['id']}"
        write_marker_files(project)
        save_project(project)
        return JSONResponse({"ok": True, "project": project})
    except Exception as exc:
        return error_response(exc)


async def api_save(request):
    try:
        body = await json_body(request)
        project = load_project(request.path_params["project_id"])
        for key in ("title", "requirement", "settings", "assets", "clips", "captions", "audio"):
            if key in body:
                project[key] = body[key]
        project["workflow_status"] = body.get("workflow_status") or "user_review_saved"
        project["review_url"] = f"{APP_BASE_URL}/?project={project['id']}"
        write_marker_files(project)
        save_project(project)
        return JSONResponse({"ok": True, "project": project})
    except Exception as exc:
        return error_response(exc)


async def api_draft(request):
    try:
        project = load_project(request.path_params["project_id"])
        result = await run_in_threadpool(create_jianying_draft, project)
        project.setdefault("exports", {})["jianying"] = result
        project["workflow_status"] = "draft_created"
        write_marker_files(project)
        save_project(project)
        return JSONResponse({"ok": True, "result": result, "project": project})
    except Exception as exc:
        return error_response(exc)


def split_caption_text(text: str) -> list[str]:
    clean = re.sub(r"\s+", " ", text or "").strip()
    if not clean:
        return []
    parts = re.split(r"(?<=[。！？!?；;.])\s+|\n+", clean)
    lines = [p.strip(" ,，") for p in parts if p.strip(" ,，")]
    if len(lines) <= 1 and len(clean) > 28:
        words = clean.split(" ")
        lines = []
        current = ""
        for word in words:
            candidate = f"{current} {word}".strip()
            if len(candidate) > 24 and current:
                lines.append(current)
                current = word
            else:
                current = candidate
        if current:
            lines.append(current)
    return lines


async def api_captions(request):
    try:
        project = load_project(request.path_params["project_id"])
        body = await json_body(request)
        lines = split_caption_text(body.get("text", ""))
        clips = [c for c in project.get("clips", []) if c.get("role") != "reject"]
        captions = []
        if clips and lines:
            for idx, line in enumerate(lines):
                clip = clips[min(idx, len(clips) - 1)]
                start = float(clip.get("timeline_start") or 0)
                end = float(clip.get("timeline_end") or start + 1)
                captions.append({"text": line, "start": round(start + 0.08, 3), "end": round(max(start + 0.2, end - 0.08), 3)})
        project["captions"] = captions
        save_project(project)
        return JSONResponse({"ok": True, "captions": captions, "project": project})
    except Exception as exc:
        return error_response(exc)


async def api_preview(request):
    try:
        project = load_project(request.path_params["project_id"])
        body = await json_body(request)
        for key in ("settings", "assets", "clips", "captions", "audio"):
            if key in body:
                project[key] = body[key]
        if body:
            save_project(project)
        result = await run_in_threadpool(render_preview, project)
        latest = load_project(request.path_params["project_id"])
        latest.setdefault("exports", {})["preview"] = result
        latest["workflow_status"] = "preview_rendered"
        write_marker_files(latest)
        save_project(latest)
        return JSONResponse({"ok": True, "result": result, "project": latest})
    except Exception as exc:
        return error_response(exc)


async def api_file(request):
    path = Path(request.query_params.get("path", ""))
    if not path.exists() or not path.is_file():
        return PlainTextResponse("not found", status_code=404)
    media_type = mimetypes.guess_type(path.name)[0] or "application/octet-stream"
    return FileResponse(path, media_type=media_type)


async def api_agent_prepare(request):
    try:
        body = await json_body(request)
        media_dir = body.get("media_dir", "")
        if not media_dir or not Path(media_dir).exists():
            return error_response(Exception(f"media_dir not found: {media_dir}"), 400)
        title = body.get("title") or f"HarmonyCut_{Path(media_dir).name}"
        requirement = body.get("requirement", "")
        project = create_project(media_dir, title, requirement)
        project["workflow_status"] = "agent_analyzing"
        project["review_url"] = f"{APP_BASE_URL}/?project={project['id']}"
        project["assets"] = await run_in_threadpool(
            scan_media,
            media_dir,
            bool(body.get("auto_rotate_landscape", True)),
        )
        project = build_initial_edl(project)
        project["workflow_status"] = "agent_roughcut_ready"
        marker_files = write_marker_files(project)
        save_project(project)
        return JSONResponse({
            "ok": True,
            "project": project,
            "review_url": project["review_url"],
            "marker_files": marker_files,
            "next_instruction": "请在工作台完成校正并保存，然后回到 AI 对话发送：继续",
        })
    except Exception as exc:
        return error_response(exc)


async def api_agent_continue(request):
    try:
        body = await json_body(request)
        project_id = body.get("project_id")
        if not project_id:
            projects = list_projects()
            if not projects:
                return error_response(Exception("no project found"), 404)
            project_id = projects[0]["id"]
        project = load_project(project_id)
        if project.get("workflow_status") not in {"user_review_saved", "preview_rendered", "draft_created"}:
            project["workflow_status"] = "agent_continuing"
            save_project(project)

        preview_result = None
        draft_result = None
        if not body.get("skip_preview"):
            preview_result = await run_in_threadpool(render_preview, project)
            latest = load_project(project_id)
            latest.setdefault("exports", {})["preview"] = preview_result
            latest["workflow_status"] = "preview_rendered"
            write_marker_files(latest)
            save_project(latest)
            project = latest
        if not body.get("skip_draft"):
            draft_result = await run_in_threadpool(create_jianying_draft, project)
            latest = load_project(project_id)
            latest.setdefault("exports", {})["jianying"] = draft_result
            latest["workflow_status"] = "draft_created"
            write_marker_files(latest)
            save_project(latest)
            project = latest

        return JSONResponse({
            "ok": True,
            "project": project,
            "preview": preview_result,
            "draft": draft_result,
            "marker_files": project.get("marker_files", {}),
        })
    except Exception as exc:
        return error_response(exc)


routes = [
    Route("/", index),
    Route("/api/agent/prepare", api_agent_prepare, methods=["POST"]),
    Route("/api/agent/continue", api_agent_continue, methods=["POST"]),
    Route("/api/browse", api_browse),
    Route("/api/projects", api_projects, methods=["GET", "POST"]),
    Route("/api/projects/{project_id}", api_project),
    Route("/api/projects/{project_id}/scan", api_scan, methods=["POST"]),
    Route("/api/projects/{project_id}/save", api_save, methods=["POST"]),
    Route("/api/projects/{project_id}/captions", api_captions, methods=["POST"]),
    Route("/api/projects/{project_id}/preview", api_preview, methods=["POST"]),
    Route("/api/projects/{project_id}/draft", api_draft, methods=["POST"]),
    Route("/api/file", api_file),
    Mount("/static", StaticFiles(directory=str(WEB)), name="static"),
]


app = Starlette(debug=True, routes=routes)


if __name__ == "__main__":
    uvicorn.run("server:app", host="127.0.0.1", port=8765, reload=False)
