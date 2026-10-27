import { creativeApi, loadHarvisTasks, saveHarvisTasks, type HarvisTask } from "../../services/tauri";
import { migratePublicServiceUrl } from "../../services/serviceUrls";
import { captureMarketingSession, isCurrentMarketingSession, type MarketingSessionSnapshot } from "../../services/tauri/marketing";

export function normalizeHarvisStatus(raw: unknown): HarvisTask["status"] {
  const value = typeof raw === "string" ? raw.toLowerCase() : "";
  if (["succeeded", "success", "completed", "complete", "done"].includes(value)) return "succeeded";
  if (["incomplete", "cancelled", "canceled"].includes(value) || value.includes("fail") || value.includes("error")) return "failed";
  if (["queued", "pending", "submitted"].includes(value)) return "pending";
  return "processing";
}

export function extractHarvisResultUrl(raw: unknown): string | undefined {
  if (!raw || typeof raw !== "object") return undefined;
  const obj = raw as Record<string, unknown>;
  for (const value of [obj.url, obj.resultUrl, obj.result_url, obj.downloadUrl, obj.audioUrl, obj.audio_url, obj.imageUrl, obj.image_url]) {
    if (typeof value === "string" && value.trim()) return migratePublicServiceUrl(value.trim());
  }
  const content = Array.isArray(obj.content) ? obj.content : [obj.content];
  for (const item of content) {
    const url = extractHarvisResultUrl(item);
    if (url) return url;
  }
  return undefined;
}

export function needsHarvisRefresh(task: HarvisTask): boolean {
  return task.status === "pending" || task.status === "processing" || (task.status === "succeeded" && !task.resultUrl);
}

type PollUpdate = Pick<Partial<HarvisTask>, "status" | "progress" | "resultUrl" | "errorMessage" | "queryError">;
const inFlight = new Map<string, Promise<void>>();

function persistUpdate(task: HarvisTask, update: PollUpdate, session: MarketingSessionSnapshot) {
  if (!isCurrentMarketingSession(session)) return;
  const latest = loadHarvisTasks(session);
  const next = latest.map((current) => {
    if (current.taskId !== task.taskId || current.kind !== task.kind) return current;
    // Read at commit time: remote snapshots must never replace user feedback or terminal results.
    if (current.status === "failed" || (current.status === "succeeded" && current.resultUrl)) return current;
    if (current.status === "succeeded" && update.status && update.status !== "succeeded") return current;
    const status = current.status === "processing" && update.status === "pending" ? current.status : update.status ?? current.status;
    return { ...current, ...update, status, progress: Math.max(current.progress, update.progress ?? 0) };
  });
  saveHarvisTasks(next, task.kind, session);
}

async function queryTask(task: HarvisTask, session: MarketingSessionSnapshot): Promise<void> {
  try {
    const response = await creativeApi.harvisQueryTask(task.taskId, task.kind);
    if (!isCurrentMarketingSession(session)) return;
    const status = normalizeHarvisStatus(response?.status ?? response?.state ?? response);
    if (status === "failed") {
      persistUpdate(task, { status, errorMessage: String(response.error_message || response.errorMessage || "生成任务失败"), queryError: undefined }, session);
      return;
    }
    if (status === "succeeded") {
      let resultUrl = extractHarvisResultUrl(response);
      persistUpdate(task, { status, progress: 100, resultUrl, errorMessage: undefined, queryError: undefined }, session);
      if (!resultUrl) {
        if (!isCurrentMarketingSession(session)) return;
        resultUrl = extractHarvisResultUrl(await creativeApi.harvisQueryTaskContent(task.taskId, task.kind));
        persistUpdate(task, { status, resultUrl, queryError: resultUrl ? undefined : "任务已完成，成品暂不可用，请稍后刷新" }, session);
      }
      return;
    }
    const progress = typeof response.progress === "number" && Number.isFinite(response.progress) ? Math.max(0, Math.min(99, response.progress)) : undefined;
    persistUpdate(task, { status, progress, queryError: undefined, errorMessage: undefined }, session);
  } catch (error) {
    persistUpdate(task, { queryError: error instanceof Error ? error.message : String(error), errorMessage: undefined }, session);
  }
}

export async function refreshHarvisTasks(tasks: HarvisTask[] = loadHarvisTasks(), session = captureMarketingSession()): Promise<HarvisTask[]> {
  if (!session.userId || !isCurrentMarketingSession(session)) return [];
  await Promise.all(tasks.map((snapshot) => {
    if (snapshot.ownerId !== session.userId) return;
    const task = loadHarvisTasks(session).find((item) => item.taskId === snapshot.taskId && item.kind === snapshot.kind);
    if (!task || !needsHarvisRefresh(task)) return;
    const key = JSON.stringify([session.userId, session.generation, task.kind, task.taskId]);
    const existing = inFlight.get(key);
    if (existing) return existing;
    const request = queryTask(task, session).finally(() => { inFlight.delete(key); });
    inFlight.set(key, request);
    return request;
  }));
  return loadHarvisTasks(session);
}
