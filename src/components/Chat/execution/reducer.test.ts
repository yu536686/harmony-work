import { describe, expect, it } from "vitest";
import type { AgentEventType, TurnExecutionEventRecord } from "../../../services/tauri/chat";
import {
  MAX_TOOL_RESULT_CHARS,
  createTurnExecutionModel,
  isTurnTerminal,
  reduceExecutionEvent,
  restoreExecutionHistory,
  toggleExecutionStep,
} from "./reducer";
import type { TurnExecutionModel } from "./types";

/** 固定时间戳，保证聚合器在测试中完全可复现。 */
const NOW = "2026-09-15T10:00:00.000Z";

const reduce = (model: TurnExecutionModel, event: AgentEventType) =>
  reduceExecutionEvent(model, event, NOW);

const startTurn = (model: TurnExecutionModel, iteration = 1) =>
  reduce(model, { type: "run_status", iteration, phase: "thinking", detail: "正在处理任务" });

const toolStart = (id: string, name = "shell", input: unknown = { command: "npm test" }): AgentEventType =>
  ({ type: "tool_start", id, name, input });

const toolEnd = (id: string, result = "PASS", isError = false, name = "shell"): AgentEventType =>
  ({ type: "tool_end", id, name, result, is_error: isError });

const record = (
  sequence: number,
  event_type: string,
  payload: unknown,
  turn_id = "turn-1",
): TurnExecutionEventRecord => ({
  id: `rec-${sequence}`,
  session_id: "session-1",
  turn_id,
  sequence,
  event_type,
  payload,
  created_at: NOW,
});

