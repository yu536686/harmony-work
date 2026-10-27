export interface DiffLine { kind: "same" | "removed" | "added" | "omitted"; text: string; before: number | null; after: number | null }

// A bounded prefix/suffix line diff. The changed middle is shown as removals
// followed by additions, avoiding a quadratic LCS on arbitrary user files.
export function lineDiff(before: string, after: string, limit = 600): DiffLine[] {
  const oldLines = before === "" ? [] : before.split("\n");
  const newLines = after === "" ? [] : after.split("\n");
  let prefix = 0;
  while (prefix < oldLines.length && prefix < newLines.length && oldLines[prefix] === newLines[prefix]) prefix++;
  let suffix = 0;
  while (suffix < oldLines.length - prefix && suffix < newLines.length - prefix && oldLines[oldLines.length - suffix - 1] === newLines[newLines.length - suffix - 1]) suffix++;
  const out: DiffLine[] = [];
  const omitted = () => out.push({ kind: "omitted", text: "部分行已折叠", before: null, after: null });
  if (prefix > 3) omitted();
  for (let n = Math.max(0, prefix - 3); n < prefix; n++) out.push({ kind: "same", text: oldLines[n], before: n + 1, after: n + 1 });
  const budget = Math.max(2, Math.floor((limit - 10) / 2));
  const removed = oldLines.length - suffix - prefix;
  const added = newLines.length - suffix - prefix;
  for (let n = prefix; n < Math.min(oldLines.length - suffix, prefix + budget); n++) out.push({ kind: "removed", text: oldLines[n], before: n + 1, after: null });
  if (removed > budget) omitted();
  for (let n = prefix; n < Math.min(newLines.length - suffix, prefix + budget); n++) out.push({ kind: "added", text: newLines[n], before: null, after: n + 1 });
  if (added > budget) omitted();
  for (let n = 0; n < Math.min(3, suffix); n++) out.push({ kind: "same", text: oldLines[oldLines.length - suffix + n], before: oldLines.length - suffix + n + 1, after: newLines.length - suffix + n + 1 });
  if (suffix > 3) omitted();
  return out;
}
