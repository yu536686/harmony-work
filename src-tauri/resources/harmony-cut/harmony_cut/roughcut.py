from pathlib import Path


DEFAULT_ROLES = [
    ("opening", 2.2),
    ("opening_detail", 2.0),
    ("hero", 2.8),
    ("hero_detail", 2.6),
    ("handheld", 2.2),
    ("handheld_detail", 2.0),
    ("alternate_handheld", 2.2),
    ("alternate_detail", 2.0),
    ("wrist_full", 2.6),
    ("wrist_detail", 2.4),
    ("wrist_full_2", 2.6),
    ("wrist_detail_2", 2.728),
]


def infer_asset_mode(asset: dict) -> str:
    name = asset.get("name", "").lower()
    if any(k in name for k in ["8433", "8434", "8435", "8436"]):
        return "hero"
    if any(k in name for k in ["8455", "8456", "8457", "8458", "8459", "8460", "8461"]):
        return "detail"
    if any(k in name for k in ["8487", "8488", "8489"]):
        return "wrist"
    return "full"


def choose_window(asset: dict, duration: float) -> tuple[float, float]:
    windows = asset.get("best_windows") or []
    if windows:
        win = windows[0]
        start = float(win["source_in"])
        return start, round(start + duration, 3)
    total = float(asset.get("duration") or duration)
    start = max(0.0, (total - duration) / 2.0)
    return round(start, 3), round(min(total, start + duration), 3)


def build_initial_edl(project: dict) -> dict:
    videos = [a for a in project.get("assets", []) if a.get("kind") == "video" and a.get("opened", True)]
    if not videos:
        project["clips"] = []
        return project

    ranked = sorted(videos, key=lambda a: max([w.get("score", 0) for w in a.get("best_windows", [{}])] or [0]), reverse=True)
    hero = [a for a in ranked if infer_asset_mode(a) == "hero"] or ranked
    detail = [a for a in ranked if infer_asset_mode(a) == "detail"] or ranked
    wrist = [a for a in ranked if infer_asset_mode(a) == "wrist"] or ranked
    full = [a for a in ranked if infer_asset_mode(a) == "full"] or ranked

    pools = {
        "opening": full,
        "opening_detail": full,
        "hero": hero,
        "hero_detail": hero,
        "handheld": full,
        "handheld_detail": detail,
        "alternate_handheld": full,
        "alternate_detail": detail,
        "wrist_full": full,
        "wrist_detail": wrist,
        "wrist_full_2": full,
        "wrist_detail_2": wrist,
    }

    clips = []
    timeline = 0.0
    used = {}
    for idx, (role, duration) in enumerate(DEFAULT_ROLES, start=1):
        pool = pools.get(role) or ranked
        asset = pool[used.get(role, 0) % len(pool)]
        used[role] = used.get(role, 0) + 1
        source_path = asset.get("proxy_path") or asset["path"]
        source_in, source_out = choose_window(asset, duration)
        clips.append({
            "shot": idx,
            "role": role,
            "asset_id": asset["id"],
            "path": source_path,
            "original_path": asset["path"],
            "source_in": source_in,
            "source_out": source_out,
            "timeline_start": round(timeline, 3),
            "timeline_end": round(timeline + duration, 3),
            "speed": 1.0,
            "scale_keyframes": [
                {"time": 0, "scale": 1.0, "x": 0, "y": 0},
                {"time": duration, "scale": 1.04 if "detail" in role or "hero" in role else 1.0, "x": 0, "y": 0},
            ],
        })
        timeline += duration
    project["clips"] = clips
    project["settings"]["duration"] = round(timeline, 3)
    return project

