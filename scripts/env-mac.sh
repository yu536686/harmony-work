#!/bin/zsh
export PROJECT_ROOT="$(cd "$(dirname "${(%):-%N}")/.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/harmony-work-macos-target}"
export npm_config_cache="${npm_config_cache:-${TMPDIR:-/tmp}/harmony-work-npm-cache}"
