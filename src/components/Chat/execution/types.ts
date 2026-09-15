/**
 * 对话执行时间线的结构类型。
 *
 * 这些类型描述「一个用户消息对应的一轮执行」，由 `reducer.ts` 中的纯函数
 * 从 `AgentEventType` 事件流聚合得到，供执行时间线 UI 只读消费。
 *
 * 与规格 §8.2 的对应关系：
 * - 规格的 `status` 在本模块命名为 `state`（值域为其超集，额外包含 idle）。
 * - 规格的 `publicSummary` / `artifacts` 保留为独立字段。
 */
import type { SessionArtifact } from "../../../services/tauri/chat";

/** 整轮执行状态。对应规格 §5.2 的轮次状态集合。 */
export type ExecutionState =
  | "idle"
  | "running"
  | "awaiting_approval"
  | "completed"
  | "failed"
  | "cancelled";

/** 单个执行步骤的状态。对应规格 §5.2 的步骤状态表。 */
export type StepState =
  | "pending"
  | "running"
  | "awaiting_approval"
  | "completed"
  | "failed"
  | "cancelled";

/**
 * 计划项。与本仓库 `store/slices/chat.ts` 的 `PlanTodoItem` 结构一致，
 * 此处独立声明以避免聚合层反向依赖 Redux 切片。
 */
export interface ExecutionPlanItem {
  id: string;
  content: string;
  status: "pending" | "in_progress" | "completed" | "cancelled";
}

/** 公开思考摘要的一个片段（来自 reasoning summary 事件，非模型隐藏推理）。 */
export interface ReasoningSegment {
  id: string;
  summaryIndex: number;
  text: string;
  /** true 表示该摘要仍在流式追加，UI 可显示动态指示器 */
  streaming: boolean;
}

/** 时间线上的一个执行步骤。首版只承载工具执行与审批等待。 */
export interface ExecutionStep {
  id: string;
  kind: "tool";
  /** 原始工具名，展示名由 presentation.ts 投影 */
  name: string;
  state: StepState;
  /** 一行可扫描的结果摘要 */
  summary: string;
  input?: unknown;
  result?: string;
  isError?: boolean;
  /** 已完成步骤默认折叠，当前与失败步骤默认展开 */
  expanded: boolean;
  startedAt?: string;
  completedAt?: string;
}

/** 一轮对话的完整执行模型。 */
export interface TurnExecutionModel {
  sessionId: string;
  /** 持久化轮次标识；实时事件流中由后端在历史查询时提供 */
  turnId?: string;
  state: ExecutionState;
  iteration: number;
  reasoning: ReasoningSegment[];
  /** 只包含工具执行与审批等待；计划项在 `plan` 中单独维护 */
  steps: ExecutionStep[];
  plan: ExecutionPlanItem[];
  /** 最终回复文本（最后一个文本段），不包含公开思考摘要 */
  finalText: string;
  startedAt?: string;
  completedAt?: string;
  /** 公开思考摘要汇总，用于「思考」折叠区（规格 §8.2） */
  publicSummary?: string;
  /** 本轮成果，由右侧工作面板填充（规格 §6.1） */
  artifacts: SessionArtifact[];
}
