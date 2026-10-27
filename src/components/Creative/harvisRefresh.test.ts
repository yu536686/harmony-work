import { beforeEach, describe, expect, it, vi } from "vitest";
import { creativeApi, loadHarvisTasks, saveHarvisTasks, type HarvisTask } from "../../services/tauri/creative";
import * as refresh from "./harvisRefresh";

const task = (kind: HarvisTask["kind"] = "music"): HarvisTask => ({
  ownerId: "test-owner", taskId: "same-id", kind, prompt: kind, status: "processing", progress: 42, createdAt: 1,
});
const deferred = () => {
  let resolve!: (value: Record<string, unknown>) => void;
  const promise = new Promise<Record<string, unknown>>((done) => { resolve = done; });
  return { promise, resolve };
};

describe("Harvis refresh consistency", () => {
  beforeEach(() => { localStorage.clear(); localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ id: "test-owner" })); vi.restoreAllMocks(); });

  it("serializes duplicate refreshes and keeps feedback edited during the request", async () => {
    saveHarvisTasks([task()]);
    const pending = deferred();
    const query = vi.spyOn(creativeApi, "harvisQueryTask").mockReturnValue(pending.promise);
    const first = refresh.refreshHarvisTasks();
    const second = refresh.refreshHarvisTasks();
    saveHarvisTasks([{ ...task(), adopted: true, rating: 5, feedbackTags: ["brand"] }], "music");
    pending.resolve({ status: "succeeded", url: "https://cdn.example/music.mp3" });
    await Promise.all([first, second]);
    expect(query).toHaveBeenCalledTimes(1);
    expect(loadHarvisTasks()[0]).toMatchObject({ status: "succeeded", progress: 100, adopted: true, rating: 5, feedbackTags: ["brand"] });
  });

  it.each(["succeeded", "failed"] as const)("never overwrites a newer %s terminal state", async (status) => {
    saveHarvisTasks([task()]);
    const pending = deferred();
    const query = vi.spyOn(creativeApi, "harvisQueryTask").mockReturnValue(pending.promise);
    const running = refresh.refreshHarvisTasks();
    saveHarvisTasks([{ ...task(), status, resultUrl: status === "succeeded" ? "https://cdn.example/final.mp3" : undefined, errorMessage: status === "failed" ? "provider failed" : undefined }]);
    pending.resolve({ status: status === "failed" ? "succeeded" : "processing", progress: 7, url: "https://cdn.example/old.mp3" });
    await running;
    expect(query).toHaveBeenCalledTimes(1);
    expect(loadHarvisTasks()[0]).toMatchObject({ status, ...(status === "failed" ? { errorMessage: "provider failed" } : { resultUrl: "https://cdn.example/final.mp3" }) });
  });

  it("does not regress processing status or progress on stale provider snapshots", async () => {
    saveHarvisTasks([task()]);
    const query = vi.spyOn(creativeApi, "harvisQueryTask").mockResolvedValue({ status: "queued", progress: 2 });
    await refresh.refreshHarvisTasks();
    expect(query).toHaveBeenCalledTimes(1);
    expect(loadHarvisTasks()[0]).toMatchObject({ status: "processing", progress: 42 });
  });

  it("persists actual generation failure separately from transport errors", async () => {
    saveHarvisTasks([task()]);
    vi.spyOn(creativeApi, "harvisQueryTask").mockResolvedValue({ status: "failed", error_message: "content rejected" });
    await refresh.refreshHarvisTasks();
    expect(loadHarvisTasks()[0]).toMatchObject({ status: "failed", errorMessage: "content rejected" });
    expect(loadHarvisTasks()[0].queryError).toBeUndefined();
  });

  it("keeps success when result retrieval fails, then recovers the result after restart", async () => {
    saveHarvisTasks([task()]);
    const query = vi.spyOn(creativeApi, "harvisQueryTask").mockResolvedValue({ status: "succeeded" });
    vi.spyOn(creativeApi, "harvisQueryTaskContent").mockRejectedValueOnce(new Error("content offline"));
    await refresh.refreshHarvisTasks();
    expect(loadHarvisTasks()[0]).toMatchObject({ status: "succeeded", queryError: "content offline" });
    query.mockResolvedValue({ status: "succeeded", url: "https://cdn.example/recovered.mp3" });
    await refresh.refreshHarvisTasks(loadHarvisTasks());
    expect(loadHarvisTasks()[0]).toMatchObject({ status: "succeeded", resultUrl: "https://cdn.example/recovered.mp3" });
    expect(loadHarvisTasks()[0].queryError).toBeUndefined();
  });

  it("isolates identical IDs in different kinds and preserves unqueried kinds", async () => {
    saveHarvisTasks([task("music"), task("image"), { ...task("video"), status: "failed" }]);
    vi.spyOn(creativeApi, "harvisQueryTask").mockImplementation(async (_id, kind) => ({ status: "succeeded", url: `https://cdn.example/${kind}` }));
    await refresh.refreshHarvisTasks(loadHarvisTasks().filter((item) => item.kind !== "video"));
    expect(loadHarvisTasks().find((item) => item.kind === "music")?.resultUrl).toBe("https://cdn.example/music");
    expect(loadHarvisTasks().find((item) => item.kind === "image")?.resultUrl).toBe("https://cdn.example/image");
    expect(loadHarvisTasks().find((item) => item.kind === "video")?.status).toBe("failed");
  });

  it("clears legacy query errors from the generation-error field on a new query failure", async () => {
    saveHarvisTasks([{ ...task(), errorMessage: "Error: old connection error" }]);
    vi.spyOn(creativeApi, "harvisQueryTask").mockRejectedValue(new Error("offline again"));
    await refresh.refreshHarvisTasks();
    expect(loadHarvisTasks()[0]).toMatchObject({ status: "processing", queryError: "offline again" });
    expect(loadHarvisTasks()[0].errorMessage).toBeUndefined();
  });

  it("never resurrects deleted tasks when a pending request settles", async () => {
    saveHarvisTasks([task()]);
    const pending = deferred();
    const query = vi.spyOn(creativeApi, "harvisQueryTask").mockReturnValue(pending.promise);
    const running = refresh.refreshHarvisTasks();
    saveHarvisTasks([]);
    pending.resolve({ status: "succeeded", url: "https://cdn.example/deleted.mp3" });
    await running;
    expect(query).toHaveBeenCalledTimes(1);
    expect(loadHarvisTasks()).toEqual([]);
  });
});
