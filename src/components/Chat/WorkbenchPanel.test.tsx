import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import "../../i18n";
import type { SessionArtifact } from "../../services/tauri";
import type { TurnExecutionModel } from "./execution/types";
import WorkbenchPanel from "./WorkbenchPanel";

const artifact: SessionArtifact = {
  id: "artifact-1",
  session_id: "session-1",
  name: "七夕推广方案.md",
  artifact_type: "document",
  uri: "E:/workspace/七夕推广方案.md",
  content_summary: "六页推广方案",
  source_tool: "artifact_render",
  tool_use_id: "tool-2",
  metadata_json: null,
  created_at: "2026-09-15T10:00:00.000Z",
};

const secondArtifact: SessionArtifact = {
  ...artifact,
  id: "artifact-2",
  name: "七夕门店执行清单.pdf",
  uri: "E:/workspace/七夕门店执行清单.pdf",
  artifact_type: "pdf",
  content_summary: "门店执行清单",
};

const execution: TurnExecutionModel = {
  sessionId: "session-1",
  state: "completed",
  iteration: 1,
  reasoning: [],
  plan: [{ id: "plan-1", content: "生成推广方案", status: "completed" }],
  steps: [
    {
      id: "tool-1",
      kind: "tool",
      name: "shell",
      state: "completed",
      summary: "Verification passed",
      input: { command: "npm test" },
      result: "Verification passed: 12 tests",
      expanded: false,
    },
    {
      id: "tool-2",
      kind: "tool",
      name: "browser",
      state: "completed",
      summary: "已读取官网",
      input: { url: "https://example.com/watch" },
      result: "200 OK",
      expanded: false,
    },
  ],
  finalText: "任务已完成",
  artifacts: [],
};

