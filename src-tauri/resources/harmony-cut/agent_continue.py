import argparse
import json
import urllib.request


def _post(url: str, payload: dict | None = None, timeout: int = 3600) -> dict:
    body = json.dumps(payload or {}, ensure_ascii=False).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=body,
        headers={"Content-Type": "application/json; charset=utf-8"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read().decode("utf-8"))


def main() -> int:
    parser = argparse.ArgumentParser(description="Continue a reviewed Harmony Cut project.")
    parser.add_argument("--project-id", required=True)
    parser.add_argument("--server", default="http://127.0.0.1:8765")
    parser.add_argument("--skip-preview", action="store_true")
    parser.add_argument("--skip-draft", action="store_true")
    args = parser.parse_args()

    base = args.server.rstrip("/")
    result = _post(f"{base}/api/agent/continue", {
        "project_id": args.project_id,
        "skip_preview": args.skip_preview,
        "skip_draft": args.skip_draft,
    })
    print(json.dumps({
        "project_id": result["project"]["id"],
        "preview": result.get("preview"),
        "draft": result.get("draft"),
        "marker_files": result.get("marker_files", {}),
    }, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
