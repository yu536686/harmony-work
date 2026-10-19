import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

async function readProjectFile(relativePath) {
  return readFile(path.join(projectRoot, relativePath), "utf8");
}

test("conversation workbench keeps an output list and bounded preview tabs inside its panel", async () => {
  const [css, panel] = await Promise.all([
    readProjectFile("src/components/Chat/ConversationExperience.css"),
    readProjectFile("src/components/Chat/WorkbenchPanel.tsx"),
  ]);

  assert.match(panel, /workbench-output-list/);
  assert.match(panel, /workbench-preview-tabs/);
  assert.match(css, /\.workbench-output-list\s*\{[\s\S]*?display:\s*grid;/);
  assert.match(css, /\.workbench-preview-tab\s*\{[\s\S]*?max-width:\s*180px;/);
  assert.match(css, /\.workbench-preview-tab span\s*\{[\s\S]*?text-overflow:\s*ellipsis;/);
  assert.match(css, /\.workbench-log-command\s*\{[\s\S]*?overflow-wrap:\s*anywhere;/);
  assert.match(css, /\.workbench-log-list pre\s*\{[\s\S]*?overflow-wrap:\s*anywhere;/);
  assert.match(css, /\.workbench-summary li span,[\s\S]*?\.workbench-log-title strong\s*\{[\s\S]*?min-width:\s*0;[\s\S]*?overflow-wrap:\s*anywhere;/);
});

test("conversation annotation and narrow workbench rules remain bounded", async () => {
  const [css, runtime] = await Promise.all([
    readProjectFile("src/components/Chat/ConversationExperience.css"),
    readProjectFile("src-tauri/src/codex_runtime/service.rs"),
  ]);

  assert.match(css, /\.annotation-popover\s*\{[\s\S]*?width:\s*min\(248px,\s*calc\(100% - 16px\)\)/);
  assert.match(css, /@media \(max-width: 980px\)\s*\{[\s\S]*?\.workbench-panel\s*\{[\s\S]*?width:\s*min\(440px,\s*92vw\)(?:\s*!important)?;[\s\S]*?min-width:\s*0(?:\s*!important)?;/);
  assert.match(runtime, /const MAX_ANNOTATION_ITEMS:\s*usize\s*=\s*20;/);
});

test("approval overlays stay above the composer and workbench", async () => {
  const [chatCss, experienceCss] = await Promise.all([
    readProjectFile("src/components/Chat/Chat.css"),
    readProjectFile("src/components/Chat/ConversationExperience.css"),
  ]);

  const composerZIndex = Number(chatCss.match(/\.input-area\s*\{[\s\S]*?z-index:\s*(\d+);/)?.[1]);
  const permissionZIndex = Number(chatCss.match(/\.permission-overlay\s*\{[\s\S]*?z-index:\s*(\d+);/)?.[1]);
  const workbenchZIndex = Number(experienceCss.match(/@media \(max-width: 980px\)\s*\{[\s\S]*?\.workbench-panel\s*\{[\s\S]*?z-index:\s*(\d+);/)?.[1]);

  assert.ok(composerZIndex > 0, "composer must declare a stacking level");
  assert.ok(workbenchZIndex > composerZIndex, "narrow workbench must cover the composer when open");
  assert.ok(permissionZIndex > workbenchZIndex, "permission overlay must stay above the workbench and composer");
});

test("conversation keeps its last card above the measured composer", async () => {
  const [chat, css] = await Promise.all([
    readProjectFile("src/components/Chat/index.tsx"),
    readProjectFile("src/components/Chat/Chat.css"),
  ]);

  assert.match(chat, /ResizeObserver/);
  assert.match(chat, /--composer-clearance/);
  assert.match(css, /\.messages-area\s*\{[\s\S]*?padding-bottom:\s*max\([^;]*var\(--composer-clearance/);
});
