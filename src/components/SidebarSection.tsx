import { useId, type ReactNode } from "react";
import { ChevronRight, type LucideIcon } from "lucide-react";

export function SidebarDisclosure({ expanded, id, children }: { expanded: boolean; id?: string; children: ReactNode }) {
  return <div id={id} className="sidebar-disclosure" data-expanded={expanded} aria-hidden={!expanded || undefined}
    ref={node => { if (node) node.inert = !expanded; }}>
    <div className="sidebar-disclosure-content">{children}</div>
  </div>;
}

export default function SidebarSection({ label, icon: Icon, expanded, onToggle, action, children }: {
  label: string; icon: LucideIcon; expanded: boolean; onToggle: () => void; action?: ReactNode; children: ReactNode;
}) {
  const id = useId();
  return <section className="sidebar-section">
    <div className="sidebar-section-heading">
      <button type="button" className="nav-item" title={label} aria-expanded={expanded} aria-controls={id} onClick={onToggle}>
        <span className="nav-icon"><Icon size={17} strokeWidth={1.8} aria-hidden="true" /></span>
        <span className="nav-label">{label}</span>
        <ChevronRight className="nav-caret sidebar-disclosure-chevron" data-expanded={expanded} size={14} aria-hidden="true" />
      </button>
      {action}
    </div>
    <SidebarDisclosure id={id} expanded={expanded}>{children}</SidebarDisclosure>
  </section>;
}
