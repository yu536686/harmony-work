import { schedulerApi, sessionsApi, type Session, type ScheduledTask } from "../services/tauri/chat";
import { loadHarvisTasks, type HarvisTask, type HarvisTaskKind } from "../services/tauri/creative";
import { classifyMainChatSession, isInternalSession } from "../utils/session";

export type TaskStatus = "queued" | "running" | "succeeded" | "partial" | "failed" | "cancelled" | "paused" | "scheduled" | "idle" | "unknown";
export type TaskSource = "session" | "im" | "harvis" | "scheduler";
export type TaskCenterTarget =
  | { type: "session"; sessionId: string; source: string; session?: Session; view: "conversation" | "artifacts" | "error"; artifactId?: string }
  | { type: "harvis"; taskId: string; kind: HarvisTaskKind; view: "task" | "result" | "error" }
  | { type: "scheduler"; taskId: string; view: "task" | "error" };
export type SessionActivity = Record<string, { running?: boolean; outcome?: "unread" | "error" }>;
export interface TaskItem {
  id: string;
  title: string;
  source: TaskSource;
  sourceLabel: string;
  status: TaskStatus;
  rawStatus: string;
  updatedAt: number;
  detail: string;
  progress?: number;
  error?: string;
  queryError?: string;
  target: TaskCenterTarget;
  results: Array<{ label: string; target: TaskCenterTarget }>;
  adopted?: boolean;
  rating?: number;
}
export interface TaskSnapshot {
  sessions: Session[];
  harvis: HarvisTask[];
  scheduled: ScheduledTask[];
  errors: Array<{ source: "sessions" | "scheduler"; message: string }>;
}
export interface TaskFilter { status?: TaskStatus | "all"; source?: TaskSource | "all"; search?: string }
export const emptyTaskSnapshot = (): TaskSnapshot => ({ sessions: [], harvis: [], scheduled: [], errors: [] });

export function taskErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function isNonMobileSession(session: Session): boolean {
  // list_sessions only gates mobile on device auth; it does not scope records to the current account.
  return !session.source?.trim().toLowerCase().startsWith("mobile");
}

async function listAllSessions(): Promise<Session[]> {
  const sessions = new Map<string, Session>();
  let offset = 0;
  while (true) {
    const page = await sessionsApi.list(100, offset);
    for (const session of page.sessions) {
      if (isNonMobileSession(session)) sessions.set(session.id, session);
    }
    offset += page.sessions.length;
    if (offset >= page.total) return [...sessions.values()];
    if (!page.sessions.length) throw new Error("会话列表分页未完成，请刷新重试");
  }
}

export async function loadTaskSources(previous: TaskSnapshot = emptyTaskSnapshot()): Promise<TaskSnapshot> {
  const [sessions, scheduled] = await Promise.allSettled([
    listAllSessions(), schedulerApi.list(),
  ]);
  const errors: TaskSnapshot["errors"] = [];
  if (sessions.status === "rejected") errors.push({ source: "sessions", message: taskErrorMessage(sessions.reason) });
  if (scheduled.status === "rejected") errors.push({ source: "scheduler", message: taskErrorMessage(scheduled.reason) });
  return {
    sessions: (sessions.status === "fulfilled" ? sessions.value : previous.sessions).filter(isNonMobileSession),
    scheduled: scheduled.status === "fulfilled" ? scheduled.value.tasks : previous.scheduled,
    harvis: loadHarvisTasks(),
    errors,
  };
}

function statusOf(raw: string): TaskStatus {
  switch (raw.toLowerCase()) {
    case "pending": case "queued": case "submitting": case "submitted": return "queued";
    case "processing": case "running": case "archiving": return "running";
    case "success": case "succeeded": case "completed": case "archived": case "no_change": return "succeeded";
    case "partial_success": return "partial";
    case "error": case "failed": case "expired": return "failed";
    case "cancelled": case "canceled": return "cancelled";
    case "paused": return "paused";
    case "idle": return "idle";
    default: return "unknown";
  }
}

function timestamp(value?: string | number | null): number {
  const result = typeof value === "number" ? value : Date.parse(value || "");
  return Number.isFinite(result) ? result : 0;
}

