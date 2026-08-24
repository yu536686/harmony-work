import os
import sys
from pathlib import Path


def create_jianying_draft(project: dict) -> dict:
    skill_root = Path(os.environ["USERPROFILE"]) / ".codex" / "skills" / "jianying-editor"
    scripts_path = skill_root / "scripts"
    sys.path.insert(0, str(scripts_path))
    from jy_wrapper import JyProject

    width = int(project.get("settings", {}).get("width", 1080))
    height = int(project.get("settings", {}).get("height", 1920))
    title = project.get("title") or "HarmonyCut_Draft"
    draft_name = f"{title}_{project.get('id', '')}".strip("_")
    jy = JyProject(draft_name, width=width, height=height, overwrite=True)
    for clip in project.get("clips", []):
        duration = float(clip["timeline_end"]) - float(clip["timeline_start"])
        jy.add_clip(
            clip["path"],
            source_start=f"{float(clip['source_in']):.3f}s",
            duration=f"{duration:.3f}s",
            target_start=f"{float(clip['timeline_start']):.3f}s",
            track_name="MainVideo",
        )
    for audio in project.get("audio", []):
        path = audio.get("path")
        if path and Path(path).exists():
            start = float(audio.get("start", 0))
            end = float(audio.get("end", project.get("settings", {}).get("duration", 0)))
            jy.add_media_safe(path, start_time=f"{start:.3f}s", duration=f"{max(0.1, end - start):.3f}s", track_name="BGM")
    result = jy.save()
    return {
        "draft_name": jy.name,
        "draft_path": result.get("draft_path"),
    }

