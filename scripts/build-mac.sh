#!/bin/zsh
set -e
source "$(dirname "$0")/env-mac.sh"
cd "$PROJECT_ROOT"
npm run tauri build -- --config src-tauri/tauri.macos.conf.json --ci "$@"
