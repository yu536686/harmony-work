import { StrictMode } from "react";
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { creativeApi, loadHarvisTasks, saveHarvisTasks, type HarvisTaskKind } from "../../services/tauri/creative";
import { useHarvisTasks } from "./useHarvisTasks";
import { marketingApi } from "../../services/tauri/marketing";

beforeEach(() => { localStorage.clear(); localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ id: "test-owner" })); vi.useFakeTimers(); });
afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers(); });

it("hides old-owner history, stops polling signed out and ignores A's late response even after A logs back in", async () => {
  localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ id: "A" }));
  saveHarvisTasks([{ ownerId: "A", taskId: "private-A", kind: "image", prompt: "A private", status: "pending", progress: 0, createdAt: 1 }]);
  let resolve!: (value: Record<string, unknown>) => void;
  const query = vi.spyOn(creativeApi, "harvisQueryTask").mockImplementation(() => new Promise(done => { resolve = done; }));
  const content = vi.spyOn(creativeApi, "harvisQueryTaskContent").mockResolvedValue({ url: "https://cdn.example/A.png" });
  const view = renderHook(() => useHarvisTasks());
  expect(query).toHaveBeenCalledTimes(1);
  await act(async () => { await marketingApi.logout(); });
  expect(view.result.current.tasks).toEqual([]);
  await act(async () => { await vi.advanceTimersByTimeAsync(30_000); });
  expect(query).toHaveBeenCalledTimes(1);
  act(() => {
    localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ id: "B" }));
    window.dispatchEvent(new StorageEvent("storage", { key: "workbuddy.marketingUser" }));
  });
  expect(view.result.current.tasks).toEqual([]);
  expect(query).toHaveBeenCalledTimes(1);
  await act(async () => { resolve({ status: "succeeded" }); });
  expect(content).not.toHaveBeenCalled();
  expect(view.result.current.tasks).toEqual([]);
  act(() => {
    localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ id: "A" }));
    window.dispatchEvent(new StorageEvent("storage", { key: "workbuddy.marketingUser" }));
  });
  expect(view.result.current.tasks[0]).toMatchObject({ taskId: "private-A", status: "pending" });
  expect(query).toHaveBeenCalledTimes(2);
  const second = resolve;
  await act(async () => { second({ status: "succeeded", url: "https://cdn.example/current.png" }); });
  expect(view.result.current.tasks[0].resultUrl).toBe("https://cdn.example/current.png");
});

it("shares requests across the creation page and task center including StrictMode remounts", async () => {
  saveHarvisTasks([{ ownerId: "test-owner", taskId: "shared", kind: "image", prompt: "shared", status: "pending", progress: 0, createdAt: 1 }]);
  let resolve!: (value: Record<string, unknown>) => void;
  const query = vi.spyOn(creativeApi, "harvisQueryTask").mockImplementation(() => new Promise((done) => { resolve = done; }));
  const creation = renderHook(() => useHarvisTasks("image"), { wrapper: StrictMode });
  const center = renderHook(() => useHarvisTasks());
  await act(async () => { await vi.advanceTimersByTimeAsync(30_000); });
  expect(query).toHaveBeenCalledTimes(1);
  await act(async () => { resolve({ status: "succeeded", url: "https://cdn.example/shared.png" }); });
  expect(creation.result.current.tasks[0].status).toBe("succeeded");
  expect(center.result.current.tasks[0].status).toBe("succeeded");
  creation.unmount();
  center.unmount();
  await act(async () => { await vi.advanceTimersByTimeAsync(30_000); });
  expect(query).toHaveBeenCalledTimes(1);
});

it.each(["B", "A"])("does not coalesce a new %s login with A's old request for the same task ID", async id => {
  localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ id: "A" }));
  saveHarvisTasks([{ ownerId: "A", taskId: "same", kind: "image", prompt: "A private", status: "pending", progress: 0, createdAt: 1 }]);
  const completions: Array<(value: Record<string, unknown>) => void> = [];
  const query = vi.spyOn(creativeApi, "harvisQueryTask").mockImplementation(() => new Promise(done => { completions.push(done); }));
  const content = vi.spyOn(creativeApi, "harvisQueryTaskContent");
  const view = renderHook(() => useHarvisTasks());
  await act(async () => { await marketingApi.logout(); });
  act(() => {
    localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ id }));
    window.dispatchEvent(new StorageEvent("storage", { key: "workbuddy.marketingUser" }));
    if (id === "B") saveHarvisTasks([{ ownerId: "B", taskId: "same", kind: "image", prompt: "B private", status: "pending", progress: 0, createdAt: 2 }]);
  });
  expect(query).toHaveBeenCalledTimes(2);
  await act(async () => { completions[0]({ status: "succeeded" }); });
  expect(content).not.toHaveBeenCalled();
  expect(view.result.current.tasks[0]).toMatchObject({ ownerId: id, status: "pending" });
  await act(async () => { await vi.advanceTimersByTimeAsync(16_000); });
  expect(query).toHaveBeenCalledTimes(2);
  await act(async () => { completions[1]({ status: "succeeded", url: `https://cdn.example/${id}.png` }); });
  expect(view.result.current.tasks[0]).toMatchObject({ ownerId: id, resultUrl: `https://cdn.example/${id}.png` });
});

