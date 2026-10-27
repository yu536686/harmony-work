import { useCallback, useEffect, useRef, useState } from "react";
import { useDispatch, useSelector, useStore } from "react-redux";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { RootState, schedulerActions } from "../../store";
import { schedulerApi, ScheduledTask } from "../../services/tauri";
import ConfirmDialog from "../ConfirmDialog";

export interface SchedulerProps {
  focusedTaskId?: string | null;
}

export default function Scheduler({ focusedTaskId = null }: SchedulerProps = {}) {
  const { t } = useTranslation();
  const dispatch = useDispatch();
  const store = useStore<RootState>();
  const { tasks } = useSelector((s: RootState) => s.scheduler);
  const [loaded, setLoaded] = useState(false);
  const [loadError, setLoadError] = useState("");
  const taskNodes = useRef(new Map<string, HTMLDivElement>());
  const [showForm, setShowForm] = useState(false);
  const [form, setForm] = useState({
    name: "",
    description: "",
    cron_expression: "0 9 * * 1-5",
    task_prompt: "",
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);

  useEffect(() => {
    let active = true;
    schedulerApi.list().then(({ tasks }) => {
      if (!active) return;
      dispatch(schedulerActions.setTasks(tasks));
      setLoaded(true);
    }).catch((e: unknown) => {
      if (active) setLoadError(e instanceof Error ? e.message : String(e));
    });
    return () => { active = false; };
  }, [dispatch]);

  const focusedTask = tasks.find((task) => task.id === focusedTaskId);
  useEffect(() => {
    if (!loaded || !focusedTaskId || !focusedTask) return;
    const node = taskNodes.current.get(focusedTaskId);
    node?.scrollIntoView?.({ block: "nearest" });
    node?.focus({ preventScroll: true });
  }, [loaded, focusedTaskId, focusedTask?.id]);

  const patchTask = useCallback((id: string, changes: Partial<ScheduledTask>) => {
    // Redux updates synchronously; reading here also handles events batched before React re-renders.
    const latest = store.getState().scheduler.tasks;
    if (!latest.some((task) => task.id === id)) return;
    dispatch(schedulerActions.setTasks(latest.map((task) => task.id === id ? { ...task, ...changes } : task)));
  }, [dispatch, store]);

  const handleCreate = async () => {
    if (!form.name.trim() || !form.task_prompt.trim()) {
      setError(t("scheduler.nameRequired"));
      return;
    }
    setSaving(true);
    setError("");
    try {
      const task = await schedulerApi.create({
        name: form.name,
        description: form.description || undefined,
        cron_expression: form.cron_expression,
        task_prompt: form.task_prompt,
      });
      dispatch(schedulerActions.addTask(task));
      setShowForm(false);
      setForm({ name: "", description: "", cron_expression: "0 9 * * 1-5", task_prompt: "" });
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = (id: string) => {
    setDeleteTarget(id);
  };

  const doDelete = async () => {
    if (!deleteTarget) return;
    setDeleting(true);
    try {
      await schedulerApi.delete(deleteTarget);
      dispatch(schedulerActions.removeTask(deleteTarget));
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setDeleting(false);
      setDeleteTarget(null);
    }
  };

  const handleToggle = async (task: ScheduledTask) => {
    const newStatus = task.status === "active" ? "paused" : "active";
    try {
      await schedulerApi.update({ task_id: task.id, status: newStatus });
      patchTask(task.id, { status: newStatus });
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleRunNow = async (id: string) => {
    try {
      // Optimistically mark as running
      patchTask(id, { last_run_status: "running" });
      await schedulerApi.runNow(id);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  // Listen for real-time status updates from backend
  useEffect(() => {
    let active = true;
    const unlisteners: Array<() => void> = [];
    for (const task of tasks) {
      void listen<{ status: string }>(`task_status_${task.id}`, (e) => {
        if (active) patchTask(task.id, { last_run_status: e.payload.status });
      }).then((unlisten) => {
        if (active) unlisteners.push(unlisten);
        else unlisten();
      }).catch((cause: unknown) => {
        if (active) setError(cause instanceof Error ? cause.message : String(cause));
      });
    }
    return () => {
      active = false;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [JSON.stringify(tasks.map((task) => task.id).sort()), patchTask]);

  return (
    <div className="page">
      <div className="page-header">
        <h1 className="page-title">⏰ {t("scheduler.title")}</h1>
        <div className="page-header-actions">
          <button type="button" className="btn-header btn-header-primary" onClick={() => setShowForm(!showForm)}>
            + {t("scheduler.newTask")}
          </button>
        </div>
      </div>

      <div className="page-body">
        {(loadError || (!showForm && error)) && <div role="alert" style={{ marginBottom: 12, color: "var(--error)", overflowWrap: "anywhere" }}>{loadError || error}</div>}
        {loaded && focusedTaskId && !focusedTask && <div role="status" style={{ marginBottom: 12, color: "var(--text-secondary)" }}>目标定时任务不存在或已删除</div>}
        {showForm && (
          <div className="card" style={{ marginBottom: 20 }}>
            <h3 style={{ marginBottom: 16, color: "var(--text-primary)" }}>{t("scheduler.newTaskTitle")}</h3>
            {error && (
              <div style={{ padding: "8px 12px", background: "rgba(248,113,113,0.1)", border: "1px solid var(--error)", borderRadius: "var(--radius)", color: "var(--error)", marginBottom: 12, fontSize: 13 }}>
                {error}
              </div>
            )}
            <div className="form-group">
              <label className="label">{t("scheduler.taskName")}</label>
              <input className="input" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} placeholder={t("scheduler.taskNamePlaceholder")} />
            </div>
            <div className="form-group">
              <label className="label">{t("scheduler.description")}</label>
              <input className="input" value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })} placeholder={t("scheduler.descriptionPlaceholder")} />
            </div>
            <div className="form-group">
              <label className="label">{t("scheduler.cronExpression")}</label>
              <input className="input" value={form.cron_expression} onChange={(e) => setForm({ ...form, cron_expression: e.target.value })} placeholder={t("scheduler.cronPlaceholder")} style={{ fontFamily: "var(--font-mono)" }} />
              <p style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 4 }}>{t("scheduler.cronHelp")}</p>
            </div>
            <div className="form-group">
              <label className="label">{t("scheduler.taskPrompt")}</label>
              <textarea className="input" value={form.task_prompt} onChange={(e) => setForm({ ...form, task_prompt: e.target.value })} placeholder={t("scheduler.taskPromptPlaceholder")} rows={4} />
            </div>
            <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
              <button className="btn btn-secondary" onClick={() => setShowForm(false)}>{t("common.cancel")}</button>
              <button className="btn btn-primary" onClick={handleCreate} disabled={saving}>
                {saving ? t("common.creating") : t("scheduler.newTask").replace("+ ", "")}
              </button>
            </div>
          </div>
        )}

        {tasks.length === 0 ? (
          <div className="empty-state">
            <div className="empty-state-icon">⏰</div>
            <div className="empty-state-title">{t("scheduler.noTasks")}</div>
            <div className="empty-state-desc">{t("scheduler.noTasksDesc")}</div>
          </div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            {tasks.map((task) => (
              <div key={task.id} className="card" role="article" aria-label={task.name} tabIndex={-1} data-task-id={task.id} data-focused={task.id === focusedTaskId}
                ref={(node) => { if (node) taskNodes.current.set(task.id, node); else taskNodes.current.delete(task.id); }}
                style={task.id === focusedTaskId ? { outline: "2px solid var(--accent-color)", outlineOffset: -2 } : undefined}>
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", gap: 12 }}>
                  <div style={{ flex: 1 }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6, flexWrap: "wrap" }}>
                      <span style={{ fontWeight: 600, color: "var(--text-primary)" }}>{task.name}</span>
                      <span className={`badge ${task.status === "active" ? "badge-success" : "badge-warning"}`}>
                        {task.status === "active" ? t("scheduler.statusActive") : t("scheduler.statusPaused")}
                      </span>
                      {task.last_run_status && (
                        <span className={`badge ${
                          task.last_run_status === "running" ? "badge-info" :
                          task.last_run_status === "success" ? "badge-success" :
                          task.last_run_status === "no_change" ? "badge-info" :
                          "badge-danger"
                        }`} style={{ fontSize: 11 }}>
                          {task.last_run_status === "running"
                            ? `⟳ ${t("scheduler.lastRunRunning")}`
                            : task.last_run_status === "success"
                            ? `✓ ${t("scheduler.lastRunSuccess")}`
                            : task.last_run_status === "no_change"
                            ? t("memory.dreamNoChange")
                            : `✗ ${t("scheduler.lastRunFailed")}`}
                        </span>
                      )}
                    </div>
                    {task.description && (
                      <p style={{ fontSize: 13, color: "var(--text-secondary)", marginBottom: 6 }}>{task.description}</p>
                    )}
                    <div style={{ display: "flex", gap: 12, fontSize: 12, color: "var(--text-muted)" }}>
                      <span style={{ fontFamily: "var(--font-mono)" }}>⏱ {task.cron_expression}</span>
                      <span>{t("scheduler.runs", { count: task.run_count })}</span>
                      {task.last_run_at && <span>{t("scheduler.lastRun", { time: new Date(task.last_run_at).toLocaleString() })}</span>}
                    </div>
                  </div>
                  <div style={{ display: "flex", gap: 6, flexShrink: 0 }}>
                    <button className="btn btn-secondary" style={{ padding: "4px 10px", fontSize: 12 }} onClick={() => handleRunNow(task.id)}>
                      ▶ {t("common.run")}
                    </button>
                    <button className="btn btn-secondary" style={{ padding: "4px 10px", fontSize: 12 }} onClick={() => handleToggle(task)}>
                      {task.status === "active" ? `⏸ ${t("common.pause")}` : `▶ ${t("common.resume")}`}
                    </button>
                    <button className="btn btn-danger" style={{ padding: "4px 10px", fontSize: 12 }} onClick={() => handleDelete(task.id)}>
                      {t("common.delete")}
                    </button>
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      <ConfirmDialog
        open={!!deleteTarget}
        title={t("common.delete")}
        message={t("scheduler.confirmDelete")}
        confirmLabel={t("common.delete")}
        cancelLabel={t("common.cancel")}
        loading={deleting}
        onConfirm={doDelete}
        onCancel={() => !deleting && setDeleteTarget(null)}
      />
    </div>
  );
}
