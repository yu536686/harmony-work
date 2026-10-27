import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ArrowUpRight, CalendarClock, CircleAlert, FileOutput, Image, LoaderCircle, MessageSquare, RefreshCw, Search } from "lucide-react";
import { artifactsApi, type SessionArtifact } from "../../services/tauri/chat";
import { useHarvisTasks } from "../Creative/useHarvisTasks";
import HarvisMediaPreview from "./HarvisMediaPreview";
import type { LocalMediaKind } from "./mediaSource";
import { aggregateTasks, emptyTaskSnapshot, filterTasks, loadTaskSources, taskErrorMessage, type SessionActivity, type TaskCenterTarget, type TaskItem, type TaskSource, type TaskStatus } from "../../tasks/taskCenter";
import "./TaskCenter.css";
import { isCurrentMarketingSession, type MarketingSessionSnapshot } from "../../services/tauri/marketing";

export type { TaskCenterTarget, SessionActivity } from "../../tasks/taskCenter";

export interface TaskCenterProps {
  onNavigate: (target: TaskCenterTarget) => void;
  sessionActivity?: SessionActivity;
  refreshIntervalMs?: number;
}

const statusLabels: Record<TaskStatus, string> = {
  queued: "排队中", running: "进行中", succeeded: "已完成", partial: "部分完成", failed: "失败",
  cancelled: "已取消", paused: "已暂停", scheduled: "待调度", idle: "空闲", unknown: "未知",
};
const sourceLabels: Record<TaskSource, string> = { session: "会话", im: "IM", harvis: "Harvis", scheduler: "定时任务" };
const loadSourceLabels = { sessions: "会话 / IM", scheduler: "定时任务" };

function SessionResults({ target, onNavigate }: { target: Extract<TaskCenterTarget, { type: "session" }>; onNavigate: TaskCenterProps["onNavigate"] }) {
  const [open, setOpen] = useState(false);
  const [artifacts, setArtifacts] = useState<SessionArtifact[] | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const load = async () => {
    if (loading) return;
    setLoading(true);
    setError("");
    try {
      const results = await artifactsApi.list(target.sessionId);
      if (mounted.current) setArtifacts(results);
    } catch (cause) {
      if (mounted.current) setError(taskErrorMessage(cause));
    } finally {
      if (mounted.current) setLoading(false);
    }
  };
  return <div className="task-center-session-results">
    <button type="button" className="task-center-command" aria-expanded={open} onClick={() => { setOpen(!open); if (!open) void load(); }}><FileOutput size={14} />会话成果</button>
    {open && <div className="task-center-artifacts">
      {loading && <span role="status">正在读取成果…</span>}
      {error && <div role="alert">成果读取失败：{error}<button type="button" className="task-center-icon" title="重试成果" aria-label="重试成果" onClick={() => void load()}><RefreshCw size={14} /></button></div>}
      {!loading && !error && artifacts?.length === 0 && <span>暂无已登记成果</span>}
      {artifacts?.map((artifact) => <button type="button" className="task-center-command" key={artifact.id} onClick={() => onNavigate({ ...target, view: "artifacts", artifactId: artifact.id })}><FileOutput size={14} />{artifact.name || "未命名成果"}</button>)}
    </div>}
  </div>;
}

function TaskRow({ task, onNavigate }: { task: TaskItem; onNavigate: TaskCenterProps["onNavigate"] }) {
  const Icon = task.source === "scheduler" ? CalendarClock : task.source === "session" || task.source === "im" ? MessageSquare : Image;
  const hasProgress = task.status === "running" || task.status === "queued";
  const progress = typeof task.progress === "number" && Number.isFinite(task.progress) ? Math.max(0, Math.min(100, task.progress)) : undefined;
  return <article className="task-center-row" aria-label={task.title}>
    <div className="task-center-row-icon" aria-hidden="true"><Icon size={17} /></div>
    <div className="task-center-row-body">
      <div className="task-center-row-heading"><h2>{task.title}</h2><span className={`task-center-state task-center-state-${task.status}`} title={task.rawStatus}>{statusLabels[task.status]}</span></div>
      <div className="task-center-meta"><span>{task.sourceLabel}</span>{task.updatedAt > 0 && <time dateTime={new Date(task.updatedAt).toISOString()}>{new Date(task.updatedAt).toLocaleString()}</time>}{task.adopted && <span>已采用{task.rating ? ` · ${task.rating} 星` : ""}</span>}</div>
      {task.detail && <p className="task-center-detail">{task.detail}</p>}
      {hasProgress && progress !== undefined && <div className="task-center-progress"><progress max={100} value={progress > 0 ? progress : undefined} aria-label="任务进度" /><span>{progress > 0 ? `${Math.round(progress)}%` : "进度待更新"}</span></div>}
      {task.queryError && <p className="task-center-query-error">查询暂不可用：{task.queryError}</p>}
      {task.error && <details className="task-center-error"><summary>错误详情</summary><p>{task.error}</p></details>}
      <div className="task-center-row-actions">
        <button type="button" className="task-center-command" onClick={() => onNavigate(task.target)}><ArrowUpRight size={14} />打开来源</button>
        {task.error && <button type="button" className="task-center-command" onClick={() => onNavigate({ ...task.target, view: "error" })}><CircleAlert size={14} />查看错误</button>}
        {task.results.map((result, index) => <button type="button" key={index} className="task-center-command" onClick={() => onNavigate(result.target)}><FileOutput size={14} />{result.label}</button>)}
        {task.target.type === "session" && <SessionResults target={task.target} onNavigate={onNavigate} />}
      </div>
    </div>
  </article>;
}

