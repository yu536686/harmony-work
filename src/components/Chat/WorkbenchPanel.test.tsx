import { render, screen } from "@testing-library/react";
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
    expect(screen.getByText("七夕推广方案.md")).toBeVisible();

    await userEvent.click(screen.getByRole("tab", { name: "终端" }));
    expect(screen.getByText(/Verification passed/)).toBeVisible();
    expect(screen.getByText("npm test")).toBeVisible();
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
});
