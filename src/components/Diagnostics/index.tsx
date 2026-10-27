import { useCallback, useEffect, useRef, useState } from "react";
import { Download, RefreshCw, ShieldCheck, Wrench, X } from "lucide-react";
import { runtimeDiagnostics, type RuntimeDiagnosticStatus, type RuntimeDiagnosticsReport } from "../../services/runtimeDiagnostics";
import "./Diagnostics.css";

export interface DiagnosticsProps {
  onClose?: () => void;
  autoCheck?: boolean;
}

const labels: Record<RuntimeDiagnosticStatus, string> = {
  ok: "正常", missing: "缺失", broken: "异常", timeout: "超时", warning: "待确认",
};

export default function Diagnostics({ onClose, autoCheck = true }: DiagnosticsProps) {
  const [report, setReport] = useState<RuntimeDiagnosticsReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const mounted = useRef(false);
  const running = useRef(false);
  const run = useCallback(async (action: "check" | "repair" | "export") => {
    if (running.current) return;
    running.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      if (action === "export") {
        const saved = await runtimeDiagnostics.export();
        if (mounted.current) setNotice(saved ? "已保存脱敏报告" : "已取消保存");
      } else {
        const next = await runtimeDiagnostics[action]();
        if (mounted.current) setReport(next);
      }
    } catch (cause) {
      if (mounted.current) setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      running.current = false;
      if (mounted.current) setBusy(false);
    }
  }, []);
  useEffect(() => {
    mounted.current = true;
    if (autoCheck) void run("check");
    return () => { mounted.current = false; };
  }, [autoCheck, run]);

  return <section className="runtime-diagnostics" aria-label="环境自检" aria-busy={busy}>
    <header className="runtime-diagnostics-header">
      <div><h2><ShieldCheck size={20} aria-hidden="true" />环境自检</h2>
        <p role="status">{busy ? "正在处理…" : report ? report.healthy ? "环境正常" : "存在待处理项" : "尚未检测"}</p></div>
      {onClose && <button type="button" className="diagnostics-icon" title="关闭诊断" aria-label="关闭诊断" onClick={onClose}><X size={18} /></button>}
    </header>
    <div className="runtime-diagnostics-actions">
      <button type="button" disabled={busy} onClick={() => void run("check")}><RefreshCw size={16} />重新检测</button>
      <button type="button" disabled={busy} onClick={() => void run("repair")}><Wrench size={16} />离线修复</button>
      <button type="button" disabled={busy} onClick={() => void run("export")}><Download size={16} />导出脱敏报告</button>
    </div>
    {error && <p className="diagnostics-error" role="alert">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    {report && <>
      <p className="diagnostics-meta">{report.platform} · <time dateTime={report.checked_at}>{new Date(report.checked_at).toLocaleString()}</time></p>
      <ul className="diagnostics-results">{report.items.map((item) => <li key={item.key}>
        <div className="diagnostics-item-heading"><h3>{item.name}</h3><span className={`diagnostics-state diagnostics-${item.status}`}>{labels[item.status]}</span></div>
        <dl>{item.path && <><dt>路径</dt><dd>{item.path}</dd></>}{item.version && <><dt>版本</dt><dd>{item.version}</dd></>}</dl>
        <p>{item.details}</p>{item.status !== "ok" && <p className="diagnostics-remediation">{item.remediation}</p>}
      </li>)}</ul>
    </>}
  </section>;
}
