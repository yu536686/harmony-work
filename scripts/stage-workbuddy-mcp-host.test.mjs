import assert from "node:assert/strict";
import test from "node:test";
import { mcpHostBinaryName, mcpHostPaths } from "./stage-workbuddy-mcp-host.mjs";

test("uses the Windows executable name for Windows bundles", () => {
  assert.equal(mcpHostBinaryName("win32"), "workbuddy-mcp-host.exe");
  const paths = mcpHostPaths({ platform: "win32", targetTriple: null });
  assert.match(paths.source, /src-tauri\/target\/release\/workbuddy-mcp-host\.exe$/);
  assert.match(paths.destination, /resources\/workbuddy-mcp-host\/workbuddy-mcp-host\.exe$/);
});

test("uses Cargo target directories for explicit cross targets", () => {
  const paths = mcpHostPaths({
    platform: "win32",
    targetTriple: "x86_64-pc-windows-msvc",
  });
  assert.match(
    paths.source,
    /target\/x86_64-pc-windows-msvc\/release\/workbuddy-mcp-host\.exe$/,
  );
});
