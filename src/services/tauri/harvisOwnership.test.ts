import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { addHarvisTask, loadHarvisTasks, saveHarvisTasks } from "./creative";
import { captureMarketingSession, isCurrentMarketingSession, marketingApi } from "./marketing";

const key = "workbuddy.harvis.tasks";
const task = (taskId: string, ownerId?: string) => ({ taskId, ownerId, kind: "image" as const, prompt: taskId, status: "pending" as const, progress: 0, createdAt: 1 });
const switchUser = (id: string | null) => {
  if (id) localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ id }));
  else localStorage.removeItem("workbuddy.marketingUser");
  window.dispatchEvent(new StorageEvent("storage", { key: "workbuddy.marketingUser" }));
};
beforeEach(() => { localStorage.clear(); vi.mocked(invoke).mockReset(); });

it("hides legacy unowned rows without adopting, deleting, or changing their URLs during owned writes", () => {
  const legacy = { ...task("legacy-private"), resultUrl: "https://api.harmonywatch.cn:13001/private.png?token=old" };
  localStorage.setItem(key, JSON.stringify([legacy, task("A-private", "A"), task("B-private", "B")]));
  switchUser("A");
  expect(loadHarvisTasks().map(item => item.taskId)).toEqual(["A-private"]);
  saveHarvisTasks([legacy, task("A-new", "A")], "image");
  const raw = JSON.parse(localStorage.getItem(key)!);
  expect(raw).toEqual(expect.arrayContaining([legacy, task("B-private", "B"), { ...task("A-new"), ownerId: "A" }]));
  switchUser(null);
  expect(loadHarvisTasks()).toEqual([]);
  addHarvisTask(task("signed-out"));
  expect(localStorage.getItem(key)).toBe(JSON.stringify(raw));
  switchUser("B");
  expect(loadHarvisTasks().map(item => item.taskId)).toEqual(["B-private"]);
  switchUser("A");
  expect(loadHarvisTasks().map(item => item.taskId)).toEqual(["A-new"]);
});

it("does not return a generation response received after logout and another account login", async () => {
  switchUser("A");
  let resolve!: (result: unknown) => void;
  vi.mocked(invoke).mockImplementation(command => command === "harvis_generate" ? new Promise(done => { resolve = done; }) : Promise.resolve(undefined));
  const { creativeApi } = await import("./creative");
  const result = creativeApi.harvisGenerate({ kind: "image", prompt: "A private" });
  const rejected = expect(result).rejects.toThrow(/账户|登录/);
  await marketingApi.logout();
  switchUser("B");
  resolve({ task_id: "late-A" });
  await rejected;
  expect(loadHarvisTasks()).toEqual([]);
});

it("captures a frozen owner generation and rejects writes after logout and re-login to the same ID", async () => {
  switchUser("A");
  const owner = captureMarketingSession();
  expect(Object.isFrozen(owner)).toBe(true);
  addHarvisTask(task("original"), owner);
  await marketingApi.logout();
  switchUser("A");
  expect(isCurrentMarketingSession(owner)).toBe(false);
  addHarvisTask(task("late"), owner);
  saveHarvisTasks([], "image", owner);
  expect(loadHarvisTasks().map(item => item.taskId)).toEqual(["original"]);
});

it("does not restore an old me response after a cross-window identity change", async () => {
  switchUser("A");
  let resolve!: (result: unknown) => void;
  vi.mocked(invoke).mockImplementation(() => new Promise(done => { resolve = done; }));
  const response = marketingApi.me();
  const rejected = expect(response).rejects.toThrow(/账户|登录/);
  switchUser("B");
  resolve({ user: { id: "A" } });
  await rejected;
  expect(JSON.parse(localStorage.getItem("workbuddy.marketingUser")!).id).toBe("B");
});
