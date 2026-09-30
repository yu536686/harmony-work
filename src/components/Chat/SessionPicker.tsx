import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  Building2,
  Gamepad2,
  Inbox,
  MessageCircle,
  Paperclip,
  Radio,
  Send,
  Smartphone,
  SquareTerminal,
  UserRound,
  Webhook,
  X,
  Zap,
} from "lucide-react";
import type { Session } from "../../services/tauri";

function SourceIcon({ source }: { source: string }) {
  const props = { size: 14, strokeWidth: 1.8, "aria-hidden": true } as const;
  if (source === "chat" || !source) return <UserRound {...props} />;
  if (source === "mobile") return <Smartphone {...props} />;
  if (source === "cli") return <SquareTerminal {...props} />;
  if (source.includes("telegram")) return <Send {...props} />;
  if (source.includes("feishu") || source.includes("lark")) return <Building2 {...props} />;
  if (source.includes("wechat") || source.includes("wecom")) return <MessageCircle {...props} />;
  if (source.includes("dingtalk")) return <Paperclip {...props} />;
  if (source.includes("slack")) return <Zap {...props} />;
  if (source.includes("discord")) return <Gamepad2 {...props} />;
  if (source.includes("teams") || source.includes("matrix")) return <Radio {...props} />;
  if (source.includes("webhook")) return <Webhook {...props} />;
  return <Inbox {...props} />;
}

const DAY_MS = 24 * 60 * 60 * 1000;
const MORE_PAGE = 20;

type Group = "today" | "last7" | "more";

function groupForSession(session: Session, now: number): Group {
  const ts = Date.parse(session.updated_at || session.created_at || "");
  if (Number.isNaN(ts)) return "more";
  const startOfToday = new Date(now);
  startOfToday.setHours(0, 0, 0, 0);
  if (ts >= startOfToday.getTime()) return "today";
  if (ts >= now - 7 * DAY_MS) return "last7";
  return "more";
}

export interface SessionPickerProps {
  sessions: Session[];
  activeSessionId: string | null;
  onSelect: (id: string) => void;
  onDelete: (e: React.MouseEvent, id: string, title: string) => void;
  onNew?: () => void;
  onClose: () => void;
  t: (key: string, options?: Record<string, unknown>) => string;
  footer?: ReactNode;
  /** When false, hide the + button (e.g. Pond CLI sessions are project-bound). */
  allowCreate?: boolean;
  emptyHint?: string;
  embedded?: boolean;
}

export default function SessionPicker({
  sessions,
  activeSessionId,
  onSelect,
  onDelete,
  onNew,
  onClose,
  t,
  footer,
  allowCreate = true,
  emptyHint,
  embedded = false,
}: SessionPickerProps) {
  const [query, setQuery] = useState("");
  const [moreVisible, setMoreVisible] = useState(MORE_PAGE);
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!embedded) inputRef.current?.focus();
  }, [embedded]);

  // Close on outside click / Escape.
  useEffect(() => {
    if (embedded) return;
    const onPointerDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [embedded, onClose]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    const matched = q
      ? sessions.filter((s) => (s.title ?? "").toLowerCase().includes(q))
      : sessions;
    return [...matched].sort((a, b) => {
      const ta = Date.parse(b.updated_at || b.created_at || "") || 0;
      const tb = Date.parse(a.updated_at || a.created_at || "") || 0;
      return ta - tb;
    });
  }, [sessions, query]);

  const { today, last7, more } = useMemo(() => {
    const now = Date.now();
    const buckets: { today: Session[]; last7: Session[]; more: Session[] } = {
      today: [],
      last7: [],
      more: [],
    };
    for (const s of filtered) {
      buckets[groupForSession(s, now)].push(s);
    }
    return buckets;
  }, [filtered]);

  const renderRow = (s: Session) => {
    const title = (s.title ?? t("chat.defaultTitle"))
      .replace(/^🐠\s*/, "")
      .replace(/^Piscis CLI/, "小亨 CLI");
    return (
      <div
        key={s.id}
        className={`session-picker-item ${s.id === activeSessionId ? "active" : ""}`}
        onClick={() => onSelect(s.id)}
      >
        <span className="session-picker-item-icon" aria-hidden="true">
          <SourceIcon source={s.source} />
        </span>
        <span className="session-picker-item-title">{title}</span>
        {s.source === "mobile" && <small className="mobile-source-badge">手机端</small>}
        <span className="session-picker-item-count">{s.message_count}</span>
        <button
          className="session-picker-item-delete"
          title={t("chat.deleteChat")}
          onClick={(e) => onDelete(e, s.id, title)}
        >
          <X size={12} strokeWidth={2} aria-hidden="true" />
        </button>
      </div>
    );
  };

  const renderGroup = (label: string, items: Session[], extra?: ReactNode) => {
    if (items.length === 0) return null;
    return (
      <div className="session-picker-group">
        <div className="session-picker-group-label">{label}</div>
        {items.map(renderRow)}
        {extra}
      </div>
    );
  };

  const moreShown = more.slice(0, moreVisible);
  const moreExtra =
    more.length > moreVisible ? (
      <button
        className="session-picker-loadmore"
        onClick={() => setMoreVisible((n) => n + MORE_PAGE)}
      >
        {t("chat.loadMoreSessions")} ({more.length - moreVisible})
      </button>
    ) : null;

  const isEmpty = filtered.length === 0;

  return (
    <div className={`session-picker ${embedded ? "session-picker--embedded" : ""}`} ref={rootRef} role="dialog">
      <div className="session-picker-header">
        <input
          ref={inputRef}
          className="session-picker-search"
          type="text"
          value={query}
          placeholder={t("chat.searchSessions")}
          onChange={(e) => {
            setQuery(e.target.value);
            setMoreVisible(MORE_PAGE);
          }}
        />
        {allowCreate && onNew && (
          <button className="session-picker-new" onClick={onNew} title={t("chat.newChat")}>
            +
          </button>
        )}
      </div>

      <div className="session-picker-list">
        {isEmpty ? (
          <div className="session-picker-empty">
            {query.trim()
              ? t("chat.noSearchResults")
              : emptyHint ?? t("chat.noChats")}
          </div>
        ) : (
          <>
            {renderGroup(t("chat.sessionGroupToday"), today)}
            {renderGroup(t("chat.sessionGroupLast7Days"), last7)}
            {renderGroup(t("chat.sessionGroupMore"), moreShown, moreExtra)}
          </>
        )}
      </div>

      {footer && <div className="session-picker-footer">{footer}</div>}
    </div>
  );
}
