from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DATA_DIR = ROOT / "data"
PROJECTS_DIR = DATA_DIR / "projects"
PROXIES_DIR = DATA_DIR / "proxies"
THUMBS_DIR = DATA_DIR / "thumbs"
PREVIEWS_DIR = DATA_DIR / "previews"


def ensure_dirs():
    for path in (DATA_DIR, PROJECTS_DIR, PROXIES_DIR, THUMBS_DIR, PREVIEWS_DIR):
        path.mkdir(parents=True, exist_ok=True)


def safe_project_id(value: str) -> str:
    cleaned = "".join(ch if ch.isalnum() or ch in "-_" else "_" for ch in value.strip())
    return cleaned[:80] or "project"
