import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { SessionArtifact } from "../../services/tauri";
import "../../i18n";
import AnnotationLayer from "./AnnotationLayer";

const artifact: SessionArtifact = {
  id: "artifact-1",
  session_id: "session-1",
  name: "cover.png",
  artifact_type: "image",
  uri: "E:/work/cover.png",
  content_summary: "封面",
  created_at: "2026-09-15T10:00:00.000Z",
};

describe("AnnotationLayer", () => {
  it("stages an image region comment without sending it", async () => {
    const onStage = vi.fn();
    render(
      <AnnotationLayer
        enabled
        sessionId="session-1"
        artifact={artifact}
        previewKind="image"
        annotations={[]}
        onStage={onStage}
      >
        <img src="cover.png" alt="cover" />
      </AnnotationLayer>,
    );
    const canvas = screen.getByLabelText("批注画布");
    vi.spyOn(canvas.parentElement!, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, left: 0, top: 0, right: 400, bottom: 300, width: 400, height: 300,
      toJSON: () => ({}),
    });
    fireEvent.pointerDown(canvas, { clientX: 40, clientY: 30, pointerId: 1 });
    fireEvent.pointerMove(canvas, { clientX: 240, clientY: 180, pointerId: 1 });
    fireEvent.pointerUp(canvas, { clientX: 240, clientY: 180, pointerId: 1 });
    await userEvent.type(screen.getByPlaceholderText("描述希望小亨如何修改所选内容"), "主标题缩小一些");
    await userEvent.click(screen.getByRole("button", { name: "添加到对话" }));

    expect(onStage).toHaveBeenCalledOnce();
    expect(onStage.mock.calls[0][0]).toMatchObject({
      sessionId: "session-1",
      target: { id: "artifact-1" },
      selection: { type: "image_region", x: 0.1, y: 0.1, width: 0.5, height: 0.5 },
      comment: "主标题缩小一些",
      status: "staged",
    });
  });

  it("Escape cancels only the active draft and keeps staged markers", () => {
    render(
      <AnnotationLayer
        enabled
        sessionId="session-1"
        artifact={artifact}
        previewKind="image"
        annotations={[{
          id: "saved",
          sessionId: "session-1",
          target: { type: "artifact", id: artifact.id, title: artifact.name, uri: artifact.uri },
          selection: { type: "point", page: 1, x: 0.2, y: 0.2 },
          comment: "保留批注",
          status: "staged",
        }]}
        focusedAnnotationId="saved"
        onStage={vi.fn()}
      >
        <img src="cover.png" alt="cover" />
      </AnnotationLayer>,
    );
    expect(screen.getByRole("button", { name: "批注 1" })).toBeVisible();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByPlaceholderText("描述希望小亨如何修改所选内容")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "批注 1" })).toBeVisible();
  });
});
