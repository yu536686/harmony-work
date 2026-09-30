#!/bin/zsh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RESOURCES="$ROOT/src-tauri/resources"
CACHE="$ROOT/installer/payload-cache-macos"
NODE_VERSION="v24.18.0"
GIT_VERSION="2.54.0"
PYTHON_VERSION="3.12.12"
WECOM_VERSION="0.1.9"

if [[ "$(uname -s)" != "Darwin" || "$(uname -m)" != "arm64" ]]; then
  print -u2 "The macOS offline runtime bundle requires an Apple Silicon build host."
  exit 1
fi
mkdir -p "$CACHE" "$RESOURCES"

fetch_verified() {
  local url="$1" filename="$2" expected="$3"
  local destination="$CACHE/$filename"
  if [[ -f "$destination" ]] && print -r -- "$expected  $destination" | shasum -a 256 -c - >/dev/null 2>&1; then
    print -r -- "$destination"
    return
  fi
  if [[ -f "$destination" ]]; then
    rm -f "$destination"
  fi
  curl -fL --retry 3 --connect-timeout 15 --max-time 300 -C - -o "$destination" "$url" >&2
  if ! print -r -- "$expected  $destination" | shasum -a 256 -c - >/dev/null; then
    rm -f "$destination"
    print -u2 "Offline runtime checksum mismatch: $filename"
    exit 1
  fi
  print -r -- "$destination"
}

stage_node() {
  local target="$RESOURCES/node"
  if [[ -x "$target/bin/node" && -f "$target/lib/node_modules/npm/bin/npm-cli.js" ]] &&
      [[ "$("$target/bin/node" --version)" == "$NODE_VERSION" ]]; then
    cp "$ROOT/scripts/macos-npm-wrapper" "$target/bin/npm.wrapper"
    cp "$ROOT/scripts/macos-npx-wrapper" "$target/bin/npx.wrapper"
  else
    local archive staging
    archive="$(fetch_verified \
      "https://nodejs.org/dist/$NODE_VERSION/node-$NODE_VERSION-darwin-arm64.tar.xz" \
      "node-$NODE_VERSION-darwin-arm64.tar.xz" \
      "4477b9f78efb77744cf5eb57a0e9594dba66466b38b4e93fa9f35cb907a095a6")"
    staging="$(mktemp -d "$CACHE/node-stage.XXXXXX")"
    tar -xf "$archive" -C "$staging"
    rm -rf "$target"
    mv "$staging/node-$NODE_VERSION-darwin-arm64" "$target"
    rmdir "$staging"
    print -r -- "$NODE_VERSION" > "$target/VERSION"
    cp "$ROOT/scripts/macos-npm-wrapper" "$target/bin/npm.wrapper"
    cp "$ROOT/scripts/macos-npx-wrapper" "$target/bin/npx.wrapper"
  fi
  rm -f "$target/bin/npm" "$target/bin/npx"
  mv "$target/bin/npm.wrapper" "$target/bin/npm"
  mv "$target/bin/npx.wrapper" "$target/bin/npx"
  chmod 755 "$target/bin/npm" "$target/bin/npx"
}

stage_git() {
  local target="$RESOURCES/git"
  local archive
  archive="$(fetch_verified \
    "https://mirrors.edge.kernel.org/pub/software/scm/git/git-$GIT_VERSION.tar.xz" \
    "git-$GIT_VERSION.tar.xz" \
    "f689162364c10de79ef89aa8dbf48731eb057e34edbbd20aca510ce0154681a3")"
  if [[ -x "$target/bin/git" && -f "$target/VERSION" ]] &&
      [[ "$(<"$target/VERSION")" == "$GIT_VERSION" ]] &&
      [[ "$("$target/bin/git" --version)" == "git version $GIT_VERSION" ]]; then
    mkdir -p "$target/source"
    cp "$archive" "$target/source/"
    return
  fi
  local staging source install_root
  staging="$(mktemp -d "$CACHE/git-stage.XXXXXX")"
  tar -xf "$archive" -C "$staging"
  source="$staging/git-$GIT_VERSION"
  install_root="$staging/installed"
  local -a flags=(RUNTIME_PREFIX=YesPlease HAVE_NS_GET_EXECUTABLE_PATH=YesPlease
    NO_OPENSSL=YesPlease NO_GETTEXT=YesPlease NO_PERL=YesPlease
    NO_PYTHON=YesPlease NO_TCLTK=YesPlease "prefix=$install_root")
  MACOSX_DEPLOYMENT_TARGET=13.5 make -C "$source" -j8 "${flags[@]}" all > "$staging/build.log" 2>&1 || {
    tail -n 80 "$staging/build.log" >&2
    exit 1
  }
  MACOSX_DEPLOYMENT_TARGET=13.5 make -C "$source" "${flags[@]}" install >> "$staging/build.log" 2>&1 || {
    tail -n 80 "$staging/build.log" >&2
    exit 1
  }
  cp "$source/COPYING" "$install_root/LICENSE-GPL-2.0"
  mkdir -p "$install_root/source"
  cp "$archive" "$install_root/source/"
  print -r -- "$GIT_VERSION" > "$install_root/VERSION"
  rm -rf "$target"
  mv "$install_root" "$target"
  rm -rf "$staging"
}

