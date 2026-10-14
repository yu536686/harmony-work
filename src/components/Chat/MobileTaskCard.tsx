import { FileText, LoaderCircle, Monitor } from "lucide-react";
import { useState } from "react";
import type { MobileTaskEvent } from "./mobileTaskEvents";

export type MobileTaskView = {
  taskId: string;
  initiatingMessageId: string | null;
  latest: MobileTaskEvent;
  running: boolean;
  steps: MobileTaskEvent[];
};

export type MobileSharedAttachment = {
  id: string;
  taskId: string;
  name: string;
  mimeType: string;
  size: number;
};

function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.ceil(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export default function MobileTaskCard({ card, result, attachments = [], onOpenAttachment }: {
  card: MobileTaskView;
  result?: string | null;
  attachments?: MobileSharedAttachment[];
  onOpenAttachment?: (attachmentId: string) => void | Promise<void>;
}) {
  const [openError, setOpenError] = useState("");
  const failed = card.latest.kind === "failed" || card.latest.kind === "cancelled";
  const latestStep = [...card.steps].reverse().find(event =>
    event.kind === "tool_started" || event.kind === "planning" || event.kind === "artifact_ready");
  const files = card.steps.filter(event => event.kind === "artifact_ready" &&
    typeof event.payload.attachmentId === "string")
    .map(event => ({
      id: String(event.payload.attachmentId),
      fallbackName: String(event.payload.artifactName ?? "电脑附件"),
      metadata: attachments.find(item => item.id === event.payload.attachmentId && item.taskId === card.taskId),
    }));
  return <section className="mobile-task-card" role="status" aria-live="polite">
    <div className="mobile-task-card-heading">
      <Monitor size={16} aria-hidden="true" />
      <strong>{card.running ? "Windows 正在执行手机任务" : failed ? "电脑任务未完成" : "电脑任务已完成"}</strong>
      {card.running && <LoaderCircle className="mobile-task-card-spinner" size={16} aria-label="执行中" />}
    </div>
    <p>{card.latest.kind === "succeeded" ? "电脑成果已就绪" : card.latest.summary || latestStep?.summary}</p>
    {files.map(file => <div className="mobile-task-attachment" key={file.id}>
      <FileText size={20} aria-hidden="true" />
      <div>
        <strong>{file.metadata?.name ?? file.fallbackName}</strong>
        <small>{file.metadata ? `${fileSize(file.metadata.size)} · ${file.metadata.mimeType}` : "正在读取附件信息"}</small>
      </div>
      {file.metadata && onOpenAttachment && <button type="button" onClick={() => {
        setOpenError("");
        void Promise.resolve(onOpenAttachment(file.id)).catch(() => setOpenError("文件已移动或无法打开"));
      }}>在电脑打开</button>}
    </div>)}
    {openError && <small role="alert">{openError}</small>}
    {!card.running && !failed && result && <details>
      <summary>查看电脑成果</summary>
      <div className="mobile-task-card-result">{result}</div>
    </details>}
    {!card.running && !failed && <small>手机恢复后，小亨会继续在本会话整理最终答复。</small>}
  </section>;
}
