import hashlib
import math
import os
import subprocess
from pathlib import Path

import cv2
import imageio_ffmpeg
import numpy as np

from .paths import PROXIES_DIR, THUMBS_DIR, ensure_dirs


VIDEO_EXTS = {".mp4", ".mov", ".m4v", ".avi", ".mkv", ".webm"}
AUDIO_EXTS = {".mp3", ".wav", ".m4a", ".aac", ".flac", ".ogg"}
IMAGE_EXTS = {".jpg", ".jpeg", ".png", ".webp"}
TARGET_W = 1080
TARGET_H = 1920
TARGET_FPS = 60


def asset_id_for(path: Path) -> str:
    raw = str(path.resolve()).encode("utf-8", errors="ignore")
    return hashlib.sha1(raw).hexdigest()[:16]


def media_kind(path: Path) -> str | None:
    ext = path.suffix.lower()
    if ext in VIDEO_EXTS:
        return "video"
    if ext in AUDIO_EXTS:
        return "audio"
    if ext in IMAGE_EXTS:
        return "image"
    return None


def probe_video(path: Path) -> dict:
    cap = cv2.VideoCapture(str(path))
    if not cap.isOpened():
        return {"opened": False}
    fps = cap.get(cv2.CAP_PROP_FPS) or 0
    frames = int(cap.get(cv2.CAP_PROP_FRAME_COUNT) or 0)
    width = int(cap.get(cv2.CAP_PROP_FRAME_WIDTH) or 0)
    height = int(cap.get(cv2.CAP_PROP_FRAME_HEIGHT) or 0)
    cap.release()
    return {
        "opened": True,
        "fps": fps,
        "frames": frames,
        "width": width,
        "height": height,
        "duration": frames / fps if fps else 0,
    }


