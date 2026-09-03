import { invoke } from "@tauri-apps/api/core";

export interface HarmonyCutHealth {
  installed: boolean;
  serverRunning: boolean;
  root: string;
  serverUrl: string;
  message: string;
}

export interface HarmonyCutPrepareRequest {
  mediaDir: string;
  title?: string;
  requirement?: string;
  autoRotateLandscape?: boolean;
}

export interface HarmonyCutPrepareResult {
  projectId: string;
  reviewUrl: string;
  workbuddyUrl: string;
  markerFiles: unknown;
  nextInstruction: string;
  project: Record<string, unknown>;
}

export interface HarmonyCutContinueRequest {
  projectId?: string;
  skipPreview?: boolean;
  skipDraft?: boolean;
}

export interface HarmonyCutContinueResult {
  projectId: string;
  preview: unknown;
  draft: unknown;
  markerFiles: unknown;
  project: Record<string, unknown>;
}

export interface HarmonyCutProjectSummary {
  id: string;
  title: string;
  mediaDir: string;
  workflowStatus: string;
  reviewUrl: string;
}

export const harmonyCutApi = {
  health: () => invoke<HarmonyCutHealth>("harmony_cut_health"),
  startService: () => invoke<HarmonyCutHealth>("harmony_cut_start_service"),
  prepare: (request: HarmonyCutPrepareRequest) =>
    invoke<HarmonyCutPrepareResult>("harmony_cut_prepare", { request }),
  continue: (request: HarmonyCutContinueRequest) =>
    invoke<HarmonyCutContinueResult>("harmony_cut_continue", { request }),
  projects: () => invoke<HarmonyCutProjectSummary[]>("harmony_cut_projects"),
  project: (projectId: string) =>
    invoke<Record<string, unknown>>("harmony_cut_project", { projectId }),
};

export function harmonyCutWorkbenchUrl(projectId?: string | null, serverUrl = "http://127.0.0.1:8765") {
  const base = serverUrl.replace(/\/$/, "");
  return projectId ? `${base}/?project=${encodeURIComponent(projectId)}` : base;
}

export function parseHarmonyCutProjectId(url: string | null | undefined): string | null {
  if (!url) return null;
  try {
    const parsed = new URL(url, window.location.origin);
    return parsed.searchParams.get("project_id") || parsed.searchParams.get("project");
  } catch {
    return null;
  }
}
