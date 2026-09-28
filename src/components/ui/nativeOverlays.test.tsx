import { useRef } from "react";
import { render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useMacNativeAnnotation } from "../Chat/annotation/useMacNativeAnnotation";
import { useMacNativeDialog } from "./useMacNativeDialog";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), isTauri: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => bridge);

function Annotation({ onStage, onClose }: { onStage: (value: string) => void; onClose: () => void }) {
  const rootRef = useRef<HTMLDivElement>(null);
  useMacNativeAnnotation({
    draft: { left: 12, top: 20 }, rootRef, initialComment: "old",
    title: "批注", stageLabel: "添加", cancelLabel: "取消", onStage, onClose,
  });
  return <div ref={rootRef}>成果</div>;
}

function Dialog({ onConfirm, onCancel, onExtra }: { onConfirm: (value: string) => void; onCancel: () => void; onExtra?: () => void }) {
  useMacNativeDialog({
    open: true, loading: false, title: "新建分支", message: "名称",
    confirmLabel: "创建", cancelLabel: "取消", prompt: true,
    value: "main", extraLabel: onExtra ? "清除" : undefined, onConfirm, onCancel, onExtra,
  });
  return null;
}

describe("native annotation and sheet events", () => {
  const originalUserAgent = navigator.userAgent;

  beforeEach(() => {
    Object.defineProperty(navigator, "userAgent", { configurable: true, value: "Macintosh" });
    bridge.isTauri.mockReturnValue(true);
    bridge.invoke.mockResolvedValue(undefined);
  });

  afterEach(() => {
    Object.defineProperty(navigator, "userAgent", { configurable: true, value: originalUserAgent });
    vi.clearAllMocks();
  });

  it("stages native annotation text through the existing callback", async () => {
    const onStage = vi.fn();
    render(<Annotation onStage={onStage} onClose={vi.fn()} />);
    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("show_macos_annotation", expect.objectContaining({ comment: "old" })));
    const { annotationId } = bridge.invoke.mock.calls.find(([command]) => command === "show_macos_annotation")![1];
    window.dispatchEvent(new CustomEvent("harmony:native-annotation-stage", { detail: { annotationId, comment: "请调整这里" } }));
    expect(onStage).toHaveBeenCalledWith("请调整这里");
  });

  it("delivers the value from an AppKit input sheet", async () => {
    const onConfirm = vi.fn();
    render(<Dialog onConfirm={onConfirm} onCancel={vi.fn()} />);
    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("show_macos_dialog", expect.objectContaining({ prompt: true })));
    const { dialogId } = bridge.invoke.mock.calls.find(([command]) => command === "show_macos_dialog")![1];
    window.dispatchEvent(new CustomEvent("harmony:native-dialog-result", { detail: { dialogId, accepted: true, value: "feature/mac" } }));
    expect(onConfirm).toHaveBeenCalledWith("feature/mac");
  });

  it("routes the third native sheet action without confirming", async () => {
    const onExtra = vi.fn();
    const onConfirm = vi.fn();
    render(<Dialog onConfirm={onConfirm} onCancel={vi.fn()} onExtra={onExtra} />);
    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("show_macos_dialog", expect.objectContaining({ extraLabel: "清除" })));
    const { dialogId } = bridge.invoke.mock.calls.find(([command]) => command === "show_macos_dialog")![1];
    window.dispatchEvent(new CustomEvent("harmony:native-dialog-result", { detail: { dialogId, accepted: false, extra: true, value: "" } }));
    expect(onExtra).toHaveBeenCalledOnce();
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
