import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (relative) => readFile(path.join(root, relative), "utf8");

test("Harmony startup never imports the old Piscis application directory or database", async () => {
  const store = await read("src-tauri/src/store/mod.rs");
  assert.doesNotMatch(store, /com\.piscis\.desktop/);
  assert.doesNotMatch(store, /piscis\.db/);
  assert.match(store, /app_dir\.join\("workbuddy\.db"\)/);
});

test("memory paths do not promote Piscis records into Xiaoheng ownership", async () => {
  const [database, host] = await Promise.all([
    read("src-tauri/src/store/db.rs"),
    read("src-tauri/workbuddy-mcp-host/src/lib.rs"),
  ]);
  assert.doesNotMatch(database, /UPDATE memories SET owner_id='xiaoheng' WHERE owner_id='piscis'/);
  assert.doesNotMatch(host, /migrate_legacy_memories\(context, &mut connection\)/);
});

test("workspace journal never copies the old product's undo history", async () => {
  const journal = await read("src-tauri/src/commands/journal.rs");
  assert.equal(journal.includes('root.join(".piscis")'), false);
});

test("consolidation snapshots exclude stale sessions and task spines", async () => {
  const scheduler = await read("src-tauri/src/commands/chat/scheduler.rs");
  assert.match(scheduler, /CONSOLIDATION_MAX_AGE_DAYS/);
  assert.match(scheduler, /recent_for_consolidation/);
});
