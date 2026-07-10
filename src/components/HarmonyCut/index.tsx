import { useEffect, useMemo, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useDispatch, useSelector } from "react-redux";
import { RootState, sessionsActions } from "../../store";
import {
  harmonyCutApi,
  harmonyCutWorkbenchUrl,
  openPath,
  sessionsApi,
  type HarmonyCutHealth,
  type HarmonyCutProjectSummary,
} from "../../services/tauri";
import "./HarmonyCut.css";

const ACTIVE_PROJECT_KEY = "workbuddy.harmonyCut.activeProjectId";

interface HarmonyCutProps {
  projectId?: string | null;
  sourceSessionId?: string | null;
  onNavigateToChat?: () => void;
}

function formatJson(value: unknown) {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value ?? "");
  }
}

export default function HarmonyCut({ projectId, sourceSessionId, onNavigateToChat }: HarmonyCutProps) {
  const dispatch = useDispatch();
  const activeSessionId = useSelector((s: RootState) => s.sessions.activeSessionId);
  const [health, setHealth] = useState<HarmonyCutHealth | null>(null);
  const [projects, setProjects] = useState<HarmonyCutProjectSummary[]>([]);
  const [activeProjectId, setActiveProjectId] = useState<string>(() => {
    return projectId || localStorage.getItem(ACTIVE_PROJECT_KEY) || "";
  });
  const [mediaDir, setMediaDir] = useState("");
  const [title, setTitle] = useState("");
  const [requirement, setRequirement] = useState("12镜头商摄短视频，按开箱、英雄镜头、手持展示、上手佩戴的节奏初剪。");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [lastResult, setLastResult] = useState<unknown>(null);

  useEffect(() => {
    if (!projectId) return;
    setActiveProjectId(projectId);
    localStorage.setItem(ACTIVE_PROJECT_KEY, projectId);
  }, [projectId]);

  const loadStatus = async () => {
    try {
      const nextHealth = await harmonyCutApi.health();
      setHealth(nextHealth);
      if (nextHealth.installed) {
        const list = await harmonyCutApi.projects().catch(() => []);
        setProjects(list);
      }
    } catch (error) {
      setMessage(String(error));
    }
  };

  useEffect(() => {
    void (async () => {
      setBusy(true);
      try {
        const nextHealth = await harmonyCutApi.startService();
        setHealth(nextHealth);
        const list = await harmonyCutApi.projects().catch(() => []);
        setProjects(list);
      } catch (error) {
        setMessage(String(error));
      } finally {
        setBusy(false);
      }
    })();
  }, []);

  const selectedProject = useMemo(
    () => projects.find((item) => item.id === activeProjectId),
    [projects, activeProjectId],
  );

  const workbenchUrl = harmonyCutWorkbenchUrl(activeProjectId || null, health?.serverUrl);

  const chooseFolder = async () => {
    const picked = await openFileDialog({ directory: true, multiple: false });
    if (typeof picked === "string") {
      setMediaDir(picked);
      if (!title.trim()) {
        const parts = picked.split(/[\\/]/).filter(Boolean);
        setTitle(parts[parts.length - 1] || "Harmony Cut");
      }
    }
  };

  const prepare = async () => {
    if (!mediaDir.trim()) {
      setMessage("请先选择素材文件夹。");
      return;
    }
    setBusy(true);
    setMessage("正在分析素材并生成初剪...");
    setLastResult(null);
    try {
      const result = await harmonyCutApi.prepare({
        mediaDir: mediaDir.trim(),
        title: title.trim() || undefined,
        requirement: requirement.trim() || undefined,
        autoRotateLandscape: true,
      });
      setActiveProjectId(result.projectId);
      localStorage.setItem(ACTIVE_PROJECT_KEY, result.projectId);
      setLastResult(result);
      setMessage("初剪已完成，工作台已载入。");
      await loadStatus();
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  };

  const returnToChatAndContinue = async () => {
    if (!activeProjectId) {
      setMessage("还没有可继续的项目。");
      return;
    }
    setBusy(true);
    try {
      const targetSessionId = sourceSessionId || activeSessionId || (await sessionsApi.create("商摄快剪继续生成", "chat")).id;
      if (!activeSessionId && targetSessionId) {
        const sessions = await sessionsApi.list(100).catch(() => null);
        if (sessions) dispatch(sessionsActions.setSessions(sessions.sessions));
      }
      const draft = [
        "已在商摄快剪工作台完成标记并保存。",
        `项目ID：${activeProjectId}`,
        "请继续执行后续 capcut-mate / 剪映草稿生成流程，生成预览和可编辑草稿。",
      ].join("\n");
      dispatch(sessionsActions.openMainChatView({
        filter: "chat",
        sessionId: targetSessionId,
        composerDraft: draft,
        autoSend: true,
      }));
      onNavigateToChat?.();
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  };

  const runContinueHere = async () => {
    if (!activeProjectId) {
      setMessage("还没有可继续的项目。");
      return;
    }
    setBusy(true);
    setMessage("正在生成预览和剪映/CapCut 草稿...");
    try {
      const result = await harmonyCutApi.continue({ projectId: activeProjectId });
      setLastResult(result);
      setMessage("后续生成已完成。");
      await loadStatus();
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="harmony-cut-page">
      <header className="harmony-cut-topbar">
        <div>
          <h1>商摄快剪</h1>
          <p>Agent 初剪、人工快速标记、保存后回到对话继续生成草稿。</p>
        </div>
        <div className="harmony-cut-actions">
          <button className="btn btn-secondary" onClick={() => void loadStatus()} disabled={busy}>刷新</button>
          <button className="btn btn-secondary" onClick={() => void harmonyCutApi.startService().then(setHealth).catch((e) => setMessage(String(e)))} disabled={busy}>启动服务</button>
          <button className="btn btn-primary" onClick={returnToChatAndContinue} disabled={busy || !activeProjectId}>回到对话继续</button>
        </div>
      </header>

      <div className="harmony-cut-statusbar">
        <span className={health?.serverRunning ? "hc-dot ok" : "hc-dot"} />
        <span>{health?.message || "正在检查 Harmony Cut 服务..."}</span>
        {health?.root && <button className="hc-link-btn" onClick={() => void openPath(health.root)}>打开目录</button>}
        {selectedProject && <span className="hc-project-pill">{selectedProject.workflowStatus || "project"} · {selectedProject.title || selectedProject.id}</span>}
      </div>

      {message && <div className="harmony-cut-message">{message}</div>}

      <div className="harmony-cut-shell">
        <aside className="harmony-cut-side">
          <section className="hc-panel">
            <div className="hc-panel-title">新建初剪</div>
            <label>
              素材文件夹
              <div className="hc-path-row">
                <input value={mediaDir} onChange={(e) => setMediaDir(e.target.value)} placeholder="D:\\拍摄素材\\项目" />
                <button className="btn btn-secondary" onClick={chooseFolder}>选择</button>
              </div>
            </label>
            <label>
              项目名
              <input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="可留空" />
            </label>
            <label>
              剪辑要求
              <textarea value={requirement} onChange={(e) => setRequirement(e.target.value)} rows={5} />
            </label>
            <button className="btn btn-primary hc-wide" onClick={prepare} disabled={busy || !mediaDir.trim()}>
              {busy ? "处理中..." : "分析素材并生成初剪"}
            </button>
          </section>

          <section className="hc-panel">
            <div className="hc-panel-title">项目</div>
            <select
              value={activeProjectId}
              onChange={(e) => {
                setActiveProjectId(e.target.value);
                localStorage.setItem(ACTIVE_PROJECT_KEY, e.target.value);
              }}
            >
              <option value="">打开空工作台</option>
              {projects.map((item) => (
                <option key={item.id} value={item.id}>{item.title || item.id}</option>
              ))}
            </select>
            <button className="btn btn-secondary hc-wide" onClick={() => void openPath(workbenchUrl)} disabled={!health?.serverRunning}>
              系统浏览器打开工作台
            </button>
            <button className="btn btn-secondary hc-wide" onClick={runContinueHere} disabled={busy || !activeProjectId}>
              直接生成草稿
            </button>
          </section>

          {lastResult ? (
            <section className="hc-panel hc-result">
              <div className="hc-panel-title">最近结果</div>
              <pre>{formatJson(lastResult)}</pre>
            </section>
          ) : null}
        </aside>

        <main className="harmony-cut-workbench">
          {health?.serverRunning ? (
            <iframe title="Harmony Cut Workbench" src={workbenchUrl} />
          ) : (
            <div className="hc-empty-workbench">
              <strong>等待 Harmony Cut 服务启动</strong>
              <span>如果服务未启动，请点击右上角“启动服务”。</span>
            </div>
          )}
        </main>
      </div>
    </section>
  );
}
