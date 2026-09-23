import { invoke } from "@tauri-apps/api/core";
import { migratePublicServiceUrl } from "../serviceUrls";

export type CreativeKind = "music" | "image";
export type CreativeStatus = "queued" | "submitting" | "submitted" | "processing" | "partial_success" | "succeeded" | "archiving" | "archived" | "failed" | "expired";

export type HarvisTaskKind = "image" | "video" | "music" | "tts";

export interface HarvisTask {
  taskId: string;
  kind: HarvisTaskKind;
  prompt: string;
  status: "pending" | "processing" | "succeeded" | "failed";
  progress: number;
  resultUrl?: string;
  errorMessage?: string;
  createdAt: number;
}

export interface CreativeArtifact {
  id: string;
  kind: "audio" | "image" | "cover";
  assetId: string | null;
  fileName: string;
  mimeType: string;
  byteSize: number | null;
  durationSeconds: number | null;
  previewUrl: string | null;
}

export interface CreativeJob {
  id: string;
  kind: CreativeKind;
  mode: string;
  status: CreativeStatus;
  progress: number;
  progressMessage: string;
  promptSnapshot: string;
  options: Record<string, unknown>;
  errorMessage: string;
  adoptedArtifactId: string | null;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
  artifacts: CreativeArtifact[];
}

export interface MusicJobRequest {
  mode: "free" | "assistant";
  prompt?: string;
  tags: string;
  title: string;
  duration: number;
}

export interface ImageJobRequest {
  mode: "text" | "image";
  prompt: string;
  canvasPreset: "television" | "vertical" | "store-poster" | "phone-fullscreen" | "custom";
  outputWidth?: number;
  outputHeight?: number;
  referenceIds?: string[];
}

export interface CreativeReferenceUpload {
  id: string;
  referenceUrl?: string;
  assetId: string | null;
  expiresInSeconds: number;
}

export interface MusicPromptAssistantRequest {
  businessLine: string;
  brand: string;
  watchCollection: string;
  model?: string;
  theme?: string;
  watchStyle?: string;
  desiredStyle?: string;
  bpm?: number;
  duration: number;
  details?: Record<string, string>;
}

export interface MusicAssistantField {
  key: string;
  label: string;
  type: "select" | "text" | "number" | "asset";
  required: boolean;
  options: string[];
}

export interface MusicAssistantBusinessLine {
  id: string;
  title: string;
  description: string;
  fields: MusicAssistantField[];
}

export interface MusicAssistantConfig {
  businessLines: MusicAssistantBusinessLine[];
  brands: Array<{ id: string; name: string; collections: string[] }>;
}

export interface MusicPromptAssistantResponse {
  prompt: string;
  promptZh?: string;
  model?: string;
  analysisZh?: string;
  confidence?: number;
  degraded?: boolean;
  warnings?: string[];
  evidenceCases?: Array<{
    id: string;
    title: string;
    genre?: string;
  }>;
}

export interface CanvasRequest {
  canvasPreset: ImageJobRequest["canvasPreset"];
  outputWidth?: number;
  outputHeight?: number;
  providerRatio: "16:9" | "9:16" | "3:4" | "1:1";
}

export function resolveCanvasRequest(
  canvasPreset: ImageJobRequest["canvasPreset"],
  width?: number,
  height?: number
): CanvasRequest {
  if (canvasPreset === "television") return { canvasPreset, outputWidth: 1536, outputHeight: 1024, providerRatio: "16:9" };
  if (canvasPreset === "vertical") return { canvasPreset, outputWidth: 1024, outputHeight: 1536, providerRatio: "9:16" };
  if (canvasPreset === "store-poster") return { canvasPreset, outputWidth: 420, outputHeight: 592, providerRatio: "3:4" };
  if (canvasPreset === "phone-fullscreen") return { canvasPreset, outputWidth: 1536, outputHeight: 1024, providerRatio: "16:9" };
  if (!width || !height) throw new Error("自定义比例需要填写宽度和高度");
  const ratio = width / height;
  return {
    canvasPreset,
    outputWidth: width,
    outputHeight: height,
    providerRatio: ratio >= 1.45 ? "16:9" : ratio <= 0.65 ? "9:16" : ratio < 0.9 ? "3:4" : "1:1"
  };
}

export function normalizeReferenceSelection(ids: string[]) {
  const unique = ids.filter((id, index) => id.trim().length > 0 && ids.indexOf(id) === index);
  return unique.slice(-4);
}

