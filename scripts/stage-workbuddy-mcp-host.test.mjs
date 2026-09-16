import assert from "node:assert/strict";
import test from "node:test";
import { mcpHostBinaryName, mcpHostPaths } from "./stage-workbuddy-mcp-host.mjs";

const posixPath = (value) => value.replace(/\\/g, "/");

test("uses the Windows executable name for Windows bundles", () => {
  assert.equal(mcpHostBinaryName("win32"), "workbuddy-mcp-host.exe");
  const paths = mcpHostPaths({ platform: "win32", targetTriple: null });
  assert.match(posixPath(paths.source), /src-tauri\/target\/release\/workbuddy-mcp-host\.exe$/);
  assert.match(posixPath(paths.destination), /resources\/workbuddy-mcp-host\/workbuddy-mcp-host\.exe$/);
});

test("uses Cargo target directories for explicit cross targets", () => {
  const paths = mcpHostPaths({
    platform: "win32",
    targetTriple: "x86_64-pc-windows-msvc",
  });
  assert.match(
    posixPath(paths.source),
    /target\/x86_64-pc-windows-msvc\/release\/workbuddy-mcp-host\.exe$/,
  );
});

test("uses a configured Cargo target directory", () => {
  const paths = mcpHostPaths({
    platform: "win32",
    targetTriple: null,
    cargoTargetDirectory: "E:/xiaoheng-cargo-target",
  });
  assert.match(
    posixPath(paths.source),
    /xiaoheng-cargo-target\/release\/workbuddy-mcp-host\.exe$/,
  );
});