describe("TurnExecutionModel 聚合", () => {
  it("按计划步骤、工具启动与结束推进到已完成", () => {
    const initial = createTurnExecutionModel("session-1");
    const started = startTurn(initial);
    const planned = reduce(started, {
      type: "plan_update",
      items: [{ id: "step-1", content: "读取文件", status: "in_progress" }],
    });
    const running = reduce(planned, toolStart("tool-1"));
    const done = reduce(running, toolEnd("tool-1"));

    expect(done.steps[0].state).toBe("completed");
    expect(done.steps[0].expanded).toBe(false);
    // 计划项与执行步骤分属两个集合，计划不会挤占 steps 的下标
    expect(done.plan).toHaveLength(1);
    expect(done.steps).toHaveLength(1);
  });

  it("初始模型处于 idle 且不显示为空时间线", () => {
    const initial = createTurnExecutionModel("session-1");

    expect(initial.state).toBe("idle");
    expect(initial.steps).toEqual([]);
    expect(initial.reasoning).toEqual([]);
    expect(initial.finalText).toBe("");
    expect(initial.artifacts).toEqual([]);
  });

  it("重复事件幂等", () => {
    const running = reduce(startTurn(createTurnExecutionModel("session-1")), toolStart("tool-1"));
    const duplicatedStart = reduce(running, toolStart("tool-1"));
    expect(duplicatedStart.steps).toHaveLength(1);

    const done = reduce(duplicatedStart, toolEnd("tool-1"));
    const duplicatedEnd = reduce(done, toolEnd("tool-1"));
    expect(duplicatedEnd.steps).toHaveLength(1);
    expect(duplicatedEnd.steps[0].result).toBe("PASS");
    expect(duplicatedEnd.steps[0].state).toBe("completed");
  });

  it("tool_end 先于 tool_start 也能得到已完成步骤", () => {
    const model = reduce(startTurn(createTurnExecutionModel("session-1")), toolEnd("tool-9", "done"));

    expect(model.steps).toHaveLength(1);
    expect(model.steps[0].id).toBe("tool-9");
    expect(model.steps[0].state).toBe("completed");
    expect(model.steps[0].expanded).toBe(false);
  });

  it("工具失败时步骤标记为 failed 且保持展开", () => {
    const running = reduce(startTurn(createTurnExecutionModel("session-1")), toolStart("tool-1"));
    const failed = reduce(running, toolEnd("tool-1", "boom", true));

    expect(failed.steps[0].state).toBe("failed");
    expect(failed.steps[0].isError).toBe(true);
    expect(failed.steps[0].expanded).toBe(true);
    // 单个工具失败不等于整轮失败
    expect(failed.state).toBe("running");
  });

  it("等待批准时整轮进入 awaiting_approval", () => {
    const running = reduce(startTurn(createTurnExecutionModel("session-1")), toolStart("tool-1"));
    const waiting = reduce(running, {
      type: "permission_request",
      request_id: "perm-1",
      tool_name: "shell",
      tool_input: { command: "rm -rf build" },
      description: "需要执行删除命令",
    });

    expect(waiting.state).toBe("awaiting_approval");
    expect(waiting.steps.some((step) => step.id === "perm-1" && step.state === "awaiting_approval")).toBe(true);
  });

  it("done 结束运行状态并停止摘要流式标记", () => {
    let model = startTurn(createTurnExecutionModel("session-1"));
    model = reduce(model, { type: "reasoning_summary_start", id: "r1", summary_index: 0 });
    model = reduce(model, { type: "reasoning_summary_delta", id: "r1", summary_index: 0, delta: "正在核对素材" });
    expect(model.reasoning[0].streaming).toBe(true);

    model = reduce(model, { type: "done", total_input_tokens: 10, total_output_tokens: 5 });

    expect(model.state).toBe("completed");
    expect(isTurnTerminal(model.state)).toBe(true);
    expect(model.reasoning.every((segment) => !segment.streaming)).toBe(true);
    expect(model.completedAt).toBe(NOW);
  });

  it("cancelled 与 error 都是终态并清除运行状态", () => {
    const started = startTurn(createTurnExecutionModel("session-1"));
    const cancelled = reduce(started, { type: "cancelled" });
    expect(cancelled.state).toBe("cancelled");
    expect(isTurnTerminal(cancelled.state)).toBe(true);

    const errored = reduce(started, { type: "error", message: "运行失败" });
    expect(errored.state).toBe("failed");
    expect(isTurnTerminal(errored.state)).toBe(true);
  });

  it("新的一轮重置模型且不泄漏上一轮的步骤", () => {
    let model = reduce(startTurn(createTurnExecutionModel("session-1")), toolStart("tool-1"));
    model = reduce(model, toolEnd("tool-1"));
    model = reduce(model, { type: "done", total_input_tokens: 1, total_output_tokens: 1 });

    const nextTurn = startTurn(model, 1);

    expect(nextTurn.state).toBe("running");
    expect(nextTurn.steps).toEqual([]);
    expect(nextTurn.reasoning).toEqual([]);
    expect(nextTurn.finalText).toBe("");
    expect(nextTurn.iteration).toBe(1);
  });

  it("公开思考摘要与最终回复分开累积", () => {
    let model = startTurn(createTurnExecutionModel("session-1"));
    model = reduce(model, { type: "reasoning_summary_delta", id: "r1", summary_index: 0, delta: "正在比对字段" });
    model = reduce(model, { type: "text_delta", delta: "已完成对比" });

    expect(model.publicSummary).toContain("正在比对字段");
    expect(model.finalText).toBe("已完成对比");
    expect(model.finalText).not.toContain("正在比对字段");
  });

  it("新的文本段只保留最后一段作为最终回复", () => {
    let model = startTurn(createTurnExecutionModel("session-1"));
    model = reduce(model, { type: "text_delta", delta: "先看中间过程。" });
    model = reduce(model, { type: "text_segment_start", iteration: 1 });
    model = reduce(model, { type: "text_delta", delta: "最终回复。" });

    expect(model.finalText).toBe("最终回复。");
  });

  it("工具输出被截断到有界长度", () => {
    let model = reduce(startTurn(createTurnExecutionModel("session-1")), toolStart("tool-1"));
    model = reduce(model, { type: "tool_output_delta", id: "tool-1", delta: "x".repeat(MAX_TOOL_RESULT_CHARS + 500) });

    expect(model.steps[0].result!.length).toBeLessThanOrEqual(MAX_TOOL_RESULT_CHARS);
  });

  it("无关事件不改变模型", () => {
    const model = startTurn(createTurnExecutionModel("session-1"));
    const unchanged = reduce(model, {
      type: "context_usage",
      estimated_input_tokens: 1,
      total_input_budget: 2,
      trigger_threshold: 1,
      cumulative_input_tokens: 1,
      cumulative_output_tokens: 1,
      rolling_summary_version: 0,
      auto_compact_threshold: 0,
    });

    expect(unchanged).toBe(model);
  });

  it("可以手动展开和收起步骤", () => {
    const done = reduce(reduce(startTurn(createTurnExecutionModel("session-1")), toolStart("tool-1")), toolEnd("tool-1"));
    expect(done.steps[0].expanded).toBe(false);

    const expanded = toggleExecutionStep(done, "tool-1");
    expect(expanded.steps[0].expanded).toBe(true);

    const collapsed = toggleExecutionStep(expanded, "tool-1");
    expect(collapsed.steps[0].expanded).toBe(false);
  });
});

