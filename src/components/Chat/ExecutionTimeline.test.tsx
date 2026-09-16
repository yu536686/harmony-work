import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import "../../i18n";
import type { TurnExecutionModel } from "./execution/types";
import ExecutionTimeline from "./ExecutionTimeline";

const model = (overrides: Partial<TurnExecutionModel> = {}): TurnExecutionModel => ({
  sessionId: "session-1",
  state: "running",
  iteration: 1,
  reasoning: [],
  plan: [
    { id: "plan-1", content: "读取并整理素材", status: "completed" },
    { id: "plan-2", content: "生成并验证", status: "in_progress" },
  ],
  steps: [
    {
      id: "tool-1",
      kind: "tool",
      name: "file_read",
      state: "completed",
      summary: "已读取 12 个文件",
      input: { path: "E:/workspace/materials" },
      result: "读取完成",
      expanded: false,
    },
    {
      id: "tool-2",
      kind: "tool",
      name: "shell",
      state: "running",
      summary: "npm test -- --run",
      input: { command: "npm test -- --run" },
      expanded: true,
    },
  ],
  finalText: "",
  artifacts: [],
  ...overrides,
});

describe("ExecutionTimeline", () => {
  it("呈现明确状态，完成步骤默认折叠且可展开", async () => {
    const onToggleStep = vi.fn();
    const current = model();
    const { rerender } = render(
      <ExecutionTimeline model={current} onToggleStep={onToggleStep} />,
    );

    expect(screen.getAllByLabelText("已完成").length).toBeGreaterThan(0);
    expect(screen.getAllByLabelText("执行中").length).toBeGreaterThan(0);
    expect(screen.queryByText("E:/workspace/materials")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /读取文件/ }));
    expect(onToggleStep).toHaveBeenCalledWith("tool-1");

    rerender(
      <ExecutionTimeline
        model={{
          ...current,
          steps: current.steps.map((step) =>
            step.id === "tool-1" ? { ...step, expanded: true } : step,
          ),
        }}
        onToggleStep={onToggleStep}
      />,
    );
    expect(screen.getByText("E:/workspace/materials")).toBeVisible();
  });

  it("allows a keyboard user to expand a completed step", async () => {
    const onToggleStep = vi.fn();
    render(<ExecutionTimeline model={model()} onToggleStep={onToggleStep} />);

    const step = screen.getByRole("button", { name: /读取文件/ });
    step.focus();
    await userEvent.keyboard("{Enter}");

    expect(onToggleStep).toHaveBeenCalledWith("tool-1");
  });

  it("只显示公开思考摘要，不读取未知 raw reasoning 字段", async () => {
    const completed = model({
      state: "completed",
      reasoning: [
        {
          id: "reasoning-1",
          summaryIndex: 0,
          text: "已完成素材核对并确定页面结构。",
          streaming: false,
        },
      ],
    }) as TurnExecutionModel & { rawReasoning?: string };
    completed.rawReasoning = "不应显示的隐藏推理";

    render(<ExecutionTimeline model={completed} onToggleStep={vi.fn()} />);

    expect(screen.queryByText("已完成素材核对并确定页面结构。")).not.toBeInTheDocument();
    expect(screen.queryByText("不应显示的隐藏推理")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /思考过程/ }));
    expect(screen.getByText("已完成素材核对并确定页面结构。")).toBeVisible();
    expect(screen.queryByText("不应显示的隐藏推理")).not.toBeInTheDocument();
  });

  it("流式摘要在当前轮次保持展开并标记正在思考", () => {
    render(
      <ExecutionTimeline
        model={model({
          reasoning: [
            {
              id: "reasoning-1",
              summaryIndex: 0,
              text: "正在对比参考素材",
              streaming: true,
            },
          ],
        })}
        onToggleStep={vi.fn()}
      />,
    );

    expect(screen.getByText("正在对比参考素材")).toBeVisible();
    expect(screen.getByLabelText("正在思考")).toBeVisible();
  });

  it("空闲且没有内容时不渲染占位面板", () => {
    const { container } = render(
      <ExecutionTimeline
        model={model({ state: "idle", plan: [], steps: [], reasoning: [] })}
        onToggleStep={vi.fn()}
      />,
    );

    expect(container).toBeEmptyDOMElement();
  });
});
