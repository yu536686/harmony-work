import { useEffect, useId, useRef, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import { ArrowUpCircle, LoaderCircle } from "lucide-react";
import { nativeUpdateStore, supportsDesktopUpdates } from "./nativeUpdateStore";
import type { UpdateSnapshot, UpdateStore } from "./updateStore";
import "./updates.css";

interface Props { store?: UpdateStore; enabled?: boolean }
const busyPhases = new Set(["downloading", "verifying", "waiting", "installing"]);
function useUpdate(store: UpdateStore, enabled: boolean) {
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot);
  useEffect(() => { if (enabled) void store.connect(); }, [store, enabled]);
  return snapshot;
}
function bytes(value: number) { return `${(value / 1_000_000).toFixed(1)} MB`; }

function UpdateDialog({ snapshot, store, onClose, manual, anchor }: { snapshot: UpdateSnapshot; store: UpdateStore; onClose: () => void; manual: boolean; anchor?: DOMRect }) {
  const id = useId();
  const dialog = useRef<HTMLDivElement>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  const handingOff = snapshot.phase === "installing";
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    closeButton.current?.focus();
    return () => previous?.focus();
  }, []);
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !handingOff) { event.preventDefault(); onClose(); }
      if (event.key === "Tab") {
        const buttons = dialog.current?.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled), [tabindex='0']");
        if (!buttons?.length) return;
        const first = buttons[0], last = buttons[buttons.length - 1];
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [onClose, handingOff]);
  const busy = busyPhases.has(snapshot.phase);
  const canCancel = snapshot.phase === "downloading" || snapshot.phase === "waiting";
  const progress = snapshot.total && snapshot.total > 0 ? Math.min(100, Math.floor(snapshot.downloaded / snapshot.total * 100)) : null;
  const phaseLabel: Partial<Record<UpdateSnapshot["phase"], string>> = { downloading: "正在下载轻量安装包", verifying: "正在验证安装包", waiting: "等待当前任务结束后安装，可取消等待", installing: "正在准备退出，安装完成后将自动重启" };
  const anchoredStyle = anchor ? { position: "fixed" as const, left: Math.max(12, Math.min(anchor.left, window.innerWidth - 392)), top: Math.max(12, Math.min(anchor.bottom + 10, window.innerHeight - 360)) } : undefined;
  return createPortal(
    <div className={`update-backdrop ${anchor ? "update-backdrop-card" : ""}`} onMouseDown={(e) => { if (e.target === e.currentTarget && !handingOff) onClose(); }}>
      <div className="update-dialog" ref={dialog} role="dialog" aria-modal="true" aria-labelledby={id} style={anchoredStyle}>
        <div className="update-heading"><ArrowUpCircle size={22} /><h2 id={id}>Harmony Work 可用更新</h2></div>
        <p className="update-version">v{snapshot.currentVersion} → v{snapshot.available?.version}</p>
        {snapshot.available?.version.includes("-") && <span className="update-preview-label">测试版本</span>}
        {snapshot.available?.date && <p className="update-date">{new Date(snapshot.available.date).toLocaleDateString()}</p>}
        <div className="update-release-notes">{snapshot.available?.notes || "本次更新包含功能改进与问题修复。"}</div>
        <p className="update-hint">将先下载轻量安装包；安装时自动退出并重启，保留配置、会话和工作区。</p>
        {busy && <div className="update-progress" aria-live="polite"><p>{phaseLabel[snapshot.phase]}</p>{snapshot.phase === "downloading" && <><progress max={100} {...(progress === null ? {} : { value: progress })} /><span>{bytes(snapshot.downloaded)}{snapshot.total ? ` / ${bytes(snapshot.total)} · ${progress}%` : ""}</span></>}</div>}
        {snapshot.error && <p className="update-error" role="alert">{snapshot.error}</p>}
        <div className="update-actions">
          <button ref={closeButton} type="button" onClick={onClose} disabled={handingOff}>{busy ? "收起" : "取消"}</button>
          {canCancel && <button type="button" onClick={() => void store.cancel()}>{snapshot.phase === "waiting" ? "取消等待" : "取消下载"}</button>}
          {!busy && <button type="button" className="update-primary" onClick={() => void store.install()}>{manual ? "立即更新" : "立即安装"}</button>}
        </div>
      </div>
    </div>, document.body,
  );
}

export function UpdateAbout({ store = nativeUpdateStore, enabled = supportsDesktopUpdates() }: Props) {
  const snapshot = useUpdate(store, enabled);
  const [open, setOpen] = useState(false);
  const [message, setMessage] = useState("");
  const [checking, setChecking] = useState(false);
  const [channelBusy, setChannelBusy] = useState(false);
  if (!enabled) return null;
  const busy = busyPhases.has(snapshot.phase);
  async function check() {
    setChecking(true); setMessage("");
    const result = await store.check();
    setChecking(false);
    if (result.available) setOpen(true);
    else setMessage(result.error || "已是最新版本");
  }
  return <div className="update-about">
    <button className="update-check" type="button" onClick={() => void check()} disabled={checking || busy}>{checking ? <><LoaderCircle size={14} className="update-spinner" />正在检查…</> : "检查更新"}</button>
    {message && <span role="status" className={snapshot.error ? "update-error" : "update-hint"}>{message}</span>}
    <label className="update-channel"><input type="checkbox" checked={snapshot.channel === "preview"} disabled={busy || checking || channelBusy} onChange={async (e) => { setChannelBusy(true); setMessage(""); await store.setChannel(e.target.checked ? "preview" : "stable"); setChannelBusy(false); }} />接收测试版本更新</label>
    {snapshot.channel === "preview" && <small className="update-hint">已加入测试频道，可能收到 RC 等预发布版本。</small>}
    {open && snapshot.available && <UpdateDialog snapshot={snapshot} store={store} manual onClose={() => setOpen(false)} />}
  </div>;
}

export function UpdateBadge({ store = nativeUpdateStore, enabled = supportsDesktopUpdates() }: Props) {
  const snapshot = useUpdate(store, enabled);
  const [open, setOpen] = useState(false);
  const [anchor, setAnchor] = useState<DOMRect>();
  if (!enabled || !snapshot.available) return null;
  return <span className="update-badge-wrap">
    <button type="button" className="update-badge" aria-label="可用更新" title={`可用更新 · v${snapshot.available.version}`} aria-expanded={open} onClick={(e) => { setAnchor(e.currentTarget.getBoundingClientRect()); setOpen((value) => !value); }}><ArrowUpCircle size={17} strokeWidth={1.8} /></button>
    {open && <UpdateDialog snapshot={snapshot} store={store} anchor={anchor} manual={false} onClose={() => setOpen(false)} />}
  </span>;
}
