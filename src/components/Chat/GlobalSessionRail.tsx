import type { ReactNode } from "react";
import { MessageCircle, Plus, Radio, SquareTerminal } from "lucide-react";
import type { Session } from "../../services/tauri";
import SessionPicker from "./SessionPicker";

export type GlobalSessionKind = "chat" | "im" | "cli";

interface GlobalSessionRailProps {
  collapsed: boolean;
  activeKind: GlobalSessionKind;
  counts: Record<GlobalSessionKind, number>;
  sessions: Session[];
  activeSessionId: string | null;
  labels: Record<GlobalSessionKind, string> & { conversations: string };
  onKindChange: (kind: GlobalSessionKind) => void;
  onSelect: (id: string) => void;
  onDelete: (event: React.MouseEvent, id: string, title: string) => void;
  onNew: () => void;
  t: (key: string, options?: Record<string, unknown>) => string;
  emptyHint?: string;
  footer?: ReactNode;
}

const KIND_ICONS = {
  chat: MessageCircle,
  im: Radio,
  cli: SquareTerminal,
} as const;

export default function GlobalSessionRail({
  collapsed,
  activeKind,
  counts,
  sessions,
  activeSessionId,
  labels,
  onKindChange,
  onSelect,
  onDelete,
  onNew,
  t,
  emptyHint,
  footer,
}: GlobalSessionRailProps) {
  return (
    <section className={`global-session-rail ${collapsed ? "is-collapsed" : ""}`}>
      <button type="button" className="global-new-chat" onClick={onNew} aria-label={t("chat.newChat")}>
        <Plus size={17} strokeWidth={1.8} aria-hidden="true" />
        {!collapsed && <span>{t("chat.newChat")}</span>}
      </button>

      {!collapsed && <div className="sidebar-section-label">{labels.conversations}</div>}
      <nav className="global-session-filters" aria-label={labels.conversations}>
        {(["chat", "im", "cli"] as GlobalSessionKind[]).map((kind) => {
          const Icon = KIND_ICONS[kind];
          return (
            <button
              key={kind}
              type="button"
              className={`global-session-filter ${activeKind === kind ? "active" : ""}`}
              onClick={() => onKindChange(kind)}
              aria-label={labels[kind]}
              aria-pressed={activeKind === kind}
              title={labels[kind]}
            >
              <Icon size={15} strokeWidth={1.8} aria-hidden="true" />
              {!collapsed && <span>{labels[kind]}</span>}
              {!collapsed && counts[kind] > 0 && <small>{counts[kind]}</small>}
            </button>
          );
        })}
      </nav>

      {!collapsed && (
        <SessionPicker
          embedded
          sessions={sessions}
          activeSessionId={activeSessionId}
          onSelect={onSelect}
          onDelete={onDelete}
          onClose={() => {}}
          t={t}
          allowCreate={false}
          emptyHint={emptyHint}
          footer={footer}
        />
      )}
    </section>
  );
}