stage_python() {
  local target="$RESOURCES/python"
  if [[ -x "$target/bin/python3" && -x "$target/bin/pip" ]] &&
      [[ "$("$target/bin/python3" --version)" == "Python $PYTHON_VERSION" ]]; then
    return
  fi
  if ! command -v uv >/dev/null; then
    print -u2 "uv is required on the release build host to stage CPython $PYTHON_VERSION."
    exit 1
  fi
  uv python install "$PYTHON_VERSION" >&2
  local executable source staging
  executable="$(uv python find "$PYTHON_VERSION" --managed-python --no-python-downloads)"
  source="${executable:h:h}"
  print -r -- "386f2851d8e73ea266b232bf1d10b72b30b700f8a8b606e4c1ceed37f1570441  $source/bin/python3.12" | shasum -a 256 -c - >/dev/null
  print -r -- "08821e4b4838d30f3ce4a512172f05befb700bdf4ebff627518350a38e1ffb0b  $source/lib/libpython3.12.dylib" | shasum -a 256 -c - >/dev/null
  staging="$(mktemp -d "$CACHE/python-stage.XXXXXX")"
  ditto "$source" "$staging/python"
  cp "$ROOT/scripts/macos-pip-wrapper" "$staging/python/bin/pip"
  chmod 755 "$staging/python/bin/pip"
  ln -sf pip "$staging/python/bin/pip3"
  ln -sf pip "$staging/python/bin/pip3.12"
  "$staging/python/bin/python3" -c 'import ssl, sqlite3, pip' || exit 1
  print -r -- "$PYTHON_VERSION" > "$staging/python/VERSION"
  rm -rf "$target"
  mv "$staging/python" "$target"
  rmdir "$staging"
}

stage_wecom() {
  local target="$RESOURCES/wecom"
  if [[ -x "$target/bin/wecom-cli" && -f "$target/VERSION" ]] &&
      [[ "$(<"$target/VERSION")" == "$WECOM_VERSION" ]] &&
      [[ "$("$target/bin/wecom-cli" --version)" == "wecom-cli $WECOM_VERSION" ]]; then
    return
  fi
  local archive staging
  archive="$(fetch_verified \
    "https://registry.npmjs.org/@wecom/cli-darwin-arm64/-/cli-darwin-arm64-$WECOM_VERSION.tgz" \
    "wecom-cli-darwin-arm64-$WECOM_VERSION.tgz" \
    "050d251cf0bb55591569af467f9d90292edd5538431b9c5572d55cbc71aa2f33")"
  staging="$(mktemp -d "$CACHE/wecom-stage.XXXXXX")"
  tar -xzf "$archive" -C "$staging"
  mkdir -p "$staging/wecom/bin"
  cp "$staging/package/bin/wecom-cli" "$staging/wecom/bin/wecom-cli"
  cp "$staging/package/LICENSE" "$staging/wecom/LICENSE"
  chmod 755 "$staging/wecom/bin/wecom-cli"
  print -r -- "$WECOM_VERSION" > "$staging/wecom/VERSION"
  rm -rf "$target"
  mv "$staging/wecom" "$target"
  rm -rf "$staging"
}

stage_node
stage_git
stage_python
stage_wecom

[[ "$("$RESOURCES/node/bin/node" --version)" == "$NODE_VERSION" ]]
PATH="$RESOURCES/node/bin:$PATH" "$RESOURCES/node/bin/npm" --version
[[ "$("$RESOURCES/git/bin/git" --version)" == "git version $GIT_VERSION" ]]
"$RESOURCES/python/bin/python3" -c 'import ssl, sqlite3, pip'
"$RESOURCES/python/bin/pip" --version
[[ "$("$RESOURCES/wecom/bin/wecom-cli" --version)" == "wecom-cli $WECOM_VERSION" ]]
print "Staged offline macOS runtimes: Node $NODE_VERSION, Git $GIT_VERSION, Python $PYTHON_VERSION, WeCom $WECOM_VERSION"