export default function TaskCenter({ onNavigate, sessionActivity, refreshIntervalMs = 8_000 }: TaskCenterProps) {
  const [snapshot, setSnapshot] = useState(emptyTaskSnapshot);
  const [loading, setLoading] = useState(true);
  const [status, setStatus] = useState<TaskStatus | "all">("all");
  const [source, setSource] = useState<TaskSource | "all">("all");
  const [search, setSearch] = useState("");
  const [mediaTarget, setMediaTarget] = useState<{ taskId: string; kind: LocalMediaKind; session: MarketingSessionSnapshot } | null>(null);
  const { tasks: harvis, refresh: refreshHarvis, session: harvisSession } = useHarvisTasks(undefined, refreshIntervalMs > 0, refreshIntervalMs);
  const current = useRef(snapshot);
  const mounted = useRef(false);
  const inFlight = useRef<Promise<void> | null>(null);
  const refresh = useCallback(() => {
    if (inFlight.current) return inFlight.current;
    setLoading(true);
    const request = loadTaskSources(current.current).then((next) => {
      if (!mounted.current) return;
      current.current = next;
      setSnapshot(next);
    }).finally(() => {
      inFlight.current = null;
      if (mounted.current) setLoading(false);
    });
    inFlight.current = request;
    return request;
  }, []);
  useEffect(() => {
    mounted.current = true;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      await refresh();
      if (!disposed && refreshIntervalMs > 0) timer = setTimeout(() => void poll(), refreshIntervalMs);
    };
    void poll();
    return () => { disposed = true; mounted.current = false; clearTimeout(timer); };
  }, [refresh, refreshIntervalMs]);
  const tasks = useMemo(() => aggregateTasks({ ...snapshot, harvis }, sessionActivity), [snapshot, harvis, sessionActivity]);
  const visible = useMemo(() => filterTasks(tasks, { status, source, search }), [tasks, status, source, search]);
  const navigate = useCallback((target: TaskCenterTarget) => {
    if (target.type === "harvis" && (target.kind === "video" || target.kind === "tts")) {
      setMediaTarget({ taskId: target.taskId, kind: target.kind, session: harvisSession });
    } else onNavigate(target);
  }, [onNavigate, harvisSession.userId, harvisSession.generation]);
  const closeMedia = useCallback(() => setMediaTarget(null), []);

  return <section className="task-center-page" aria-label="任务中心">
    <header className="task-center-header"><div><h1>任务中心</h1><span className="task-center-count" role="status">{visible.length} / {tasks.length} 项{loading ? " · 更新中" : ""}</span></div><button type="button" className="task-center-icon" title="刷新任务" aria-label="刷新任务" disabled={loading} onClick={() => { void refresh(); void refreshHarvis(); }}><RefreshCw size={17} className={loading ? "task-center-spinning" : ""} /></button></header>
    <div className="task-center-toolbar">
      <label className="task-center-search"><Search size={15} aria-hidden="true" /><input type="search" aria-label="搜索任务" placeholder="搜索任务" value={search} onChange={(event) => setSearch(event.target.value)} /></label>
      <select aria-label="任务状态" value={status} onChange={(event) => setStatus(event.target.value as TaskStatus | "all")}><option value="all">全部状态</option>{Object.entries(statusLabels).map(([value, label]) => <option value={value} key={value}>{label}</option>)}</select>
      <select aria-label="任务来源" value={source} onChange={(event) => setSource(event.target.value as TaskSource | "all")}><option value="all">全部来源</option>{Object.entries(sourceLabels).map(([value, label]) => <option value={value} key={value}>{label}</option>)}</select>
    </div>
    {snapshot.errors.map((error) => <div className="task-center-load-error" role="alert" key={error.source}><CircleAlert size={15} /><span>{loadSourceLabels[error.source]}读取失败：{error.message}</span></div>)}
    <div className="task-center-list" aria-busy={loading}>
      {visible.map((task) => <TaskRow key={task.id} task={task} onNavigate={navigate} />)}
      {!visible.length && <div className="task-center-empty">{loading ? <><LoaderCircle size={16} className="task-center-spinning" />正在读取任务…</> : search || status !== "all" || source !== "all" ? "没有匹配的任务" : "暂无任务"}</div>}
    </div>
    {mediaTarget && isCurrentMarketingSession(mediaTarget.session) && <HarvisMediaPreview key={`${mediaTarget.kind}:${mediaTarget.taskId}`} task={harvis.find((task) => task.taskId === mediaTarget.taskId && task.kind === mediaTarget.kind)} kind={mediaTarget.kind} onClose={closeMedia} />}
  </section>;
}
