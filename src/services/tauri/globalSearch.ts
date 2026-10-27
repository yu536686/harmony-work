import { invoke } from "@tauri-apps/api/core";
import type { Session } from "./chat";
import { SETTINGS_SECTIONS, type SettingsSection } from "../../components/Settings/settingsSections";
import { isLegacyInternalProjectPath, normalizeWorkspacePath, projectNameFromPath } from "../../utils/chatProjects";
import { isInternalSession } from "../../utils/session";

export const MAX_SEARCH_QUERY = 128;
export const MAX_SEARCH_RESULTS = 40;
export interface SearchSession extends Session {
  pinned_at?: string | null;
  archived_at?: string | null;
  pool_session_id?: string | null;
  policy_mode?: string | null;
  allow_outside_workspace?: boolean | null;
}
export type CommandPaletteTarget =
  | { type: "session"; session: SearchSession; artifactId?: string }
  | { type: "workspace"; workspaceRoot: string; session?: SearchSession }
  | { type: "settings"; settingsSection: SettingsSection };

export interface GlobalSearchItem {
  id: string;
  kind: "session" | "artifact" | "workspace" | "settings";
  title: string;
  source: string;
  session: SearchSession | null;
  artifactId: string | null;
  workspaceRoot: string | null;
  settingsSection?: SettingsSection;
  local?: boolean;
}
export interface GlobalSearchResponse { results: GlobalSearchItem[]; hasMore: boolean }
const PROJECTS_KEY = "workbuddy.chatProjectDirectories.v3";

export function localSearchResults(query: string): GlobalSearchItem[] {
  const needle = query.trim().toLocaleLowerCase();
  const results: GlobalSearchItem[] = SETTINGS_SECTIONS.filter(section =>
    [section.id, section.label, ...section.fieldIds, ...section.legacyRoutes].some(value => value.toLocaleLowerCase().includes(needle)),
  ).map(section => ({ id: `settings:${section.id}`, kind: "settings", title: section.label, source: "settings", session: null, artifactId: null, workspaceRoot: null, settingsSection: section.id, local: true }));
  try {
    const raw = localStorage.getItem(PROJECTS_KEY) ?? "[]";
    if (raw.length > 64 * 1024) return results;
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return results;
    const seen = new Set<string>();
    for (const path of parsed.slice(0, 200)) {
      if (typeof path !== "string" || !path.trim() || path.length > 4096 || path.includes("\0") || isLegacyInternalProjectPath(path) || !path.toLocaleLowerCase().includes(needle)) continue;
      const key = normalizeWorkspacePath(path);
      if (seen.has(key)) continue;
      seen.add(key);
      results.push({ id: `workspace:${key}`, kind: "workspace", title: projectNameFromPath(path), source: "workspace", session: null, artifactId: null, workspaceRoot: path, local: true });
    }
  } catch { /* Corrupt optional project preferences do not hide real DB search. */ }
  return results.slice(0, MAX_SEARCH_RESULTS);
}

export function mergeSearchResults(query: string, remote: GlobalSearchResponse): GlobalSearchResponse {
  const local = localSearchResults(query);
  const settings = local.filter(result => result.kind === "settings");
  const all = [...settings, ...remote.results.slice(0, MAX_SEARCH_RESULTS), ...local.filter(result => result.kind !== "settings")];
  const seen = new Set<string>();
  const results: GlobalSearchItem[] = [];
  for (const result of all) {
    if (result.session && (isInternalSession(result.session) || result.session.source === "mobile_executor" || result.session.archived_at)) continue;
    const key = result.kind === "workspace" ? `workspace:${normalizeWorkspacePath(result.workspaceRoot ?? "")}` : result.id;
    if (seen.has(key)) continue;
    seen.add(key);
    results.push(result.kind === "workspace" && result.workspaceRoot ? { ...result, title: projectNameFromPath(result.workspaceRoot) } : result);
  }
  return { results: results.slice(0, MAX_SEARCH_RESULTS), hasMore: remote.hasMore || results.length > MAX_SEARCH_RESULTS };
}

export const globalSearchApi = {
  async search(query: string): Promise<GlobalSearchResponse> {
    if (Array.from(query).length > MAX_SEARCH_QUERY || query.includes("\0")) throw new Error("查询最多 128 个字符，且不能包含 NUL");
    const response = await invoke<GlobalSearchResponse>("global_search", { query: query.trim(), limit: 32 });
    return mergeSearchResults(query, response);
  },
};

export function searchTarget(result: GlobalSearchItem): CommandPaletteTarget {
  if (result.kind === "settings" && result.settingsSection) return { type: "settings", settingsSection: result.settingsSection };
  if (result.kind === "workspace" && result.workspaceRoot) return { type: "workspace", workspaceRoot: result.workspaceRoot, ...(result.session ? { session: result.session } : {}) };
  if (result.session && result.kind === "artifact" && result.artifactId) return { type: "session", session: result.session, artifactId: result.artifactId };
  if (result.session && result.kind === "session") return { type: "session", session: result.session };
  throw new Error("搜索结果缺少会话归属，请重新搜索");
}
