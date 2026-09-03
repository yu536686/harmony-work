import assert from "node:assert/strict";
import test from "node:test";
import { codexSidecarPaths, codexTarget } from "./stage-codex-sidecar.mjs";

test("maps supported platforms to Codex native package layouts", () => {
  assert.deepEqual(codexTarget("darwin", "arm64"), [
    "darwin-arm64",
    "aarch64-apple-darwin",
    "codex",
  ]);
  assert.deepEqual(codexTarget("win32", "x64"), [
    "win32-x64",
    "x86_64-pc-windows-msvc",
    "codex.exe",
  ]);
});

test("stages the sidecar under the Tauri resource tree", () => {
  const paths = codexSidecarPaths("linux", "x64");
  const source = paths.source.replaceAll("\\", "/");
  const destination = paths.destination.replaceAll("\\", "/");
  assert.match(source, /@openai\/codex-linux-x64\/vendor\/x86_64-unknown-linux-musl/);
  assert.match(destination, /src-tauri\/resources\/codex\/codex$/);
});

test("rejects an unsupported target", () => {
  assert.throws(() => codexTarget("freebsd", "x64"), /Unsupported Codex sidecar target/);
});
