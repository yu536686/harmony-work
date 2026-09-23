/**
 * TurnExecutionModel 聚合器。
 *
 * 纯函数：给定同一个模型、同一个事件与同一个时间戳，输出完全一致。
 * 时间戳通过第三个参数注入，默认取系统时间，便于测试与历史重放。
 *
 * 设计约定：
 * - 聚合器不新建第二套 Agent Loop，只投影既有 `AgentEventType`（规格 §8.2）。
 * - `steps` 只承载工具执行与审批等待，计划项在 `plan` 中单独维护。
 * - 无关事件（context_usage、message_commit、interactive_ui 等）原样返回入参模型，
 *   让上层可以直接用引用比较跳过重渲染。
 */
import type { AgentEventType, TurnExecutionEventRecord } from "../../../services/tauri/chat";
import { summarizeToolInput, summarizeToolResult, truncatePreview } from "./presentation";
import type {
  ExecutionPlanItem,
  ExecutionState,
  ExecutionStep,
  ReasoningSegment,
  StepState,
  TurnExecutionModel,
} from "./types";

/** 单个工具结果的存储上限 */
export const MAX_TOOL_RESULT_CHARS = 4000;
/** 公开思考摘要的存储上限 */
export const MAX_REASONING_CHARS = 12000;
/** 最终回复的存储上限 */
export const MAX_FINAL_TEXT_CHARS = 20000;

const TERMINAL_STATES: readonly ExecutionState[] = ["completed", "failed", "cancelled"];

/** 轮次是否已经结束（结束状态不得再显示任何运行动画，规格 §5.5）。 */
export function isTurnTerminal(state: ExecutionState): boolean {
  return TERMINAL_STATES.includes(state);
}

export function createTurnExecutionModel(sessionId: string): TurnExecutionModel {
  return {
    sessionId,
    state: "idle",
    iteration: 0,
    reasoning: [],
    steps: [],
    plan: [],
    finalText: "",
    artifacts: [],
  };
}

/** 按字符边界安全截断（不切碎代理对）。 */
function safeSlice(text: string, limit: number): string {
  if (limit <= 0) return "";
  if (text.length <= limit) return text;
  let end = limit;
  const code = text.charCodeAt(end - 1);
  if (code >= 0xd800 && code <= 0xdbff) end -= 1;
  return text.slice(0, end);
}

function boundedAppend(current: string, delta: string, max: number): string {
  if (!delta) return current;
  if (current.length >= max) return current;
  return safeSlice(current + delta, max);
}

function buildPublicSummary(segments: ReasoningSegment[]): string | undefined {
  const text = segments
    .map((segment) => segment.text)
    .filter((value) => value.trim().length > 0)
    .join("\n")
    .trim();
  return text.length > 0 ? text : undefined;
}

function upsertReasoning(
  model: TurnExecutionModel,
  id: string,
  summaryIndex: number,
  delta: string,
  streaming: boolean,
): TurnExecutionModel {
  const index = model.reasoning.findIndex(
    (segment) => segment.id === id && segment.summaryIndex === summaryIndex,
  );
  const segments = [...model.reasoning];
  if (index === -1) {
    segments.push({
      id,
      summaryIndex,
      text: safeSlice(delta, MAX_REASONING_CHARS),
      streaming,
    });
  } else {
    const existing = segments[index];
    const text = boundedAppend(existing.text, delta, MAX_REASONING_CHARS);
    if (text === existing.text && existing.streaming === streaming) return model;
    segments[index] = { ...existing, text, streaming };
  }
  return { ...model, reasoning: segments, publicSummary: buildPublicSummary(segments) };
}

const TERMINAL_STEP_STATES: readonly StepState[] = ["completed", "failed", "cancelled"];

function isStepTerminal(state: StepState): boolean {
  return TERMINAL_STEP_STATES.includes(state);
}

/**
 * 进入结束状态：停止所有运行动画，并记录完成时间（规格 §5.5）。
 *
 * 收尾规则：轮次终止时，仍未收到 `tool_end` 的步骤必须一并落地，
 * 否则时间线会留下永久「运行中」的假状态（规格 §11.2 明确禁止）。
 * 依据终止原因收敛：正常完成 → completed，取消 → cancelled，出错 → failed。
 */
