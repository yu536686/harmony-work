import json
import time
import uuid
from pathlib import Path

from .paths import PROJECTS_DIR, ensure_dirs, safe_project_id


def now_stamp():
    return time.strftime("%Y%m%d_%H%M%S")


def project_path(project_id: str) -> Path:
    return PROJECTS_DIR / safe_project_id(project_id) / "project.json"


def create_project(media_dir: str, title: str, requirement: str) -> dict:
    ensure_dirs()
    project_id = f"{now_stamp()}_{uuid.uuid4().hex[:8]}"
    project = {
        "id": project_id,
        "title": title or f"HarmonyCut_{now_stamp()}",
        "media_dir": media_dir,
        "requirement": requirement or "",
        "created_at": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "updated_at": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "settings": {
            "width": 1080,
            "height": 1920,
            "fps": 60,
            "template": "commercial_review",
        },
        "assets": [],
        "clips": [],
        "captions": [],
        "audio": [],
        "exports": {},
    }
    save_project(project)
    return project


def load_project(project_id: str) -> dict:
    path = project_path(project_id)
    if not path.exists():
        raise FileNotFoundError(project_id)
    return json.loads(path.read_text(encoding="utf-8"))


def save_project(project: dict) -> dict:
    ensure_dirs()
    project["updated_at"] = time.strftime("%Y-%m-%dT%H:%M:%S")
    path = project_path(project["id"])
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(project, ensure_ascii=False, indent=2), encoding="utf-8")
    return project


def _sort_time(path: Path) -> float:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        return time.mktime(time.strptime(data.get("updated_at", ""), "%Y-%m-%dT%H:%M:%S"))
    except Exception:
        return path.stat().st_mtime


def list_projects() -> list[dict]:
    ensure_dirs()
    projects = []
    for path in sorted(PROJECTS_DIR.glob("*/project.json"), key=_sort_time, reverse=True):
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
            projects.append({
                "id": data.get("id"),
                "title": data.get("title"),
                "media_dir": data.get("media_dir"),
                "updated_at": data.get("updated_at"),
            })
        except Exception:
            continue
    return projects
