import { describe, expect, it } from "vitest";
import { resolveCanvasRequest } from "./creative";

describe("resolveCanvasRequest", () => {
  it("maps a store poster to the supported 3:4 provider composition and exact archive size", () => {
    expect(resolveCanvasRequest("store-poster")).toEqual({
      canvasPreset: "store-poster",
      outputWidth: 420,
      outputHeight: 592,
      providerRatio: "3:4"
    });
  });

  it("requires dimensions for a custom canvas", () => {
    expect(() => resolveCanvasRequest("custom")).toThrow("自定义比例需要填写宽度和高度");
  });
});