function finishTurn(model: TurnExecutionModel, state: ExecutionState, now: string): TurnExecutionModel {
  const fallback: StepState =
    state === "completed" ? "completed" : state === "cancelled" ? "cancelled" : "failed";
  let stepsChanged = false;
  const steps = model.steps.map((step) => {
    if (isStepTerminal(step.state)) return step;
    stepsChanged = true;
    return {
      ...step,
      state: fallback,
      expanded: fallback === "failed",
      completedAt: step.completedAt ?? now,
    };
  });
  const reasoning = model.reasoning.some((segment) => segment.streaming)
    ? model.reasoning.map((segment) => (segment.streaming ? { ...segment, streaming: false } : segment))
    : model.reasoning;
  return {
    ...model,
    state,
    reasoning,
    steps: stepsChanged ? steps : model.steps,
    completedAt: now,
  };
}

function samePlan(left: ExecutionPlanItem[], right: ExecutionPlanItem[]): boolean {
  if (left.length !== right.length) return false;
  return left.every((item, index) => {
    const other = right[index];
    return (
      item.id === other.id && item.content === other.content && item.status === other.status
    );
  });
}

/**
 * 把一个 `AgentEventType` 折叠进执行模型。
 *
 * 对模型没有影响的事件会原样返回 `model`，调用方可据此跳过状态写入。
 */
export function reduceExecutionEvent(
  model: TurnExecutionModel,
  event: AgentEventType,
  now: string = new Date().toISOString(),
): TurnExecutionModel {
  switch (event.type) {
    case "run_status": {
      // 终态或 idle 之后的第一次 run_status 视为新一轮开始，重置模型但保留会话标识
      if (model.state === "idle" || isTurnTerminal(model.state)) {
        return {
          ...createTurnExecutionModel(model.sessionId),
          state: "running",
          iteration: event.iteration,
          startedAt: now,
        };
      }
      if (model.state === "running" && model.iteration === event.iteration) return model;
      return { ...model, state: "running", iteration: event.iteration };
    }

    case "text_segment_start": {
      // 只有最后一个文本段是最终回复，中间叙述不进入 finalText（规格 §5.5）
      return model.finalText === "" ? model : { ...model, finalText: "" };
    }

    case "text_delta": {
      const finalText = boundedAppend(model.finalText, event.delta, MAX_FINAL_TEXT_CHARS);
      return finalText === model.finalText ? model : { ...model, finalText };
    }

    case "reasoning_summary_start":
      return upsertReasoning(model, event.id, event.summary_index, "", true);

    case "reasoning_summary_delta":
      return upsertReasoning(model, event.id, event.summary_index, event.delta, true);

    case "tool_start": {
      const summary = summarizeToolInput(event.name, event.input);
      const state: ExecutionState = model.state === "awaiting_approval" ? "running" : model.state;
      const index = model.steps.findIndex((step) => step.id === event.id);
      if (index === -1) {
        const step: ExecutionStep = {
          id: event.id,
          kind: "tool",
          name: event.name,
          state: "running",
          summary,
          input: event.input,
          expanded: true,
          startedAt: now,
        };
        return { ...model, state, steps: [...model.steps, step] };
      }
      const existing = model.steps[index];
      if (isStepTerminal(existing.state)) {
        const steps = [...model.steps];
        steps[index] = {
          ...existing,
          name: event.name,
          input: event.input,
          startedAt: existing.startedAt ?? now,
        };
        return { ...model, steps };
      }
      if (existing.state === "running" && existing.name === event.name) {
        return state === model.state ? model : { ...model, state };
      }
      const steps = [...model.steps];
      steps[index] = {
        ...existing,
        name: event.name,
        input: event.input,
        state: "running",
        summary,
        isError: undefined,
        expanded: true,
        startedAt: existing.startedAt ?? now,
        completedAt: undefined,
      };
      return { ...model, state, steps };
    }

    case "tool_output_delta": {
      const index = model.steps.findIndex((step) => step.id === event.id);
      if (index === -1) return model;
      const existing = model.steps[index];
      const result = boundedAppend(existing.result ?? "", event.delta, MAX_TOOL_RESULT_CHARS);
      if (result === (existing.result ?? "")) return model;
      const steps = [...model.steps];
      steps[index] = { ...existing, result };
      return { ...model, steps };
    }

    case "tool_end": {
      const state: StepState = event.is_error ? "failed" : "completed";
      const result = truncatePreview(event.result ?? "", MAX_TOOL_RESULT_CHARS);
      const summary = summarizeToolResult(event.result ?? "");
      const index = model.steps.findIndex((step) => step.id === event.id);
      if (index === -1) {
        // tool_end 先于 tool_start 到达时也要给出可读的已完成步骤
        const step: ExecutionStep = {
          id: event.id,
          kind: "tool",
          name: event.name,
          state,
          summary,
          result,
          isError: event.is_error,
          expanded: event.is_error,
          completedAt: now,
        };
        return { ...model, steps: [...model.steps, step] };
      }
      const existing = model.steps[index];
      if (
        existing.state === state &&
        existing.result === result &&
        existing.isError === event.is_error
      ) {
        return model;
      }
      const steps = [...model.steps];
      steps[index] = {
        ...existing,
        name: event.name,
        state,
        summary,
        result,
        isError: event.is_error,
        // 失败的步骤保持展开以展示原因，成功的步骤自动折叠（规格 §5.2）
        expanded: event.is_error,
        completedAt: now,
      };
      return { ...model, steps };
    }

    case "permission_request": {
      const summary =
        event.description || summarizeToolInput(event.tool_name, event.tool_input);
      const step: ExecutionStep = {
        id: event.request_id,
        kind: "tool",
        name: event.tool_name,
        state: "awaiting_approval",
        summary,
        input: event.tool_input,
        expanded: true,
        startedAt: now,
      };
      const index = model.steps.findIndex((item) => item.id === event.request_id);
      const steps =
        index === -1
          ? [...model.steps, step]
          : model.steps.map((item, position) => (position === index ? step : item));
      return { ...model, state: "awaiting_approval", steps };
    }

    case "permission_resolved": {
      const index = model.steps.findIndex((step) => step.id === event.request_id);
      if (index === -1 || model.steps[index].state !== "awaiting_approval") return model;
      const steps = model.steps.map((step, position) =>
        position === index ? { ...step, state: "completed" as const, expanded: false, completedAt: now } : step,
      );
      return { ...model, state: model.state === "awaiting_approval" ? "running" : model.state, steps };
    }

    case "plan_update": {
      const plan: ExecutionPlanItem[] = event.items.map((item) => ({
        id: item.id,
        content: item.content,
        status: item.status,
      }));
      return samePlan(model.plan, plan) ? model : { ...model, plan };
    }

    case "done":
      return finishTurn(model, "completed", now);

    case "cancelled":
      return finishTurn(model, "cancelled", now);

    case "error":
      return finishTurn(model, "failed", now);

    default:
      return model;
  }
}

