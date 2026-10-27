import { describe, expect, it } from "vitest";
import { lineDiff } from "./lineDiff";

describe("bounded line diff", () => {
  it("represents additions, deletions and unchanged lines with source positions", () => {
    expect(lineDiff("", "new")).toEqual([{ kind: "added", text: "new", before: null, after: 1 }]);
    expect(lineDiff("gone", "")).toEqual([{ kind: "removed", text: "gone", before: 1, after: null }]);
    expect(lineDiff("same\nold", "same\nnew").map(row => row.kind)).toEqual(["same", "removed", "added"]);
  });
  it("caps DOM work and folds long common context without quadratic matching", () => {
    const rows = lineDiff(Array(4000).fill("before").join("\n"), Array(4000).fill("after").join("\n"));
    expect(rows.length).toBeLessThanOrEqual(600);
    expect(rows.some(row => row.kind === "omitted")).toBe(true);
    const context = lineDiff(`${"context\n".repeat(1000)}old`, `${"context\n".repeat(1000)}new`);
    expect(context.length).toBeLessThan(10);
  });
});