describe("restoreExecutionHistory", () => {
  it("按 sequence 顺序重放持久化事件", () => {
    const model = restoreExecutionHistory("session-1", [
      record(2, "tool_end", { type: "tool_end", id: "tool-1", name: "shell", result: "PASS", is_error: false }),
      record(1, "tool_start", { type: "tool_start", id: "tool-1", name: "shell", input: { command: "npm test" } }),
    ]);

    expect(model.turnId).toBe("turn-1");
    expect(model.steps).toHaveLength(1);
    expect(model.steps[0].state).toBe("completed");
    expect(model.steps[0].expanded).toBe(false);
  });

  it("重放历史后不残留运行中或流式状态", () => {
    const model = restoreExecutionHistory("session-1", [
      record(1, "turn_started", { type: "turn_started" }),
      record(2, "reasoning_summary", {
        type: "reasoning_summary",
        id: "r1",
        summary_index: 0,
        text: "正在核对素材",
      }),
      record(3, "done", { type: "done", total_input_tokens: 3, total_output_tokens: 2 }),
    ]);

    expect(model.state).toBe("completed");
    expect(model.reasoning[0].streaming).toBe(false);
    expect(model.reasoning[0].text).toBe("正在核对素材");
  });

  it("忽略无法识别的载荷", () => {
    const model = restoreExecutionHistory("session-1", [
      record(1, "turn_started", { type: "turn_started" }),
      record(2, "unknown_event", { nonsense: true }),
    ]);

    expect(model.steps).toEqual([]);
    expect(model.turnId).toBe("turn-1");
  });

  it("空历史返回 idle 模型", () => {
    const model = restoreExecutionHistory("session-1", []);
    expect(model.state).toBe("idle");
    expect(model.turnId).toBeUndefined();
  });
});

describe("轮次收尾", () => {
  it("轮次终止时结束所有未收到 tool_end 的步骤", () => {
    const running = reduce(startTurn(createTurnExecutionModel("session-1")), toolStart("tool-1"));

    const done = reduce(running, { type: "done", total_input_tokens: 1, total_output_tokens: 1 });
    expect(done.steps[0].state).toBe("completed");
    expect(done.steps[0].expanded).toBe(false);

    const cancelled = reduce(running, { type: "cancelled" });
    expect(cancelled.steps[0].state).toBe("cancelled");

    const errored = reduce(running, { type: "error", message: "boom" });
    expect(errored.steps[0].state).toBe("failed");
    expect(errored.steps[0].expanded).toBe(true);
  });

  it("已经结束的步骤不会被收尾规则改写", () => {
    const failed = reduce(
      reduce(startTurn(createTurnExecutionModel("session-1")), toolStart("tool-1")),
      toolEnd("tool-1", "boom", true),
    );
    const afterDone = reduce(failed, { type: "done", total_input_tokens: 1, total_output_tokens: 1 });

    expect(afterDone.steps[0].state).toBe("failed");
    expect(afterDone.steps[0].result).toBe("boom");
  });
});
