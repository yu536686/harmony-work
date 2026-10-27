import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { File, Folder, LoaderCircle, MessageSquare, RefreshCw, Search, Settings2, X } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { globalSearchApi, localSearchResults, MAX_SEARCH_QUERY, searchTarget, type CommandPaletteTarget, type GlobalSearchItem } from "../../services/tauri/globalSearch";
import "./style.css";

export type { CommandPaletteTarget, SearchSession } from "../../services/tauri/globalSearch";
export interface CommandPaletteProps { open: boolean; onClose: () => void; onNavigate: (target: CommandPaletteTarget) => void | Promise<void> }
const names = { session: "会话", artifact: "成果", workspace: "项目", settings: "设置" };
const icons = { session: MessageSquare, artifact: File, workspace: Folder, settings: Settings2 };
const sources: Record<string, string> = { chat: "本地对话", im_feishu: "飞书", im_wecom: "企业微信", im_telegram: "Telegram", cli: "CLI", mobile: "当前手机账号", settings: "设置", workspace: "本地工作区" };

function PaletteDialog({ onClose, onNavigate }: Omit<CommandPaletteProps, "open">) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<GlobalSearchItem[]>([]);
  const [active, setActive] = useState(0);
  const [revision, setRevision] = useState(0);
  const [loading, setLoading] = useState(true);
  const [navigating, setNavigating] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [error, setError] = useState("");
  const input = useRef<HTMLInputElement>(null);
  const dialog = useRef<HTMLDivElement>(null);
  const requests = useRef(0);
  const navigation = useRef(0);
  const alive = useRef(true);
  const listId = useId();
  const titleId = useId();

  useEffect(() => {
    alive.current = true;
    const previous = document.activeElement;
    input.current?.focus();
    return () => {
      alive.current = false; ++requests.current; ++navigation.current;
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, []);

  useEffect(() => {
    const request = ++requests.current;
    ++navigation.current;
    setResults(localSearchResults(query)); setError(""); setActive(0); setLoading(true); setNavigating(false); setHasMore(false);
    const timer = window.setTimeout(() => {
      void globalSearchApi.search(query).then(response => {
        if (alive.current && request === requests.current) { setResults(response.results); setHasMore(response.hasMore); }
      }).catch(error => {
        if (alive.current && request === requests.current) setError(`本地搜索不可用：${error instanceof Error ? error.message : String(error)}。请重新搜索；若命令缺失，请更新客户端。`);
      }).finally(() => { if (alive.current && request === requests.current) setLoading(false); });
    }, 120);
    return () => { window.clearTimeout(timer); ++requests.current; ++navigation.current; };
  }, [query, revision]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const refresh = () => {
      ++requests.current; ++navigation.current;
      setResults([]); setNavigating(false); setRevision(value => value + 1);
    };
    void listen("mobile_bridge_updated", refresh).then(stop => { if (disposed) stop(); else unlisten = stop; }).catch(() => {});
    window.addEventListener("focus", refresh);
    window.addEventListener("storage", refresh);
    return () => { disposed = true; unlisten?.(); window.removeEventListener("focus", refresh); window.removeEventListener("storage", refresh); };
  }, []);

  useEffect(() => {
    document.getElementById(`${listId}-${active}`)?.scrollIntoView?.({ block: "nearest" });
  }, [active, listId]);

  async function navigate(result: GlobalSearchItem) {
    if (navigating) return;
    const request = ++navigation.current;
    setNavigating(true); setError("");
    try {
      // Account switches/deletions can invalidate a displayed row. Re-read the
      // bounded local search before forwarding any session/artifact metadata.
      const current = result.local ? localSearchResults(query) : (await globalSearchApi.search(query)).results;
      const fresh = current.find(item => item.id === result.id);
      if (!fresh) throw new Error("结果已变化，请重新搜索");
      if (alive.current && request === navigation.current) {
        await onNavigate(searchTarget(fresh));
        if (alive.current && request === navigation.current) onClose();
      }
    } catch (error) {
      if (alive.current && request === navigation.current) setError(error instanceof Error ? error.message : String(error));
    } finally { if (alive.current && request === navigation.current) setNavigating(false); }
  }

  return createPortal(<div className="command-palette-backdrop" onMouseDown={event => { if (event.target === event.currentTarget) onClose(); }}>
    <div ref={dialog} className="command-palette" role="dialog" aria-modal="true" aria-labelledby={titleId} onKeyDown={event => {
      if (event.key === "Escape") { event.preventDefault(); onClose(); }
      if ((event.key === "ArrowDown" || event.key === "ArrowUp") && results.length) {
        event.preventDefault(); setActive(value => (value + (event.key === "ArrowDown" ? 1 : -1) + results.length) % results.length);
      }
      if (event.key === "Enter" && !event.nativeEvent.isComposing && document.activeElement === input.current && results[active]) { event.preventDefault(); void navigate(results[active]); }
      if (event.key === "Tab") {
        const focusable = dialog.current?.querySelectorAll<HTMLElement>('input:not(:disabled),button:not(:disabled):not([tabindex="-1"])');
        if (!focusable?.length) return;
        const next = event.shiftKey ? focusable[focusable.length - 1] : focusable[0];
        if ((!event.shiftKey && document.activeElement === focusable[focusable.length - 1]) || (event.shiftKey && document.activeElement === focusable[0])) { event.preventDefault(); next.focus(); }
      }
    }}>
      <h2 id={titleId}>搜索与命令</h2>
      <div className="command-palette-search">
        <Search size={19} aria-hidden="true" />
        <input ref={input} role="combobox" aria-label="全局搜索" aria-expanded="true" aria-controls={listId} aria-autocomplete="list" aria-activedescendant={results[active] ? `${listId}-${active}` : undefined} autoComplete="off" placeholder="搜索..." maxLength={MAX_SEARCH_QUERY} value={query} disabled={navigating} onChange={event => setQuery(Array.from(event.target.value).slice(0, MAX_SEARCH_QUERY).join(""))} />
        {(loading || navigating) && <LoaderCircle className="command-palette-spinner" size={17} aria-label="搜索中" />}
        <button title="重新搜索" aria-label="重新搜索" disabled={navigating} onClick={() => setRevision(value => value + 1)}><RefreshCw size={16} /></button>
        <button title="关闭搜索" aria-label="关闭搜索" onClick={onClose}><X size={17} /></button>
      </div>
      {error && <div className="command-palette-error" role="alert">{error}</div>}
      <div id={listId} className="command-palette-results" role="listbox" aria-label="全局搜索结果" aria-busy={loading || navigating}>
        {results.map((result, index) => {
          const Icon = icons[result.kind];
          return <button id={`${listId}-${index}`} role="option" key={result.id} tabIndex={-1} aria-selected={active === index} disabled={navigating} onMouseEnter={() => setActive(index)} onClick={() => void navigate(result)}>
            <Icon size={17} aria-hidden="true" />
            <span className="command-palette-content"><span className="command-palette-title"><span className={`command-palette-kind ${result.kind}`}>{names[result.kind]}</span>{result.title}</span>
              <span className="command-palette-meta">{sources[result.source] ?? result.source}{result.session && result.kind !== "session" && ` · ${result.session.title || "未命名会话"}`}{result.workspaceRoot && ` · ${result.workspaceRoot}`}</span>
            </span>
          </button>;
        })}
        {results.length === 0 && <p className="command-palette-empty">{loading ? "搜索中..." : error ? "本地结果未加载" : "没有匹配的结果"}</p>}
      </div>
      {hasMore && <p className="command-palette-more" role="status">结果已达上限，请细化查询</p>}
    </div>
  </div>, document.body);
}

/** The inner dialog is destroyed on close: no account/session result cache. */
export default function CommandPalette({ open, ...props }: CommandPaletteProps) {
  return open ? <PaletteDialog {...props} /> : null;
}
