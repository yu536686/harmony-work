import type { Session } from "../services/tauri";
import { classifyMainChatSession, isInternalSession } from "./session";

export type ChatProjectKind = "workspace" | "im" | "cli";

export interface ChatProject {
  id: string;
  name: string;
  path: string | null;
  kind: ChatProjectKind;
  system: boolean;
  sessions: Session[];
}

interface BuildChatProjectsOptions {
  sessions: Session[];
  defaultWorkspace: string;
  workspaceDirectories: string[];
  labels?: {
    im: string;
    cli: string;
    unassigned: string;
  };
}

const DEFAULT_LABELS = {
  im: "IM 会话",
  cli: "CLI 会话",
  unassigned: "未设置工作区",
};

export function normalizeWorkspacePath(path: string): string {
  const normalized = path.trim().replace(/\\/g, "/").replace(/\/+$/, "");
  return /^[a-z]:/i.test(normalized) ? normalized.toLowerCase() : normalized;
}

export function projectIdForWorkspace(path: string): string {
  return `workspace:${normalizeWorkspacePath(path) || "unassigned"}`;
}

export function projectNameFromPath(path: string, fallback = DEFAULT_LABELS.unassigned): string {
  const normalized = path.trim().replace(/[\\/]+$/, "");
  if (!normalized) return fallback;
  const parts = normalized.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || fallback;
}

export function joinWorkspacePath(root: string, name: string): string {
  const separator = root.includes("\\") || /^[a-z]:/i.test(root) ? "\\" : "/";
  return `${root.replace(/[\\/]+$/, "")}${separator}${name.replace(/^[\\/]+/, "")}`;
}

function newestFirst(a: Session, b: Session): number {
  return new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime();
}

export function buildChatProjects({
  sessions,
  defaultWorkspace,
  workspaceDirectories,
  labels = DEFAULT_LABELS,
}: BuildChatProjectsOptions): ChatProject[] {
  const workspaceProjects = new Map<string, ChatProject>();

  const ensureWorkspace = (path: string): ChatProject => {
    const key = normalizeWorkspacePath(path);
    const id = projectIdForWorkspace(path);
    const existing = workspaceProjects.get(key);
    if (existing) return existing;
    const project: ChatProject = {
      id,
      name: projectNameFromPath(path, labels.unassigned),
      path: path.trim() || null,
      kind: "workspace",
      system: false,
      sessions: [],
    };
    workspaceProjects.set(key, project);
    return project;
  };

  if (defaultWorkspace.trim()) ensureWorkspace(defaultWorkspace);
  for (const directory of workspaceDirectories) {
    if (directory.trim()) ensureWorkspace(directory);
  }

  const imProject: ChatProject = {
    id: "system:im",
    name: labels.im,
    path: null,
    kind: "im",
    system: true,
    sessions: [],
  };
  const cliProject: ChatProject = {
    id: "system:cli",
    name: labels.cli,
    path: null,
    kind: "cli",
    system: true,
    sessions: [],
  };

  for (const item of sessions) {
    if (isInternalSession(item)) continue;
    const kind = classifyMainChatSession(item);
    if (kind === "im") {
      imProject.sessions.push(item);
    } else if (kind === "cli") {
      cliProject.sessions.push(item);
    } else {
      ensureWorkspace(item.workspace_root?.trim() || defaultWorkspace).sessions.push(item);
    }
  }

  for (const project of workspaceProjects.values()) project.sessions.sort(newestFirst);
  imProject.sessions.sort(newestFirst);
  cliProject.sessions.sort(newestFirst);

  return [...workspaceProjects.values(), imProject, cliProject];
}