def cover_resize(frame, width=TARGET_W, height=TARGET_H):
    h, w = frame.shape[:2]
    target_ratio = width / height
    ratio = w / h
    if ratio > target_ratio:
        crop_w = int(h * target_ratio)
        x = max(0, (w - crop_w) // 2)
        frame = frame[:, x:x + crop_w]
    else:
        crop_h = int(w / target_ratio)
        y = max(0, (h - crop_h) // 2)
        frame = frame[y:y + crop_h, :]
    return cv2.resize(frame, (width, height), interpolation=cv2.INTER_AREA)


def write_image(path: Path, frame):
    path.parent.mkdir(parents=True, exist_ok=True)
    ok, encoded = cv2.imencode(path.suffix or ".jpg", frame)
    if not ok:
        raise RuntimeError(f"Could not encode image: {path}")
    path.write_bytes(encoded.tobytes())


def seek_frame(cap, t, fps=None):
    if fps:
        cap.set(cv2.CAP_PROP_POS_FRAMES, max(0, int(t * fps)))
    else:
        cap.set(cv2.CAP_PROP_POS_MSEC, max(0, t * 1000))
    ok, frame = cap.read()
    return frame if ok else None


def make_thumbnail(path: Path, asset_id: str, rotate: int = 0) -> str | None:
    ensure_dirs()
    out = THUMBS_DIR / f"{asset_id}.jpg"
    if out.exists() and out.stat().st_size > 1000:
        return str(out)
    info = probe_video(path)
    if not info.get("opened"):
        return None
    cap = cv2.VideoCapture(str(path))
    frame = seek_frame(cap, min(max(info.get("duration", 0) * 0.45, 0.1), 3.0), info.get("fps"))
    cap.release()
    if frame is None:
        return None
    if rotate == 90:
        frame = cv2.rotate(frame, cv2.ROTATE_90_CLOCKWISE)
    elif rotate == -90:
        frame = cv2.rotate(frame, cv2.ROTATE_90_COUNTERCLOCKWISE)
    thumb = cover_resize(frame, 360, 640)
    write_image(out, thumb)
    return str(out)


def rotate_proxy(path: Path, asset_id: str, degrees: int) -> str:
    ensure_dirs()
    out = PROXIES_DIR / f"{asset_id}_rot{degrees}.mp4"
    if out.exists() and out.stat().st_size > 100_000:
        return str(out)
    if degrees == 90:
        vf = "transpose=1"
    elif degrees == -90:
        vf = "transpose=2"
    else:
        return str(path)
    cmd = [
        imageio_ffmpeg.get_ffmpeg_exe(),
        "-y",
        "-i",
        str(path),
        "-vf",
        vf,
        "-an",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-preset",
        "fast",
        "-crf",
        "17",
        str(out),
    ]
    subprocess.run(cmd, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return str(out)


def frame_quality(frame):
    small = cv2.resize(frame, (180, 320), interpolation=cv2.INTER_AREA)
    gray = cv2.cvtColor(small, cv2.COLOR_BGR2GRAY)
    hsv = cv2.cvtColor(small, cv2.COLOR_BGR2HSV)
    lap = float(cv2.Laplacian(gray, cv2.CV_64F).var())
    brightness = float(gray.mean())
    contrast = float(gray.std())
    saturation = float(hsv[:, :, 1].mean())
    center = gray[70:250, 35:145]
    center_edges = float(cv2.Canny(center, 70, 150).mean())
    exposure_penalty = abs(brightness - 145.0) * 1.2 + max(0.0, 55.0 - contrast) * 1.5
    return {
        "sharpness": lap,
        "brightness": brightness,
        "contrast": contrast,
        "saturation": saturation,
        "center_edges": center_edges,
        "base": lap * 0.18 + contrast * 2.0 + saturation * 0.45 + center_edges * 4.0 - exposure_penalty,
        "gray": gray,
    }


def global_motion(prev_gray, gray):
    if prev_gray is None:
        return {"mag": 0.0, "response": 1.0}
    shift, response = cv2.phaseCorrelate(np.float32(prev_gray), np.float32(gray))
    return {"mag": float(math.hypot(shift[0], shift[1])), "response": float(response)}


def analyze_samples(path: Path, rotate: int = 0, step: float = 0.12) -> dict:
    info = probe_video(path)
    if not info.get("opened"):
        return {"duration": 0, "samples": []}
    cap = cv2.VideoCapture(str(path))
    fps = info.get("fps") or TARGET_FPS
    duration = info.get("duration") or 0
    samples = []
    prev = None
    t = 0.04
    while t < max(0.05, duration - 0.04):
        frame = seek_frame(cap, t, fps)
        if frame is not None:
            if rotate == 90:
                frame = cv2.rotate(frame, cv2.ROTATE_90_CLOCKWISE)
            elif rotate == -90:
                frame = cv2.rotate(frame, cv2.ROTATE_90_COUNTERCLOCKWISE)
            q = frame_quality(frame)
            motion = global_motion(prev, q["gray"])
            prev = q["gray"]
            samples.append({
                "time": float(t),
                "base": float(q["base"]),
                "sharpness": float(q["sharpness"]),
                "brightness": float(q["brightness"]),
                "center_edges": float(q["center_edges"]),
                "motion_mag": float(motion["mag"]),
                "motion_response": float(motion["response"]),
            })
        t += step
    cap.release()
    return {"duration": duration, "samples": samples}


def score_window(samples: list[dict], start: float, duration: float, total_duration: float, mode: str = "auto") -> dict | None:
    window = [s for s in samples if start + 0.05 <= s["time"] <= start + duration - 0.05]
    if len(window) < 4:
        return None
    bases = np.array([s["base"] for s in window], dtype=np.float32)
    sharpness = np.array([s["sharpness"] for s in window], dtype=np.float32)
    center_edges = np.array([s["center_edges"] for s in window], dtype=np.float32)
    mags = np.array([s["motion_mag"] for s in window[1:]], dtype=np.float32)
    responses = np.array([s["motion_response"] for s in window[1:]], dtype=np.float32)
    motion_mean = float(np.mean(mags)) if len(mags) else 0.0
    motion_std = float(np.std(mags)) if len(mags) else 0.0
    motion_max = float(np.max(mags)) if len(mags) else 0.0
    response = float(np.mean(responses)) if len(responses) else 1.0
    edge_penalty = 0.0
    guard = 0.28
    if start < guard:
        edge_penalty += (guard - start) * 180
    if total_duration - (start + duration) < guard:
        edge_penalty += (guard - (total_duration - (start + duration))) * 180
    score = float(np.median(bases))
    score -= motion_std * 26 + max(0.0, motion_max - 18) * 9
    score -= float(np.std(sharpness)) * 0.025
    score -= max(0.0, 0.42 - response) * 130
    score -= edge_penalty
    if mode in {"detail", "hero", "wrist"}:
        score += float(np.median(center_edges)) * 5 + float(np.median(sharpness)) * 0.08
    if mode in {"full", "presenter"}:
        score -= motion_mean * 7
    return {
        "source_in": round(start, 3),
        "source_out": round(start + duration, 3),
        "score": round(score, 2),
        "motion_mean": round(motion_mean, 3),
        "motion_std": round(motion_std, 3),
        "motion_max": round(motion_max, 3),
        "sharpness": round(float(np.median(sharpness)), 2),
    }


def best_windows(path: Path, target_duration: float, rotate: int = 0, mode: str = "auto", limit: int = 5) -> list[dict]:
    analysis = analyze_samples(path, rotate=rotate)
    total = analysis["duration"]
    if total <= 0:
        return []
    duration = min(target_duration, max(0.2, total - 0.12))
    step = 0.12 if total < 5 else 0.18
    starts = np.arange(0.08, max(0.09, total - duration - 0.08), step)
    results = []
    if len(starts) == 0:
        starts = [max(0, (total - duration) / 2)]
    for start in starts:
        scored = score_window(analysis["samples"], float(start), duration, total, mode=mode)
        if scored:
            results.append(scored)
    results.sort(key=lambda x: x["score"], reverse=True)
    return results[:limit]


def scan_media(media_dir: str, auto_rotate_landscape: bool = True) -> list[dict]:
    root = Path(media_dir)
    if not root.exists():
        raise FileNotFoundError(media_dir)
    assets = []
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        kind = media_kind(path)
        if not kind:
            continue
        asset_id = asset_id_for(path)
        item = {
            "id": asset_id,
            "path": str(path),
            "name": path.name,
            "kind": kind,
            "size": path.stat().st_size,
            "tags": [],
            "rotation": 0,
            "proxy_path": None,
            "thumbnail": None,
            "best_windows": [],
        }
        if kind == "video":
            info = probe_video(path)
            item.update(info)
            if auto_rotate_landscape and info.get("width", 0) > info.get("height", 0):
                item["rotation"] = 90
                item["tags"].append("rotate_90_cw")
            item["thumbnail"] = make_thumbnail(path, asset_id, rotate=item["rotation"])
            if item["rotation"]:
                item["proxy_path"] = rotate_proxy(path, asset_id, item["rotation"])
            scoring_path = Path(item["proxy_path"]) if item["proxy_path"] else path
            item["best_windows"] = best_windows(scoring_path, min(2.4, max(0.6, info.get("duration", 2.4) - 0.2)), rotate=0, mode="auto", limit=3)
        assets.append(item)
    return assets