async function encodeFile(file: File) {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  for (let index = 0; index < bytes.length; index += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
  }
  return btoa(binary);
}

export const creativeApi = {
  listJobs: () => invoke<{ jobs: CreativeJob[] }>("marketing_creative_jobs"),
  getJob: (jobId: string) => invoke<{ job: CreativeJob }>("marketing_creative_job", { jobId }),
  musicConfig: () => invoke<MusicAssistantConfig>("marketing_creative_music_config"),
  createMusic: (request: MusicJobRequest) =>
    invoke<{ job: CreativeJob }>("marketing_creative_create_music", { request }),
  createImage: (request: ImageJobRequest) =>
    invoke<{ job: CreativeJob }>("marketing_creative_create_image", { request }),
  composeMusicPrompt: (request: MusicPromptAssistantRequest) =>
    invoke<MusicPromptAssistantResponse>("marketing_creative_compose_music_prompt", { request }),
  adoptMusicArtifact: (jobId: string, artifactId: string) =>
    invoke<{ jobId: string; artifactId: string }>("marketing_creative_adopt_music_artifact", { jobId, artifactId }),
  saveMusicFeedback: (artifactId: string, rating: number, reasonTags: string[], comment: string, submitToKnowledge: boolean) =>
    invoke<{ artifactId: string; rating: number; candidateId: string | null }>("marketing_creative_music_feedback", {
      artifactId,
      rating,
      reasonTags,
      comment: comment || null,
      submitToKnowledge
    }),
  saveMusicBrand: (name: string, collections: string[]) =>
    invoke<{ id: string; name: string; collections: string[] }>("marketing_creative_admin_upsert_music_brand", {
      request: { name, collections }
    }),
  saveMusicBusinessLine: (request: { id: string; title: string; description?: string; fields: MusicAssistantField[] }) =>
    invoke<{ id: string; title: string }>("marketing_creative_admin_upsert_music_business_line", { request }),
  stageForChat: (jobId: string) =>
    invoke<{ job: CreativeJob; assetIds: string[] }>("marketing_creative_stage_for_chat", { jobId }),
  saveToWorkspace: (jobId: string, workspaceRoot?: string | null) =>
    invoke<{ job: CreativeJob | null; saved: Array<{ localPath: string; fileName: string; mediaType: string }> }>("marketing_creative_save_to_workspace", {
      jobId,
      workspaceRoot: workspaceRoot || null
    }),
  saveAsset: (assetId: string, fileName: string, targetPath: string) =>
    invoke<{ localPath: string; fileName: string; mediaType: string }>("marketing_save_asset", {
      request: { assetId, fileName, targetPath }
    }),
  uploadReference: async (file: File) =>
    invoke<CreativeReferenceUpload>("marketing_creative_upload_reference", {
      request: {
        fileName: file.name,
        mediaType: file.type || "application/octet-stream",
        dataBase64: await encodeFile(file)
      }
    }),
  harvisGenerate: (request: Record<string, unknown>) =>
    invoke<Record<string, unknown>>("harvis_generate", { request }),
  harvisQueryTask: (taskId: string, kind: HarvisTaskKind) =>
    invoke<Record<string, unknown>>("harvis_query_task", { taskId, kind }),
  harvisQueryTaskContent: (taskId: string, kind: HarvisTaskKind) =>
    invoke<Record<string, unknown>>("harvis_query_task_content", { taskId, kind }),
  harvisSaveResult: (resultUrl: string, targetPath: string) =>
    invoke<{ localPath: string; sizeBytes: number }>("harvis_save_result", { resultUrl, targetPath }),
  harvisPreviewResult: (resultUrl: string, kind: "image" | "music") =>
    invoke<{ dataUrl: string }>("harvis_preview_result", { resultUrl, kind }),
};

const HARVIS_TASKS_KEY = "workbuddy.harvis.tasks";

export function loadHarvisTasks(): HarvisTask[] {
  try {
    const raw = localStorage.getItem(HARVIS_TASKS_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? (parsed as HarvisTask[]).map(task => (
      task && typeof task.resultUrl === "string"
        ? { ...task, resultUrl: migratePublicServiceUrl(task.resultUrl) }
        : task
    )) : [];
  } catch {
    return [];
  }
}

export function saveHarvisTasks(tasks: HarvisTask[]): void {
  try {
    localStorage.setItem(HARVIS_TASKS_KEY, JSON.stringify(tasks));
  } catch {
    // ignore quota / serialization errors
  }
}
