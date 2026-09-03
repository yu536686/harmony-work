#!/bin/zsh
set -e
source "$(dirname "$0")/env-mac.sh"
cd "$PROJECT_ROOT"
npm run tauri build -- --bundles app,dmg
