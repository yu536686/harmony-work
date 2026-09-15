import { describe, expect, it } from "vitest";
import { loadHarvisTasks, normalizeReferenceSelection, resolveCanvasRequest } from "./creative";

it("migrates cached public result URLs without changing supplier signed URLs", () => {
  const key = "workbuddy.harvis.tasks";
  const tasks = [
    { taskId: "old", resultUrl: "https://api.harmonywatch.cn:13001/output/a.png?token=x%2Fy" },
    { taskId: "supplier", resultUrl: "https://cdn.example.com/a.mp3?signature=x%2Fy" }
  ];
  localStorage.setItem(key, JSON.stringify(tasks));
  try {
    expect(loadHarvisTasks().map(task => task.resultUrl)).toEqual([
      "https://api.harmonywatch.cn/output/a.png?token=x%2Fy",
      tasks[1].resultUrl
    ]);
  } finally {
    localStorage.removeItem(key);
  }
});

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

  it("keeps at most four unique reference images in their selected order", () => {
    expect(normalizeReferenceSelection(["a", "b", "a", "c", "d", "e"])).toEqual(["b", "c", "d", "e"]);
  });
});
