import { describe, expect, it } from "vitest";
import { pointerSelection, textRangeSelection } from "./selection";

describe("annotation selection geometry", () => {
  it("normalizes a rectangle dragged in either direction", () => {
    const selection = pointerSelection(
      { x: 90, y: 80 },
      { x: 30, y: 20 },
      { left: 10, top: 10, width: 100, height: 100 },
    );
    expect(selection.type).toBe("image_region");
    if (selection.type !== "image_region") return;
    expect(selection.x).toBeCloseTo(0.2);
    expect(selection.y).toBeCloseTo(0.1);
    expect(selection.width).toBeCloseTo(0.6);
    expect(selection.height).toBeCloseTo(0.6);
  });

  it("clamps coordinates and treats a tiny drag as a point", () => {
    expect(pointerSelection(
      { x: -10, y: -10 },
      { x: -8, y: -7 },
      { left: 0, top: 0, width: 100, height: 100 },
    )).toEqual({ type: "point", page: 1, x: 0, y: 0 });
  });

  it("stores normalized offsets, line numbers, and a bounded excerpt", () => {
    const content = `first\n${"a".repeat(600)}\nlast`;
    const selection = textRangeSelection(content, 606, 6);
    expect(selection.type).toBe("text_range");
    if (selection.type !== "text_range") return;
    expect(selection.start).toBe(6);
    expect(selection.end).toBe(606);
    expect(selection.startLine).toBe(2);
    expect(selection.endLine).toBe(2);
    expect(selection.excerpt).toHaveLength(500);
  });
});
