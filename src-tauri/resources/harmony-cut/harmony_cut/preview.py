import shutil
import subprocess
from pathlib import Path

import imageio_ffmpeg

from .paths import PREVIEWS_DIR, ensure_dirs, safe_project_id


def _ffmpeg() -> str:
    return imageio_ffmpeg.get_ffmpeg_exe()


def _clip_source_duration(clip: dict) -> float:
    source_in = float(clip.get("source_in") or 0)
    source_out = float(clip.get("source_out") or 0)
    if source_out > source_in:
        return source_out - source_in
    timeline_start = float(clip.get("timeline_start") or 0)
    timeline_end = float(clip.get("timeline_end") or timeline_start + 1.0)
    return max(0.1, timeline_end - timeline_start)


def _segment_filter(width: int, height: int, fps: int, speed: float) -> str:
    ratio = width / height
    return (
        f"setpts=PTS/{speed},"
        f"scale='if(gt(a,{ratio}),-2,{width})':'if(gt(a,{ratio}),{height},-2)',"
        f"crop={width}:{height},"
        f"fps={fps},"
        "format=yuv420p"
    )


def _concat_path(path: Path) -> str:
    return str(path.resolve()).replace("\\", "/").replace("'", "'\\''")


def render_preview(project: dict) -> dict:
    ensure_dirs()
    project_id = safe_project_id(project.get("id", "preview"))
    out_dir = PREVIEWS_DIR / project_id
    work_dir = out_dir / "work"
    if work_dir.exists():
        shutil.rmtree(work_dir)
    work_dir.mkdir(parents=True, exist_ok=True)
    out_dir.mkdir(parents=True, exist_ok=True)

    settings = project.get("settings", {})
    width = int(settings.get("preview_width") or 540)
    height = int(settings.get("preview_height") or 960)
    fps = int(settings.get("preview_fps") or 30)
    segments = []
    ffmpeg = _ffmpeg()

    for idx, clip in enumerate(project.get("clips", []), start=1):
        if clip.get("role") == "reject":
            continue
        path = Path(clip.get("path") or clip.get("original_path") or "")
        if not path.exists():
            continue
        speed = max(0.05, float(clip.get("speed") or 1.0))
        source_in = max(0.0, float(clip.get("source_in") or 0))
        source_duration = max(0.1, _clip_source_duration(clip))
        segment = work_dir / f"segment_{idx:03d}.mp4"
        cmd = [
            ffmpeg,
            "-y",
            "-ss",
            f"{source_in:.3f}",
            "-t",
            f"{source_duration:.3f}",
            "-i",
            str(path),
            "-vf",
            _segment_filter(width, height, fps, speed),
            "-an",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "20",
            str(segment),
        ]
        subprocess.run(cmd, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        segments.append(segment)

    if not segments:
        raise RuntimeError("No valid clips to render")

    concat_file = work_dir / "concat.txt"
    concat_file.write_text(
        "\n".join(f"file '{_concat_path(path)}'" for path in segments),
        encoding="utf-8",
    )
    output = out_dir / "preview.mp4"
    cmd = [
        ffmpeg,
        "-y",
        "-f",
        "concat",
        "-safe",
        "0",
        "-i",
        str(concat_file),
        "-c",
        "copy",
        str(output),
    ]
    subprocess.run(cmd, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return {
        "path": str(output),
        "width": width,
        "height": height,
        "fps": fps,
        "clips": len(segments),
    }
