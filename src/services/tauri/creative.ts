import { invoke } from "@tauri-apps/api/core";

export type CreativeKind = "music" | "image";
export type CreativeStatus = "queued" | "processing" | "succeeded" | "failed" | "expired";

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
  assetId: string | null;
  expiresInSeconds: number;
}

export interface MusicPromptAssistantRequest {
  businessLine: string;
  brand: string;
  watchCollection: string;
  watchStyle?: string;
  desiredStyle?: string;
  bpm?: number;
  duration: number;
}

export interface MusicPromptAssistantResponse {
  prompt: string;
  model?: string;
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
  if (canvasPreset === "television") return { canvasPreset, providerRatio: "16:9" };
  if (canvasPreset === "vertical") return { canvasPreset, providerRatio: "9:16" };
  if (canvasPreset === "store-poster") return { canvasPreset, outputWidth: 420, outputHeight: 592, providerRatio: "3:4" };
  if (canvasPreset === "phone-fullscreen") return { canvasPreset, outputWidth: 2868, outputHeight: 1320, providerRatio: "16:9" };
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
  createMusic: (request: MusicJobRequest) =>
    invoke<{ job: CreativeJob }>("marketing_creative_create_music", { request }),
  createImage: (request: ImageJobRequest) =>
    invoke<{ job: CreativeJob }>("marketing_creative_create_image", { request }),
  composeMusicPrompt: (request: MusicPromptAssistantRequest) =>
    invoke<MusicPromptAssistantResponse>("marketing_creative_compose_music_prompt", { request }),
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
    })
};
