#!/bin/zsh
set -euo pipefail
source "$(dirname "$0")/env-mac.sh"
cd "$PROJECT_ROOT"
if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && -z "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]]; then
  print -u2 "Set TAURI_SIGNING_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY_PATH to sign the internal update package."
  exit 1
fi
app="$CARGO_TARGET_DIR/release/bundle/macos/Harmony Work.app"
[[ -d "$app" ]] || { print -u2 "Build the macOS app first with scripts/build-mac.sh."; exit 1; }
version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")"
mkdir -p release/macos
archive="$PROJECT_ROOT/release/macos/Harmony-Work-$version-macos-arm64.app.tar.gz"
COPYFILE_DISABLE=1 tar -czf "$archive" -C "${app:h}" "${app:t}"
npm run tauri -- signer sign "$archive"
shasum -a 256 "$archive"
