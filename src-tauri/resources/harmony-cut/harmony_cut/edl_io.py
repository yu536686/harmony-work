import json
import xml.etree.ElementTree as ET
from pathlib import Path


MARKER_JSON = "harmony_cut_edl.json"
MARKER_XML = "harmony_cut_edl.xml"


def _media_dir(project: dict) -> Path | None:
    raw = project.get("media_dir")
    if not raw:
        return None
    path = Path(raw)
    return path if path.exists() and path.is_dir() else None


def marker_paths(project: dict) -> dict:
    media_dir = _media_dir(project)
    if not media_dir:
        return {}
    return {
        "json": str(media_dir / MARKER_JSON),
        "xml": str(media_dir / MARKER_XML),
    }


def marker_payload(project: dict) -> dict:
    return {
        "format": "harmony_cut_marker",
        "version": 1,
        "workflow_status": project.get("workflow_status", "draft"),
        "review_url": project.get("review_url", ""),
        "project": {
            "id": project.get("id"),
            "title": project.get("title"),
            "media_dir": project.get("media_dir"),
            "requirement": project.get("requirement", ""),
            "settings": project.get("settings", {}),
            "updated_at": project.get("updated_at"),
        },
        "assets": [
            {
                "id": asset.get("id"),
                "name": asset.get("name"),
                "path": asset.get("path"),
                "kind": asset.get("kind"),
                "duration": asset.get("duration"),
                "width": asset.get("width"),
                "height": asset.get("height"),
                "rotation": asset.get("rotation", 0),
                "proxy_path": asset.get("proxy_path"),
                "thumbnail": asset.get("thumbnail"),
                "tags": asset.get("tags", []),
                "best_windows": asset.get("best_windows", []),
            }
            for asset in project.get("assets", [])
        ],
        "clips": project.get("clips", []),
        "captions": project.get("captions", []),
        "audio": project.get("audio", []),
        "exports": project.get("exports", {}),
    }


def write_marker_files(project: dict) -> dict:
    paths = marker_paths(project)
    if not paths:
        return {}
    payload = marker_payload(project)
    json_path = Path(paths["json"])
    json_path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
    xml_path = Path(paths["xml"])
    _write_xml(payload, xml_path)
    project.setdefault("marker_files", {}).update(paths)
    return paths


def _write_xml(payload: dict, path: Path) -> None:
    root = ET.Element("harmonyCutMarker", attrib={"version": str(payload["version"])})
    project = payload["project"]
    project_el = ET.SubElement(root, "project", attrib={
        "id": str(project.get("id") or ""),
        "title": str(project.get("title") or ""),
        "status": str(payload.get("workflow_status") or ""),
    })
    ET.SubElement(project_el, "mediaDir").text = str(project.get("media_dir") or "")
    ET.SubElement(project_el, "requirement").text = str(project.get("requirement") or "")
    ET.SubElement(project_el, "reviewUrl").text = str(payload.get("review_url") or "")

    sequence = ET.SubElement(root, "sequence")
    for clip in payload.get("clips", []):
        clip_el = ET.SubElement(sequence, "clip", attrib={
            "shot": str(clip.get("shot") or ""),
            "role": str(clip.get("role") or ""),
            "assetId": str(clip.get("asset_id") or ""),
        })
        for key in ("path", "original_path", "source_in", "source_out", "timeline_start", "timeline_end", "speed"):
            ET.SubElement(clip_el, key).text = str(clip.get(key, ""))

    captions = ET.SubElement(root, "captions")
    for caption in payload.get("captions", []):
        caption_el = ET.SubElement(captions, "caption", attrib={
            "start": str(caption.get("start", "")),
            "end": str(caption.get("end", "")),
        })
        caption_el.text = str(caption.get("text", ""))

    tree = ET.ElementTree(root)
    ET.indent(tree, space="  ")
    tree.write(path, encoding="utf-8", xml_declaration=True)
