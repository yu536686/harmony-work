import { describe, expect, it } from "vitest";
import {
  addAnnotation,
  annotationAnchor,
  emptyAnnotationDraft,
  markAnnotationSendFailed,
  markAnnotationSending,
  removeAnnotation,
  toCommentContext,
} from "./reducer";
import type { WorkbenchAnnotation } from "./types";

const annotation: WorkbenchAnnotation = {
  id: "annotation-1",
  sessionId: "session-1",
  target: { type: "artifact", id: "artifact-1", title: "方案.pptx", uri: "E:/work/方案.pptx" },
  selection: { type: "image_region", page: 1, x: 0.18, y: 0.27, width: 0.62, height: 0.47 },
  comment: "标题更简洁，主视觉换成腕表微距",
  status: "staged",
};

const browserAnnotation: WorkbenchAnnotation = {
  id: "annotation-browser-1",
  sessionId: "session-1",
  target: {
    type: "browser",
    id: "browser-step-1",
    title: "品牌资料页面",
    uri: "https://example.com/brand",
  },
  selection: { type: "page", url: "https://example.com/brand" },
  comment: "请以这一页的最新产品信息更新方案。",
  status: "staged",
};

describe("annotation draft reducer", () => {
  it("adds or updates an annotation without duplicating its id", () => {
    const added = addAnnotation(emptyAnnotationDraft, annotation);
    const updated = addAnnotation(added, { ...annotation, comment: "更新标题" });
    expect(updated.items).toHaveLength(1);
    expect(updated.items[0].comment).toBe("更新标题");
  });

  it("removes one annotation and keeps the remaining draft", () => {
    const state = addAnnotation(addAnnotation(emptyAnnotationDraft, annotation), { ...annotation, id: "annotation-2" });
    expect(removeAnnotation(state, "annotation-1").items.map((item) => item.id)).toEqual(["annotation-2"]);
  });

  it("strips UI-only state from the model context", () => {
    const context = toCommentContext(annotation);
    expect(context).not.toHaveProperty("status");
    expect(context).not.toHaveProperty("sessionId");
    expect(context.comment).toBe(annotation.comment);
  });

  it("keeps browser URL comments as structured model context", () => {
    const context = toCommentContext(browserAnnotation);
    expect(context.target.type).toBe("browser");
    expect(context.selection).toEqual({ type: "page", url: "https://example.com/brand" });
  });

  it("marks all staged comments as failed without dropping them", () => {
    const failed = markAnnotationSendFailed(addAnnotation(emptyAnnotationDraft, annotation));
    expect(failed.sendState).toBe("failed");
    expect(failed.items[0].status).toBe("failed");
  });

  it("keeps comments while their turn is running", () => {
    const sending = markAnnotationSending(addAnnotation(emptyAnnotationDraft, annotation));
    expect(sending.sendState).toBe("sending");
    expect(sending.items).toEqual([annotation]);
  });

  it("builds a compact numbered anchor", () => {
    expect(annotationAnchor(annotation, 0)).toBe("批注 1 · 第 1 页");
  });
});
