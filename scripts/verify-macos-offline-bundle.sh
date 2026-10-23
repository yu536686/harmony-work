#!/bin/zsh
set -euo pipefail

APP="${1:?Pass the built Harmony Work.app path}"
APP="${APP:A}"
ROOT="$APP/Contents/Resources/resources"
for relative in node/bin/node node/bin/npm node/bin/npx git/bin/git \
  git/libexec/git-core/git-remote-http git/source/git-2.54.0.tar.xz \
  python/bin/python3 python/bin/pip \
  wecom/bin/wecom-cli; do
  if [[ ! -e "$ROOT/$relative" ]]; then
    print -u2 "Missing bundled runtime: $relative"
    exit 1
  fi
done
print -r -- "f689162364c10de79ef89aa8dbf48731eb057e34edbbd20aca510ce0154681a3  $ROOT/git/source/git-2.54.0.tar.xz" | shasum -a 256 -c - >/dev/null

export PATH="$ROOT/node/bin:$ROOT/python/bin:$ROOT/git/bin:$ROOT/wecom/bin:/usr/bin:/bin:/usr/sbin:/sbin"
[[ "$(node --version)" == "v24.18.0" ]]
npm --version
npx --version
[[ "$(git --version)" == "git version 2.54.0" ]]
[[ "$(git --exec-path)" == "$ROOT/git/libexec/git-core" ]]
python3 -c 'import ssl, sqlite3, pip; print("Python", __import__("sys").version.split()[0])'
python3 -I -c 'import starlette, uvicorn, pydantic, cv2, numpy, imageio_ffmpeg; import subprocess; subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-version"], check=True, stdout=subprocess.DEVNULL); print("Harmony Cut offline dependencies verified")'
[[ -f "$ROOT/harmony-cut/server.py" ]]
[[ -f "$ROOT/licenses/LICENSE" ]]
pip --version
[[ "$(wecom-cli --version)" == "wecom-cli 0.1.9" ]]

for binary in "$ROOT/node/bin/node" "$ROOT/git/bin/git" \
  "$ROOT/python/bin/python3.12" "$ROOT/wecom/bin/wecom-cli"; do
  [[ "$(lipo -archs "$binary")" == "arm64" ]]
done
print "Offline macOS runtime bundle verified: $APP"