it("does not poll or return legacy unowned tasks even when signed in, or any task while signed out", async () => {
  localStorage.setItem("workbuddy.harvis.tasks", JSON.stringify([{ taskId: "legacy", kind: "image", prompt: "private legacy", status: "pending", progress: 0, createdAt: 1 }]));
  const query = vi.spyOn(creativeApi, "harvisQueryTask");
  const view = renderHook(() => useHarvisTasks());
  expect(view.result.current.tasks).toEqual([]);
  await act(async () => { await vi.advanceTimersByTimeAsync(30_000); await view.result.current.refresh(); });
  expect(query).not.toHaveBeenCalled();
  await act(async () => { await marketingApi.logout(); });
  expect(view.result.current.tasks).toEqual([]);
  await act(async () => { await vi.advanceTimersByTimeAsync(30_000); await view.result.current.refresh(); });
  expect(query).not.toHaveBeenCalled();
  expect(JSON.parse(localStorage.getItem("workbuddy.harvis.tasks")!)[0]).not.toHaveProperty("ownerId");
});

it("discards a result-content response after logout and same-owner re-login", async () => {
  localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ id: "A" }));
  saveHarvisTasks([{ ownerId: "A", taskId: "content", kind: "image", prompt: "A private", status: "pending", progress: 0, createdAt: 1 }]);
  const query = vi.spyOn(creativeApi, "harvisQueryTask").mockResolvedValue({ status: "succeeded" });
  let resolve!: (value: Record<string, unknown>) => void;
  const content = vi.spyOn(creativeApi, "harvisQueryTaskContent").mockImplementation(() => new Promise(done => { resolve = done; }));
  const view = renderHook(() => useHarvisTasks());
  await act(async () => { await Promise.resolve(); });
  expect(content).toHaveBeenCalledTimes(1);
  const oldContent = resolve;
  await act(async () => { await marketingApi.logout(); });
  act(() => {
    localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ id: "A" }));
    window.dispatchEvent(new StorageEvent("storage", { key: "workbuddy.marketingUser" }));
  });
  await act(async () => { await Promise.resolve(); });
  expect(query).toHaveBeenCalledTimes(2);
  const newContent = resolve;
  await act(async () => { oldContent({ url: "https://cdn.example/old-private.png" }); });
  expect(view.result.current.tasks[0].resultUrl).toBeUndefined();
  await act(async () => { newContent({ url: "https://cdn.example/current.png" }); });
  expect(view.result.current.tasks[0].resultUrl).toBe("https://cdn.example/current.png");
});

it("commits late queries only to their original kind after switching pages and unsubscribes on unmount", async () => {
  saveHarvisTasks([
    { ownerId: "test-owner", taskId: "same", kind: "image", prompt: "image", status: "processing", progress: 20, createdAt: 1 },
    { ownerId: "test-owner", taskId: "same", kind: "music", prompt: "music", status: "succeeded", progress: 100, resultUrl: "https://cdn.example/music", adopted: true, rating: 5, createdAt: 2 },
  ]);
  let resolve!: (value: Record<string, unknown>) => void;
  const query = vi.spyOn(creativeApi, "harvisQueryTask").mockImplementation(() => new Promise((done) => { resolve = done; }));
  const view = renderHook(({ kind }) => useHarvisTasks(kind), { initialProps: { kind: "image" as HarvisTaskKind } });
  view.rerender({ kind: "music" });
  view.unmount();
  await act(async () => { resolve({ status: "succeeded", url: "https://cdn.example/image" }); });
  expect(loadHarvisTasks().find((task) => task.kind === "image")).toMatchObject({ status: "succeeded", resultUrl: "https://cdn.example/image" });
  expect(loadHarvisTasks().find((task) => task.kind === "music")).toMatchObject({ status: "succeeded", resultUrl: "https://cdn.example/music", adopted: true, rating: 5 });
  await act(async () => { await vi.advanceTimersByTimeAsync(30_000); });
  expect(query).toHaveBeenCalledTimes(1);
});
