import { useCallback, useEffect, useId, useRef, useState } from "react";
import { FileDiff, RefreshCw, RotateCcw } from "lucide-react";
import { journalApi, type JournalDiff, type JournalReview, type JournalScope, type ReviewChange } from "../../services/tauri/journal";
import { lineDiff } from "./lineDiff";
import "./style.css";

export interface FileReviewProps extends JournalScope { turnKey?: string; refreshKey?: string | number }

const kindNames = { added: "新增", deleted: "删除", modified: "修改" };
function size(bytes: number) { return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KiB`; }
function errorText(error: unknown) {
  const raw = error instanceof Error ? error.message : String(error);
  if (raw.includes("CONFLICT") || raw.includes("STALE_REVIEW")) return `恢复已停止，后续修改未被覆盖。请刷新并核对当前文件。${raw}`;
  if (raw.includes("not found") || raw.includes("not allowed")) return `文件审阅命令不可用，请检查客户端接入或升级后重试。${raw}`;
  return `${raw}。请刷新重试；若工作区已变更，请重新打开当前会话。`;
}

function Confirmation({ change, busy, onCancel, onConfirm }: { change: ReviewChange; busy: boolean; onCancel: () => void; onConfirm: () => void }) {
  const title = useId();
  const cancel = useRef<HTMLButtonElement>(null);
  const confirm = useRef<HTMLButtonElement>(null);
  useEffect(() => { cancel.current?.focus(); }, []);
  return <div className="file-review-backdrop">
    <div className="file-review-confirm" role="dialog" aria-modal="true" aria-labelledby={title} onKeyDown={event => {
      if (event.key === "Escape" && !busy) onCancel();
      if (event.key === "Tab") {
        event.preventDefault();
        (document.activeElement === cancel.current ? confirm.current : cancel.current)?.focus();
      }
    }}>
      <h3 id={title}>确认恢复文件</h3>
      <p className="file-review-path">{change.path}</p>
      <p>{change.kind === "added" ? "此轮新增的文件将被删除。" : change.kind === "deleted" ? "文件将恢复为此轮开始前的内容。" : "文件将替换为此轮开始前的内容。"}当前文件与记录不一致时，恢复将被拒绝。</p>
      <div className="file-review-confirm-actions">
        <button ref={cancel} disabled={busy} onClick={onCancel}>取消</button>
        <button ref={confirm} disabled={busy} onClick={onConfirm}><RotateCcw size={14} />{busy ? "恢复中..." : "确认恢复"}</button>
      </div>
    </div>
  </div>;
}

function DiffView({ diff }: { diff: JournalDiff }) {
  return <>
    <div className="file-review-summary">
      <span>变更前：{diff.before ? size(diff.before.size) : "不存在"}</span>
      <span>变更后：{diff.after ? size(diff.after.size) : "不存在"}</span>
      {diff.before?.hash && <span title={diff.before.hash}>前 SHA-256：<code>{diff.before.hash}</code></span>}
      {diff.after?.hash && <span title={diff.after.hash}>后 SHA-256：<code>{diff.after.hash}</code></span>}
    </div>
    {diff.kind === "text" ? <div className="file-review-diff" aria-label="文件差异">
      {lineDiff(diff.beforeText ?? "", diff.afterText ?? "").map((line, index) => <div className={`file-review-line ${line.kind}`} key={index}>
        <span aria-hidden="true">{line.before ?? ""}</span><span aria-hidden="true">{line.after ?? ""}</span>
        <span aria-hidden="true">{line.kind === "removed" ? "-" : line.kind === "added" ? "+" : " "}</span><code>{line.text || " "}</code>
      </div>)}
    </div> : <p className="file-review-empty">{diff.kind === "binary" ? "二进制文件，仅显示摘要与大小" : diff.kind === "too_large" ? "文件超过差异显示上限，仅显示摘要与大小" : "未捕获完整内容，无法安全审阅或恢复"}{diff.message && <small>{diff.message}</small>}</p>}
  </>;
}

function ReviewPanel({ sessionId, workspaceRoot, turnKey, refreshKey }: FileReviewProps) {
  const [review, setReview] = useState<JournalReview | null>(null);
  const [turn, setTurn] = useState<string | undefined>(turnKey);
  const [selected, setSelected] = useState<ReviewChange | null>(null);
  const [diff, setDiff] = useState<JournalDiff | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [loading, setLoading] = useState(false);
  const [diffLoading, setDiffLoading] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [confirmation, setConfirmation] = useState<ReviewChange | null>(null);
  const listRequest = useRef(0);
  const diffRequest = useRef(0);
  const alive = useRef(true);
  const scope = { sessionId, workspaceRoot };

  const refresh = useCallback(async () => {
    const request = ++listRequest.current;
    ++diffRequest.current;
    setLoading(true); setError(""); setSelected(null); setDiff(null); setConfirmation(null); setDiffLoading(false);
    try {
      const next = await journalApi.review({ sessionId, workspaceRoot }, turn);
      if (alive.current && request === listRequest.current) setReview(next);
    } catch (error) {
      if (alive.current && request === listRequest.current) { setReview(null); setError(errorText(error)); }
    } finally {
      if (alive.current && request === listRequest.current) setLoading(false);
    }
  }, [sessionId, workspaceRoot, turn]);

  useEffect(() => {
    alive.current = true;
    void refresh();
    return () => { alive.current = false; ++listRequest.current; ++diffRequest.current; };
  }, [refresh, refreshKey]);

  async function select(change: ReviewChange) {
    if (!review?.turnKey) return;
    const request = ++diffRequest.current;
    setSelected(change); setDiff(null); setDiffLoading(true); setError(""); setNotice(""); setConfirmation(null);
    try {
      const data = await journalApi.diff(scope, review.turnKey, change.path);
      if (alive.current && request === diffRequest.current) setDiff(data);
    } catch (error) {
      if (alive.current && request === diffRequest.current) setError(errorText(error));
    } finally {
      if (alive.current && request === diffRequest.current) setDiffLoading(false);
    }
  }

  async function restore() {
    if (!confirmation || !review?.turnKey) return;
    const change = confirmation;
    setRestoring(true); setError(""); setNotice("");
    try {
      await journalApi.restore(scope, review.turnKey, change, true);
      if (alive.current) { await refresh(); if (alive.current) setNotice(`已恢复 ${change.path}`); }
    } catch (error) {
      if (alive.current) { setConfirmation(null); setError(errorText(error)); }
    } finally {
      if (alive.current) setRestoring(false);
    }
  }

  return <section className="file-review" aria-label="文件变更审阅" aria-busy={loading || restoring}>
    <header className="file-review-toolbar">
      <h2><FileDiff size={17} />文件变更 <span>{review?.changes.length ?? 0}</span></h2>
      {review && review.turns.length > 0 && <select aria-label="审阅轮次" value={turn ?? review.turnKey ?? ""} disabled={restoring || loading} onChange={event => { setNotice(""); setTurn(event.target.value); }}>
        {review.turns.map(item => <option key={item.turnKey} value={item.turnKey}>{item.turnKey}{item.status === "capturing" ? "（进行中）" : item.status === "unavailable" || item.status === "abandoned" ? "（审阅不可用）" : ""}</option>)}
      </select>}
      <button type="button" title="刷新变更" aria-label="刷新变更" disabled={loading || restoring} onClick={() => { setNotice(""); void refresh(); }}><RefreshCw size={16} /></button>
    </header>
    {error && <div className="file-review-error" role="alert">{error}</div>}
    {notice && <p className="file-review-notice" role="status">{notice}</p>}
    {review?.status === "unavailable" && <div className="file-review-error" role="alert">{review.captureError || "本轮未取得可信快照"}。对话不受影响，但本轮无法安全审阅或恢复。{review.captureActive ? "仍有写入保护，等待运行时真实结束后刷新。" : "请缩小工作区或等待并行任务结束后启动新轮次。"}</div>}
    {review?.warnings.map((warning, index) => <p className="file-review-warning" key={index}>{warning}</p>)}
    {loading ? <p className="file-review-empty">加载中...</p> : !review || review.changes.length === 0 ? <p className="file-review-empty">{!review ? "无法加载文件审阅" : review.status === "capturing" ? "本轮快照尚未完成" : review.status === "unavailable" ? "本轮文件审阅不可用" : review.status === "abandoned" ? "本轮捕获已放弃，无法审阅或恢复" : review.status === "empty" ? "暂无已捕获的轮次" : "本轮没有文件内容变更"}</p> : <div className="file-review-body">
      <nav className="file-review-list" aria-label="变更文件">
        {review.changes.map(change => <button type="button" key={change.path} disabled={restoring} aria-pressed={selected?.path === change.path} onClick={() => void select(change)}>
          <span className={`file-review-kind ${change.kind}`}>{kindNames[change.kind]}</span>
          <span className="file-review-path">{change.path}</span>
          {change.restored && <span>已恢复</span>}{!change.canRestore && !change.restored && <span>不可恢复</span>}
        </button>)}
      </nav>
      <div className="file-review-detail">
        {selected ? <>
          <div className="file-review-detail-toolbar"><strong className="file-review-path">{selected.path}</strong>
            <button type="button" title="恢复此文件" aria-label="恢复此文件" disabled={!selected.canRestore || !diff || diffLoading || restoring} onClick={() => setConfirmation({ ...selected })}><RotateCcw size={15} /></button>
          </div>
          {diffLoading && <p className="file-review-empty">加载差异中...</p>}
          {diff && <DiffView diff={diff} />}
          {!selected.canRestore && <p className="file-review-warning">{selected.restored ? "此文件已恢复" : "快照不完整，已禁用恢复"}</p>}
        </> : <p className="file-review-empty">未选择文件</p>}
      </div>
    </div>}
    {confirmation && <Confirmation change={confirmation} busy={restoring} onCancel={() => setConfirmation(null)} onConfirm={() => void restore()} />}
  </section>;
}

/** Scope-keyed mounting prevents old asynchronous work from crossing sessions. */
export default function FileReview(props: FileReviewProps) {
  return <ReviewPanel key={`${props.sessionId}\0${props.workspaceRoot}\0${props.turnKey ?? ""}`} {...props} />;
}
