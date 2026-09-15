import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  BrainCircuit,
  ChevronDown,
  ChevronRight,
  CircleCheck,
  CircleDashed,
  CircleStop,
  CircleX,
  LoaderCircle,
  ShieldAlert,
} from "lucide-react";
import { isTurnTerminal } from "./execution/reducer";
import { describeStep, toolDisplayName } from "./execution/presentation";
import type {
  ExecutionPlanItem,
  ExecutionStep,
  StepState,
  TurnExecutionModel,
} from "./execution/types";
import "./ConversationExperience.css";

interface ExecutionTimelineProps {
  model: TurnExecutionModel;
  onToggleStep: (stepId: string) => void;
}

function stateLabel(state: StepState): string {
  switch (state) {
    case "running":
      return "执行中";
    case "awaiting_approval":
      return "等待批准";
    case "completed":
      return "已完成";
    case "failed":
      return "执行失败";
    case "cancelled":
      return "已取消";
    case "pending":
    default:
      return "待处理";
  }
}

function StatusIcon({ state }: { state: StepState }) {
  const label = stateLabel(state);
  const props = {
    size: 17,
    strokeWidth: 1.8,
    "aria-label": label,
    role: "img",
  } as const;

  switch (state) {
    case "running":
      return <LoaderCircle {...props} className="execution-status-icon is-running" />;
    case "awaiting_approval":
      return <ShieldAlert {...props} className="execution-status-icon is-waiting" />;
    case "completed":
      return <CircleCheck {...props} className="execution-status-icon is-completed" />;
    case "failed":
      return <CircleX {...props} className="execution-status-icon is-failed" />;
    case "cancelled":
      return <CircleStop {...props} className="execution-status-icon is-cancelled" />;
    case "pending":
    default:
      return <CircleDashed {...props} className="execution-status-icon is-pending" />;
  }
}

function planState(status: ExecutionPlanItem["status"]): StepState {
  switch (status) {
    case "in_progress":
      return "running";
    case "completed":
      return "completed";
    case "cancelled":
      return "cancelled";
    case "pending":
    default:
      return "pending";
  }
}

function formatInput(input: unknown): string {
  if (typeof input === "string") return input;
  if (!input || typeof input !== "object") return input == null ? "" : String(input);
  const record = input as Record<string, unknown>;
  for (const key of ["command", "path", "query", "url"]) {
    if (typeof record[key] === "string") return record[key] as string;
  }
  try {
    return JSON.stringify(input, null, 2);
  } catch {
    return String(input);
  }
}

function ToolStepRow({
  step,
  onToggle,
}: {
  step: ExecutionStep;
  onToggle: () => void;
}) {
  const displayName = toolDisplayName(step.name);
  const detail = describeStep(step);
  const input = formatInput(step.input);

  return (
    <li className={`execution-step is-${step.state}`}>
      <button
        type="button"
        className="execution-step-toggle"
        onClick={onToggle}
        aria-expanded={step.expanded}
        aria-label={`${displayName}：${detail}`}
      >
        <StatusIcon state={step.state} />
        <span className="execution-step-copy">
          <strong>{displayName}</strong>
          {detail && <span>{detail}</span>}
        </span>
        {step.expanded ? (
          <ChevronDown size={16} aria-hidden="true" />
        ) : (
          <ChevronRight size={16} aria-hidden="true" />
        )}
      </button>
      {step.expanded && (
        <div className="execution-step-detail">
          {input && (
            <div>
              <span className="execution-detail-label">输入</span>
              <pre>{input}</pre>
            </div>
          )}
          {step.result && (
            <div>
              <span className="execution-detail-label">
                {step.isError ? "错误" : "输出"}
              </span>
              <pre>{step.result}</pre>
            </div>
          )}
        </div>
      )}
    </li>
  );
}

export default function ExecutionTimeline({ model, onToggleStep }: ExecutionTimelineProps) {
  const { t } = useTranslation();
  const terminal = isTurnTerminal(model.state);
  const [reasoningOpen, setReasoningOpen] = useState(!terminal);
  const reasoningText = model.reasoning
    .map((segment) => segment.text)
    .filter((text) => text.trim())
    .join("\n");
  const reasoningStreaming = model.reasoning.some((segment) => segment.streaming);
  const hasContent =
    model.state !== "idle" ||
    Boolean(reasoningText) ||
    model.plan.length > 0 ||
    model.steps.length > 0;

  useEffect(() => {
    setReasoningOpen(!isTurnTerminal(model.state));
  }, [model.turnId, model.state]);

  if (!hasContent) return null;

  return (
    <section className={`execution-timeline is-${model.state}`} aria-label={t("chat.executionTitle")}>
      {reasoningText && (
        <div className="execution-reasoning">
          <button
            type="button"
            className="execution-reasoning-toggle"
            onClick={() => setReasoningOpen((open) => !open)}
            aria-expanded={reasoningOpen}
            aria-label={t("chat.executionReasoning")}
          >
            <BrainCircuit
              size={17}
              strokeWidth={1.8}
              className={reasoningStreaming ? "is-running" : ""}
              role="img"
              aria-label={reasoningStreaming ? t("chat.executionThinking") : t("chat.executionReasoning")}
            />
            <span>{reasoningStreaming ? t("chat.executionThinking") : t("chat.executionReasoning")}</span>
            {reasoningOpen ? (
              <ChevronDown size={16} aria-hidden="true" />
            ) : (
              <ChevronRight size={16} aria-hidden="true" />
            )}
          </button>
          {reasoningOpen && <p className="execution-reasoning-text">{reasoningText}</p>}
        </div>
      )}

      {model.plan.length > 0 && (
        <ol className="execution-plan" aria-label={t("chat.executionPlan")}>
          {model.plan.map((item) => {
            const state = planState(item.status);
            return (
              <li key={item.id} className={`execution-plan-item is-${state}`}>
                <StatusIcon state={state} />
                <span>{item.content}</span>
              </li>
            );
          })}
        </ol>
      )}

      {model.steps.length > 0 && (
        <ol className="execution-steps" aria-label={t("chat.executionTools")}>
          {model.steps.map((step) => (
            <ToolStepRow
              key={step.id}
              step={step}
              onToggle={() => onToggleStep(step.id)}
            />
          ))}
        </ol>
      )}
    </section>
  );
}
