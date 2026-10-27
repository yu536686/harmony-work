import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RefreshCw, Save } from "lucide-react";
import "./UsageBudget.css";

interface UsageOverview {
  workspace_root: string;
  session_input_tokens: number;
  session_output_tokens: number;
  project_tokens: number;
  session_count: number;
  budget: { session_tokens: number; project_tokens: number };
}

export default function UsageBudget({ sessionId }: { sessionId: string }) {
  const [usage, setUsage] = useState<UsageOverview | null>(null);
  const [limits, setLimits] = useState({ session_tokens: 0, project_tokens: 0 });
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const generation = useRef(0);
  useEffect(() => {
    const request = ++generation.current;
    setUsage(null); setError(""); setBusy(false);
    invoke<UsageOverview>("usage_get", { sessionId }).then(result => {
      if (request === generation.current) { setUsage(result); setLimits(result.budget); }
    }).catch(reason => { if (request === generation.current) setError(String(reason)); });
    return () => { ++generation.current; };
  }, [sessionId]);
  const update = async (save: boolean) => {
    const request = ++generation.current;
    setBusy(true); setError("");
    try {
      const result = await invoke<UsageOverview>(save ? "usage_set_budget" : "usage_get", { sessionId, ...(save ? { budget: limits } : {}) });
      if (request === generation.current) { setUsage(result); setLimits(result.budget); }
    } catch (reason) { if (request === generation.current) setError(String(reason)); }
    finally { if (request === generation.current) setBusy(false); }
  };
  const format = (value: number) => value.toLocaleString();
  return <section className="usage-budget">
    <header><h2>用量与限额</h2><button className="workbench-icon-button" title="刷新用量" aria-label="刷新用量" disabled={busy} onClick={() => void update(false)}><RefreshCw size={16} /></button></header>
    {error && <p role="alert">{error}</p>}
    {!usage && !error && <p role="status">正在读取…</p>}
    {usage && <>
      <dl><div><dt>会话输入</dt><dd>{format(usage.session_input_tokens)} Token</dd></div>
        <div><dt>会话输出</dt><dd>{format(usage.session_output_tokens)} Token</dd></div>
        <div><dt>项目累计</dt><dd>{format(usage.project_tokens)} Token</dd></div>
        <div><dt>项目会话数</dt><dd>{usage.session_count}</dd></div></dl>
      <p className="usage-workspace" title={usage.workspace_root}>{usage.workspace_root}</p>
      <form onSubmit={event => { event.preventDefault(); void update(true); }}>
        <label>每会话 Token 上限<input className="input" type="number" min={0} max={1000000000000} step={1} required value={limits.session_tokens} onChange={event => setLimits(value => ({ ...value, session_tokens: Number(event.target.value) }))} /></label>
        <label>项目 Token 上限<input className="input" type="number" min={0} max={1000000000000} step={1} required value={limits.project_tokens} onChange={event => setLimits(value => ({ ...value, project_tokens: Number(event.target.value) }))} /></label>
        <button type="submit" className="btn btn-primary" disabled={busy}><Save size={14} />保存限额</button>
      </form>
      <p className="usage-note">0 为不限额。按服务报告并已记录的 Token 限制新轮次，不中断正在执行的任务。已删除的会话不计入项目统计；实际账单以服务商为准。</p>
    </>}
  </section>;
}