describe("WorkbenchPanel", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockResolvedValue({
      kind: "text",
      uri: artifact.uri,
      content: "# 七夕推广方案",
      media_type: "text/markdown",
      truncated: false,
      size_bytes: 18,
    });
  });

  it("默认展示成果，并可切换到终端记录", async () => {
    render(
      <WorkbenchPanel
        open
        sessionId="session-1"
        artifacts={[artifact]}
        execution={execution}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByRole("tab", { name: "成果" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("button", { name: "打开 七夕推广方案.md" })).toBeVisible();

    await userEvent.click(screen.getByRole("tab", { name: "终端" }));
    expect(screen.getByText(/Verification passed/)).toBeVisible();
    expect(screen.getByText("npm test")).toBeVisible();
  });

  it("uses an output list and opens a preview tab only after the user selects an output", async () => {
    render(
      <WorkbenchPanel
        open
        sessionId="session-1"
        artifacts={[artifact, secondArtifact]}
        execution={execution}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByRole("list", { name: "输出内容" })).toBeVisible();
    expect(screen.getByRole("button", { name: "打开 七夕推广方案.md" })).toBeVisible();
    expect(screen.getByRole("button", { name: "打开 七夕门店执行清单.pdf" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "七夕推广方案.md" })).toBeVisible();
    expect(screen.queryByRole("tab", { name: "七夕门店执行清单.pdf" })).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "打开 七夕门店执行清单.pdf" }));
    expect(screen.getByRole("tab", { name: "七夕门店执行清单.pdf" })).toBeVisible();
    expect(screen.getByLabelText("七夕门店执行清单.pdf 预览")).toBeVisible();
  });

  it("honors the caller-selected panel width", () => {
    render(
      <WorkbenchPanel
        open
        width={760}
        sessionId="session-1"
        artifacts={[]}
        execution={execution}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByLabelText("工具面板")).toHaveStyle({
      width: "760px",
      minWidth: "760px",
      flexBasis: "760px",
    });
  });

  it("展示结构化摘要和浏览器目标", async () => {
    render(
      <WorkbenchPanel
        open
        sessionId="session-1"
        artifacts={[artifact]}
        execution={execution}
        onClose={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("tab", { name: "摘要" }));
    expect(screen.getByText("生成推广方案")).toBeVisible();
    expect(screen.getByText("任务已完成")).toBeVisible();

    await userEvent.click(screen.getByRole("tab", { name: "浏览器" }));
    expect(screen.getByText("https://example.com/watch")).toBeVisible();
  });

  it("can focus a chat-opened browser URL in the workbench", async () => {
    render(
      <WorkbenchPanel
        open
        sessionId="session-1"
        artifacts={[]}
        execution={null}
        browserTarget={{ title: "腕表官网", url: "https://example.com/watch" }}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByRole("tab", { name: "浏览器" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTitle("腕表官网")).toHaveAttribute("src", "https://example.com/watch");
  });

  it("can stage a URL-level browser annotation for the next turn", async () => {
    const onStageAnnotation = vi.fn();
    render(
      <WorkbenchPanel
        open
        sessionId="session-1"
        artifacts={[]}
        execution={execution}
        onStageAnnotation={onStageAnnotation}
        onClose={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("tab", { name: "浏览器" }));
    await userEvent.click(screen.getByRole("button", { name: "批注页面" }));
    await userEvent.type(screen.getByPlaceholderText("描述希望小亨如何修改所选内容"), "以页面最新信息更新方案");
    await userEvent.click(screen.getByRole("button", { name: "添加到对话" }));

    expect(onStageAnnotation).toHaveBeenCalledWith(expect.objectContaining({
      target: expect.objectContaining({ type: "browser", uri: "https://example.com/watch" }),
      selection: { type: "page", url: "https://example.com/watch" },
      comment: "以页面最新信息更新方案",
    }));
  });

  it("stages a selected browser viewport region for the next turn", async () => {
    const onStageAnnotation = vi.fn();
    render(
      <WorkbenchPanel
        open
        sessionId="session-1"
        artifacts={[]}
        execution={null}
        browserTarget={{ id: "watch-site", title: "腕表官网", url: "https://example.com/watch" }}
        onStageAnnotation={onStageAnnotation}
        onClose={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "开始注释" }));
    const canvas = screen.getByLabelText("浏览器批注画布");
    vi.spyOn(canvas.parentElement as HTMLElement, "getBoundingClientRect").mockReturnValue({
      x: 0, y: 0, left: 0, top: 0, right: 500, bottom: 400, width: 500, height: 400,
      toJSON: () => ({}),
    });
    fireEvent.pointerDown(canvas, { pointerId: 1, clientX: 40, clientY: 40 });
    fireEvent.pointerMove(canvas, { pointerId: 1, clientX: 240, clientY: 160 });
    fireEvent.pointerUp(canvas, { pointerId: 1, clientX: 240, clientY: 160 });
    await userEvent.type(screen.getByPlaceholderText("描述希望小亨如何修改所选内容"), "将这里改为更简洁的版式");
    await userEvent.click(screen.getByRole("button", { name: "添加到对话" }));

    const annotation = onStageAnnotation.mock.calls[0]?.[0];
    expect(annotation).toMatchObject({
      target: { type: "browser", id: "watch-site", uri: "https://example.com/watch" },
      selection: { type: "image_region" },
      comment: "将这里改为更简洁的版式",
    });
    if (!annotation || annotation.selection.type !== "image_region") throw new Error("expected a browser region annotation");
    expect(annotation.selection.x).toBeCloseTo(0.08);
    expect(annotation.selection.y).toBeCloseTo(0.1);
    expect(annotation.selection.width).toBeCloseTo(0.4);
    expect(annotation.selection.height).toBeCloseTo(0.3);
  });

  it("does not accept new annotations while the current turn is running", async () => {
    render(
      <WorkbenchPanel
        open
        sessionId="session-1"
        artifacts={[]}
        execution={execution}
        running
        onClose={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("tab", { name: "浏览器" }));
    expect(screen.getByRole("button", { name: "批注页面" })).toBeDisabled();
  });

  it("关闭按钮保持为明确命令", async () => {
    const onClose = vi.fn();
    render(
      <WorkbenchPanel
        open
        sessionId="session-1"
        artifacts={[]}
        execution={null}
        onClose={onClose}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "关闭工具面板" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("allows a keyboard user to switch workbench views", async () => {
    render(
      <WorkbenchPanel
        open
        sessionId="session-1"
        artifacts={[]}
        execution={execution}
        onClose={vi.fn()}
      />,
    );

    const browserTab = screen.getByRole("tab", { name: "浏览器" });
    browserTab.focus();
    await userEvent.keyboard(" ");
    expect(browserTab).toHaveAttribute("aria-selected", "true");
  });

  it("关闭时不占用页面布局", () => {
    const { container } = render(
      <WorkbenchPanel
        open={false}
        sessionId="session-1"
        artifacts={[]}
        execution={null}
        onClose={vi.fn()}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("空成果和预览失败都保留在可滚动的工作台内容区域", async () => {
    const { container, rerender } = render(
      <WorkbenchPanel
        open
        sessionId="session-1"
        artifacts={[]}
        execution={null}
        onClose={vi.fn()}
      />,
    );

    expect(container.querySelector(".workbench-content .workbench-empty")).toBeInTheDocument();

    vi.mocked(invoke).mockRejectedValueOnce(new Error("超长预览读取失败"));
    rerender(
      <WorkbenchPanel
        open
        sessionId="session-1"
        artifacts={[artifact]}
        execution={execution}
        onClose={vi.fn()}
      />,
    );

    const error = await screen.findByRole("alert");
    expect(error).toHaveClass("artifact-preview-state", "is-error");
    expect(error).toHaveTextContent("超长预览读取失败");
  });
});
