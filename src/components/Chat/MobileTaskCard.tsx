import { LoaderCircle, Monitor } from "lucide-react";
import type { MobileTaskEvent } from "./mobileTaskEvents";

export type MobileTaskView = {
  taskId: string;
  initiatingMessageId: string | null;
  latest: MobileTaskEvent;
  running: boolean;
  steps: MobileTaskEvent[];
};

export default function MobileTaskCard({ card, result }: { card: MobileTaskView; result?: string | null }) {
  const failed = card.latest.kind === "failed" || card.latest.kind === "cancelled";
  const latestStep = [...card.steps].reverse().find(event =>
    event.kind === "tool_started" || event.kind === "planning" || event.kind === "artifact_ready");
  return <section className="mobile-task-card" role="status" aria-live="polite">
    <div className="mobile-task-card-heading">
      <Monitor size={16} aria-hidden="true" />
      <strong>{card.running ? "Windows 正在执行手机任务" : failed ? "电脑任务未完成" : "电脑任务已完成"}</strong>
      {card.running && <LoaderCircle className="mobile-task-card-spinner" size={16} aria-label="执行中" />}
    </div>
    <p>{card.latest.kind === "succeeded" ? "电脑成果已就绪" : card.latest.summary || latestStep?.summary}</p>
    {!card.running && !failed && result && <details>
      <summary>查看电脑成果</summary>
      <div className="mobile-task-card-result">{result}</div>
    </details>}
    {!card.running && !failed && <small>手机恢复后，小亨会继续在本会话整理最终答复。</small>}
  </section>;
}
