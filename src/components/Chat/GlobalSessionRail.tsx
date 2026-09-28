import { useEffect, useState } from "react";
import { ChevronDown, ChevronRight, CircleAlert, Folder, FolderPlus, LoaderCircle, Trash2 } from "lucide-react";
import type { ChatProject } from "../../utils/chatProjects";

interface GlobalSessionRailProps {
  collapsed: boolean;
  projects: ChatProject[];
  activeProjectId: string | null;
  activeSessionId: string | null;
  runningBySession?: Record<string, boolean>;
  outcomesBySession?: Record<string, "unread" | "error">;
  labels: { projects: string; createProject: string; emptyProject: string; removeProject?: string };
  onProjectSelect: (project: ChatProject) => void;
  onSelect: (id: string) => void;
  onDelete: (event: React.MouseEvent, id: string, title: string) => void;
  onRemoveProject: (project: ChatProject) => void;
  onCreateProject: () => void;
  t: (key: string, options?: Record<string, unknown>) => string;
  imFooter?: React.ReactNode;
}

export default function GlobalSessionRail({
  collapsed,
  projects,
  activeProjectId,
  activeSessionId,
  runningBySession = {},
  outcomesBySession = {},
  labels,
  onProjectSelect,
  onSelect,
  onDelete,
  onRemoveProject,
  onCreateProject,
  t,
  imFooter,
}: GlobalSessionRailProps) {
  const [expanded, setExpanded] = useState<Set<string>>(
    () => new Set(projects.filter((project) => project.sessions.length > 0).map((project) => project.id)),
  );

  useEffect(() => {
    if (!activeProjectId) return;
    setExpanded((current) => {
      if (current.has(activeProjectId)) return current;
      const next = new Set(current);
      next.add(activeProjectId);
      return next;
    });
  }, [activeProjectId]);

  if (collapsed) return null;

  const toggleProject = (project: ChatProject) => {
    if (project.id !== activeProjectId) onProjectSelect(project);
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(project.id)) next.delete(project.id);
      else next.add(project.id);
      return next;
    });
  };

  return (
    <section className="global-session-rail">
      <div className="project-rail-heading">
        <span>{labels.projects}</span>
        <button type="button" onClick={onCreateProject} aria-label={labels.createProject} title={labels.createProject}>
          <FolderPlus size={15} strokeWidth={1.8} aria-hidden="true" />
        </button>
      </div>
      <div className="project-tree" role="tree" aria-label={labels.projects}>
        {projects.map((project) => {
          const isExpanded = expanded.has(project.id);
          const isActive = project.id === activeProjectId;
          return (
            <div key={project.id} className="project-tree-group" role="treeitem" aria-expanded={isExpanded}>
              <div className="project-tree-folder-row">
                <button
                  type="button"
                  className={`project-tree-folder ${isActive ? "active" : ""}`}
                  onClick={() => toggleProject(project)}
                  title={project.path ?? project.name}
                >
                  {isExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                  <Folder size={15} strokeWidth={1.8} />
                  <span>{project.name}</span>
                  {project.sessions.length > 0 && <small>{project.sessions.length}</small>}
                </button>
                {!project.system && project.path && (
                  <button
                    type="button"
                    className="project-tree-remove"
                    aria-label={labels.removeProject ?? "Remove project"}
                    title={labels.removeProject ?? "Remove project"}
                    onClick={(event) => {
                      event.stopPropagation();
                      onRemoveProject(project);
                    }}
                  >
                    <Trash2 size={12} strokeWidth={1.8} />
                  </button>
                )}
              </div>
              {isExpanded && (
                <div className="project-tree-children" role="group">
                  {project.sessions.map((session) => (
                    <div key={session.id} className={`project-session-row ${session.id === activeSessionId ? "active" : ""}`}>
                      <button
                        type="button"
                        className="project-session-select"
                        onClick={() => onSelect(session.id)}
                        title={session.title}
                        aria-label={`${session.title || t("chat.defaultTitle")}${runningBySession[session.id] || (session.status === "running" && !outcomesBySession[session.id]) ? "，正在执行" : outcomesBySession[session.id] === "error" ? "，执行出错" : outcomesBySession[session.id] === "unread" ? "，有未读结果" : ""}`}
                      >
                        <span className="project-session-indicator" aria-hidden="true">
                          {runningBySession[session.id] || (session.status === "running" && !outcomesBySession[session.id]) ? (
                            <LoaderCircle className="project-session-spinner" size={12} strokeWidth={2.2} />
                          ) : outcomesBySession[session.id] === "error" ? (
                            <CircleAlert className="project-session-error" size={12} strokeWidth={2.2} />
                          ) : outcomesBySession[session.id] === "unread" ? (
                            <span className="project-session-unread" />
                          ) : null}
                        </span>
                        <span>{session.title || t("chat.defaultTitle")}</span>
                      </button>
                      <button
                        type="button"
                        className="project-session-delete"
                        aria-label={t("chat.deleteChat")}
                        title={t("chat.deleteChat")}
                        onClick={(event) => onDelete(event, session.id, session.title || t("chat.defaultTitle"))}
                      >
                        <Trash2 size={12} strokeWidth={1.8} />
                      </button>
                    </div>
                  ))}
                  {project.sessions.length === 0 && <div className="project-tree-empty">{labels.emptyProject}</div>}
                  {project.kind === "im" && imFooter}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </section>
  );
}