export function aggregateTasks(snapshot: TaskSnapshot, activity: SessionActivity = {}): TaskItem[] {
  const sessions: TaskItem[] = snapshot.sessions.filter((session) => isNonMobileSession(session) && !isInternalSession(session)).map((session) => {
    const kind = session.source === "scheduled_task" ? "chat" : classifyMainChatSession(session);
    const signal = activity[session.id];
    const status = signal?.running ? "running" : signal?.outcome === "error" ? "failed" : signal?.outcome === "unread" ? "succeeded" : signal?.running === false && session.status === "running" ? "idle" : statusOf(session.status);
    return {
      id: `session:${session.id}`, title: session.title?.trim() || "未命名会话", source: kind === "im" ? "im" : "session",
      sourceLabel: session.source === "scheduled_task" ? "定时任务会话" : kind === "im" ? `IM · ${session.source}` : kind === "cli" ? "CLI 会话" : "会话",
      status, rawStatus: session.status, updatedAt: timestamp(session.updated_at || session.created_at),
      detail: session.rolling_summary || `${session.message_count} 条消息`,
      error: status === "failed" ? "会话执行出错，请查看会话记录" : undefined,
      target: { type: "session", sessionId: session.id, source: session.source, session, view: "conversation" }, results: [],
    };
  });
  const harvis: TaskItem[] = snapshot.harvis.map((task) => ({
    id: `harvis:${task.kind}:${task.taskId}`, title: task.prompt || "未命名创作", source: "harvis", sourceLabel: `Harvis · ${task.kind}`,
    status: statusOf(task.status), rawStatus: task.status, updatedAt: timestamp(task.createdAt),
    detail: "", progress: task.progress, error: task.status === "failed" ? task.errorMessage || "生成任务失败" : undefined,
    queryError: task.queryError, adopted: task.adopted, rating: task.rating,
    target: { type: "harvis", taskId: task.taskId, kind: task.kind, view: "task" },
    results: task.resultUrl ? [{ label: "查看成果", target: { type: "harvis", taskId: task.taskId, kind: task.kind, view: "result" } }] : [],
  }));
  const scheduled: TaskItem[] = snapshot.scheduled.map((task) => {
    const execution = snapshot.sessions.find((session) => session.id === `sched_${task.id}` && session.source === "scheduled_task");
    return {
      id: `scheduler:${task.id}`, title: task.name, source: "scheduler", sourceLabel: "定时任务",
      status: task.last_run_status === "running" ? "running" : task.status === "paused" ? "paused" : task.last_run_status ? statusOf(task.last_run_status) : task.status === "active" ? "scheduled" : statusOf(task.status),
      rawStatus: [task.status, task.last_run_status].filter(Boolean).join(" / "), updatedAt: timestamp(task.last_run_at || task.created_at),
      detail: [task.description || task.task_prompt, task.cron_expression, task.next_run_at ? `下次 ${task.next_run_at}` : ""].filter(Boolean).join(" · "),
      error: task.last_run_status === "failed" ? "最近一次执行失败，请查看定时任务" : undefined,
      target: { type: "scheduler", taskId: task.id, view: "task" },
      results: execution ? [{ label: "执行记录", target: { type: "session", sessionId: execution.id, source: execution.source, session: execution, view: "conversation" } }] : [],
    };
  });
  return [...sessions, ...harvis, ...scheduled].sort((a, b) => b.updatedAt - a.updatedAt || a.id.localeCompare(b.id));
}

export function filterTasks(tasks: TaskItem[], filter: TaskFilter): TaskItem[] {
  const query = filter.search?.trim().toLocaleLowerCase() ?? "";
  return tasks.filter((task) => (!filter.status || filter.status === "all" || task.status === filter.status)
    && (!filter.source || filter.source === "all" || task.source === filter.source)
    && (!query || [task.title, task.sourceLabel, task.detail, task.error, task.queryError, task.rawStatus, ...task.results.map((result) => result.label)].filter(Boolean).join(" ").toLocaleLowerCase().includes(query)));
}