/** 手动展开/收起一个步骤。 */
export function toggleExecutionStep(
  model: TurnExecutionModel,
  stepId: string,
): TurnExecutionModel {
  const index = model.steps.findIndex((step) => step.id === stepId);
  if (index === -1) return model;
  const steps = [...model.steps];
  steps[index] = { ...steps[index], expanded: !steps[index].expanded };
  return { ...model, steps };
}

/**
 * 从持久化执行事件重建静态时间线（规格 §5.5）。
 *
 * 事件按 `sequence` 升序重放；每个 `turn_started` 开始新的一轮，因此
 * 不指定 `turnId` 时返回该会话最后一次执行的模型。历史轮次不会保留
 * 运行中状态，避免打开旧会话时出现「仍在执行」的假象。
 */
export function restoreExecutionHistory(
  sessionId: string,
  records: TurnExecutionEventRecord[],
  turnId?: string,
): TurnExecutionModel {
  const scoped = turnId ? records.filter((record) => record.turn_id === turnId) : records;
  const ordered = [...scoped].sort((left, right) => left.sequence - right.sequence);
  let model = createTurnExecutionModel(sessionId);

  for (const item of ordered) {
    if (item.event_type === "turn_started") {
      model = {
        ...createTurnExecutionModel(sessionId),
        turnId: item.turn_id,
        state: "running",
        startedAt: item.created_at,
      };
      continue;
    }
    if (!model.turnId) model = { ...model, turnId: item.turn_id };

    const payload = item.payload as { type?: unknown } | null | undefined;
    const type = payload && typeof payload === "object" ? payload.type : undefined;
    if (typeof type !== "string") continue;

    if (type === "reasoning_summary") {
      const summary = payload as { id?: unknown; summary_index?: unknown; text?: unknown };
      if (typeof summary.id === "string" && typeof summary.text === "string") {
        const summaryIndex =
          typeof summary.summary_index === "number" ? summary.summary_index : 0;
        model = upsertReasoning(model, summary.id, summaryIndex, summary.text, false);
      }
      continue;
    }

    model = reduceExecutionEvent(model, payload as AgentEventType, item.created_at);
  }

  if (ordered.length > 0 && !isTurnTerminal(model.state)) {
    // 会话已经不再由这个轮次驱动：结束态必须落地，否则时间线会一直显示运行中
    model = finishTurn(
      model,
      "cancelled",
      ordered[ordered.length - 1]?.created_at ?? new Date(0).toISOString(),
    );
  }
  return model;
}
