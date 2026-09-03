import argparse
import json
import urllib.request


def main() -> int:
    parser = argparse.ArgumentParser(description="Prepare a Harmony Cut review project.")
    parser.add_argument("--media-dir", required=True)
    parser.add_argument("--title", default="")
    parser.add_argument("--requirement", default="")
    parser.add_argument("--server", default="http://127.0.0.1:8765")
    parser.add_argument("--no-auto-rotate", action="store_true")
    args = parser.parse_args()

    payload = {
        "media_dir": args.media_dir,
        "title": args.title,
        "requirement": args.requirement,
        "auto_rotate_landscape": not args.no_auto_rotate,
    }
    request = urllib.request.Request(
        f"{args.server.rstrip('/')}/api/agent/prepare",
        data=json.dumps(payload, ensure_ascii=False).encode("utf-8"),
        headers={"Content-Type": "application/json; charset=utf-8"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=3600) as response:
        data = json.loads(response.read().decode("utf-8"))
    if not data.get("ok"):
        raise SystemExit(data.get("error", "prepare failed"))
    print(json.dumps({
        "project_id": data["project"]["id"],
        "review_url": data["review_url"],
        "marker_files": data.get("marker_files", {}),
        "next_instruction": data.get("next_instruction"),
    }, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
