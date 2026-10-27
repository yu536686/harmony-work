import { useEffect, useRef, useState } from "react";
import { Download, RefreshCw, X } from "lucide-react";
import { save } from "@tauri-apps/plugin-dialog";
import { creativeApi, type HarvisTask } from "../../services/tauri/creative";
import { taskErrorMessage } from "../../tasks/taskCenter";
import { resolveMediaSource, type LocalMediaKind } from "./mediaSource";

interface HarvisMediaPreviewProps {
  task?: HarvisTask;
  kind: LocalMediaKind;
  onClose: () => void;
}

export default function HarvisMediaPreview({ task, kind, onClose }: HarvisMediaPreviewProps) {
  const dialog = useRef<HTMLDivElement>(null);
  const mounted = useRef(true);
  const [mediaError, setMediaError] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [saving, setSaving] = useState(false);
  const [saveNotice, setSaveNotice] = useState("");
  const [saveError, setSaveError] = useState("");
  const source = resolveMediaSource(task?.resultUrl, kind);
  useEffect(() => {
    mounted.current = true;
    const previousFocus = document.activeElement;
    dialog.current?.querySelector<HTMLButtonElement>("button")?.focus();
    return () => {
      mounted.current = false;
      if (previousFocus instanceof HTMLElement && previousFocus.isConnected) previousFocus.focus();
    };
  }, []);
  useEffect(() => { setMediaError(false); setSaveNotice(""); setSaveError(""); }, [task?.resultUrl]);

  const saveResult = async () => {
    if (!source || !task || saving) return;
    setSaving(true);
    setSaveError("");
    setSaveNotice("");
    try {
      const targetPath = await save({
        title: "保存 Harvis 创作成品",
        defaultPath: `Harvis-${task.taskId.replace(/[^a-zA-Z0-9_-]/g, "")}.${source.extension}`,
        filters: [{ name: "创作成品", extensions: [source.extension] }],
      });
      if (!mounted.current || !targetPath || Array.isArray(targetPath)) return;
      const result = await creativeApi.harvisSaveResult(source.url, targetPath);
      if (mounted.current) setSaveNotice(`已保存到 ${result.localPath}`);
    } catch (error) {
      if (mounted.current) setSaveError(`保存失败：${taskErrorMessage(error)}`);
    } finally {
      if (mounted.current) setSaving(false);
    }
  };

  return <div className="task-center-preview-mask" onClick={(event) => { if (event.target === event.currentTarget) onClose(); }} onKeyDown={(event) => {
    if (event.key === "Escape") { event.stopPropagation(); onClose(); }
    if (event.key === "Tab") {
      const nodes = Array.from(dialog.current?.querySelectorAll<HTMLElement>("button:not(:disabled),video[controls],audio[controls]") ?? []);
      const first = nodes[0];
      const last = nodes[nodes.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    }
  }}>
    <div className="task-center-preview" ref={dialog} role="dialog" aria-modal="true" aria-label={kind === "video" ? "Harvis 视频成果" : "Harvis 语音成果"}>
      <header className="task-center-preview-heading"><h2>{kind === "video" ? "Harvis 视频成果" : "Harvis 语音成果"}</h2><button type="button" className="task-center-icon" title="关闭预览" aria-label="关闭预览" onClick={onClose}><X size={16} /></button></header>
      {!task ? <p role="alert">任务不存在或已删除</p> : <>
        <p className="task-center-preview-prompt">{task.prompt}</p>
        <p className="task-center-meta">{({ pending: "排队中", processing: "生成中", succeeded: "已完成", failed: "生成失败" })[task.status]}</p>
        {task.status === "failed" && task.errorMessage && <p role="alert" className="task-center-error">{task.errorMessage}</p>}
        {task.queryError && <p className="task-center-query-error">查询暂不可用：{task.queryError}</p>}
        {!task.resultUrl ? <p role="status">任务尚无可用成品</p> : !source ? <p role="alert" className="task-center-error">无法安全预览或保存：仅支持 HTTPS 或匹配媒体类型的 data URL</p> : <>
          {kind === "video" ? <video key={attempt} src={source.url} controls playsInline crossOrigin="anonymous" preload="metadata" aria-label="视频成果" onError={() => setMediaError(true)} /> : <audio key={attempt} src={source.url} controls crossOrigin="anonymous" preload="metadata" aria-label="语音成果" onError={() => setMediaError(true)} />}
          {mediaError && <p role="alert" className="task-center-error">媒体加载失败，链接可能已过期、禁止跨域访问或编码不受支持。可重试或保存成品后查看。</p>}
        </>}
        <div className="task-center-preview-actions">
          {source && mediaError && <button type="button" className="task-center-command" onClick={() => { setMediaError(false); setAttempt((value) => value + 1); }}><RefreshCw size={14} />重试预览</button>}
          <button type="button" className="task-center-command" disabled={!source || saving} onClick={() => void saveResult()}><Download size={14} />{saving ? "正在保存…" : "保存成品"}</button>
        </div>
        {saveError && <p role="alert" className="task-center-error">{saveError}</p>}
        {saveNotice && <p role="status">{saveNotice}</p>}
      </>}
    </div>
  </div>;
}
