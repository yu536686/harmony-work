import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { saveHarvisTasks } from "../services/tauri/creative";
import type { Session, ScheduledTask } from "../services/tauri/chat";
import { aggregateTasks, filterTasks, loadTaskSources, type TaskSnapshot } from "./taskCenter";

const session = (id: string, source = "chat", status = "idle"): Session => ({ id, source, status, title: `Session ${id}`, created_at: "2026-10-20T01:00:00Z", updated_at: "2026-10-25T01:00:00Z", message_count: 2 });
const scheduled: ScheduledTask = { id: "1", name: "每日巡检", task_prompt: "巡检", cron_expression: "0 9 * * *", status: "active", last_run_status: "failed", run_count: 3, created_at: "2026-10-20T01:00:00Z", last_run_at: "2026-10-25T03:00:00Z", next_run_at: "2026-10-26T01:00:00Z" };
const snapshot = (): TaskSnapshot => ({ sessions: [session("1"), session("im", "im_feishu", "running"), session("koi_runtime_1"), session("cli", "cli")], harvis: [{ ownerId: "test-owner", taskId: "1", kind: "music", prompt: "品牌配乐", status: "processing", progress: 35, queryError: "network offline", createdAt: 1 }], scheduled: [scheduled], errors: [] });

describe("Task Center real source aggregation", () => {
  beforeEach(() => { localStorage.clear(); localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ id: "test-owner" })); vi.mocked(invoke).mockReset(); });

  it("aggregates all real domains with stable namespaced identities, truthful states and source targets", () => {
    const tasks = aggregateTasks(snapshot());
    expect(tasks).toHaveLength(5);
    expect(new Set(tasks.map((task) => task.id)).size).toBe(5);
    expect(tasks.find((task) => task.source === "session" && task.title === "Session 1")?.status).toBe("idle");
    expect(tasks.find((task) => task.source === "im")).toMatchObject({ status: "running", target: { type: "session", sessionId: "im", source: "im_feishu", view: "conversation" } });
    expect(tasks.find((task) => task.source === "harvis")).toMatchObject({ status: "running", queryError: "network offline", target: { type: "harvis", taskId: "1", kind: "music", view: "task" } });
    expect(tasks.find((task) => task.source === "scheduler")).toMatchObject({ status: "failed", target: { type: "scheduler", taskId: "1", view: "task" } });
  });

  it("uses optional live session signals without classifying idle as success", () => {
    const tasks = aggregateTasks(snapshot(), { "1": { outcome: "error" }, im: { running: false, outcome: "unread" } });
    expect(tasks.find((task) => task.title === "Session 1")?.status).toBe("failed");
    expect(tasks.find((task) => task.source === "im")?.status).toBe("succeeded");
  });

  it("filters status, source and case-insensitive text including errors", () => {
    const tasks = aggregateTasks(snapshot());
    expect(filterTasks(tasks, { status: "running", search: "NETWORK" }).map((task) => task.source)).toEqual(["harvis"]);
    expect(filterTasks(tasks, { source: "im", search: "feishu" })).toHaveLength(1);
    expect(filterTasks(tasks, { status: "failed", search: "巡检" })).toHaveLength(1);
    expect(filterTasks(tasks, { status: "succeeded" })).toHaveLength(0);
  });

  it("loads every session page and scheduler IPC without querying removed online creative jobs or submitting work", async () => {
    saveHarvisTasks(snapshot().harvis);
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_sessions") return { sessions: [session((args as { offset: number }).offset === 0 ? "first" : "second")], total: 2 };
      if (command === "list_tasks") return { tasks: [scheduled], total: 1 };
      throw new Error(`Unexpected IPC ${command}`);
    });
    const result = await loadTaskSources();
    expect(result.sessions.map((item) => item.id)).toEqual(["first", "second"]);
    expect(result.scheduled).toEqual([scheduled]);
    expect(result.harvis).toHaveLength(1);
    expect(result.errors).toEqual([]);
    expect(vi.mocked(invoke).mock.calls.map(([command]) => command).sort()).toEqual(["list_sessions", "list_sessions", "list_tasks"]);
  });

  it("keeps successful sources and cached rows on a partial source failure then clears errors after recovery", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "list_sessions") throw new Error("session offline");
      if (command === "list_tasks") return { tasks: [scheduled], total: 1 };
      throw new Error(`Unexpected IPC ${command}`);
    });
    const result = await loadTaskSources(snapshot());
    expect(result.sessions).toEqual(snapshot().sessions);
    expect(result.scheduled).toEqual([scheduled]);
    expect(result.errors).toEqual([{ source: "sessions", message: "session offline" }]);
    vi.mocked(invoke).mockResolvedValue({ sessions: [], total: 0, jobs: [], tasks: [] });
    expect((await loadTaskSources(result)).errors).toEqual([]);
  });

  it("does not invent success for unknown states and keeps paused schedule configuration visible", () => {
    const data = snapshot();
    data.sessions = [session("odd", "chat", "future_state")];
    data.scheduled = [{ ...scheduled, status: "paused", last_run_status: "success" }];
    const tasks = aggregateTasks(data);
    expect(tasks.find((task) => task.source === "session")?.status).toBe("unknown");
    expect(tasks.find((task) => task.source === "scheduler")).toMatchObject({ status: "paused", rawStatus: "paused / success" });
  });

  it("classifies real scheduler execution sessions separately from IM and links only existing execution records", () => {
    const data = snapshot();
    data.sessions = [session("sched_1", "scheduled_task")];
    const tasks = aggregateTasks(data);
    expect(tasks.find((task) => task.id === "session:sched_1")).toMatchObject({ source: "session", sourceLabel: "定时任务会话" });
    expect(tasks.find((task) => task.source === "scheduler")?.results).toMatchObject([
      { label: "执行记录", target: { type: "session", sessionId: "sched_1", source: "scheduled_task", view: "conversation" } },
    ]);
    data.sessions = [];
    expect(aggregateTasks(data).find((task) => task.source === "scheduler")?.results).toEqual([]);
  });

  it("carries the complete session to preserve an old session workspace outside the parent's cached page", () => {
    const data = snapshot();
    data.sessions[0].workspace_root = "D:/archive/old-project";
    const target = aggregateTasks(data).find((task) => task.id === "session:1")?.target;
    expect(target?.type).toBe("session");
    if (target?.type !== "session") throw new Error("Expected a session target");
    expect(target.session).toEqual(data.sessions[0]);
    expect(target.session?.workspace_root).toBe("D:/archive/old-project");
  });

  it("excludes unscoped mobile records on every source page without changing raw pagination offsets", async () => {
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_tasks") return { tasks: [], total: 0 };
      if (command === "list_sessions") return {
        sessions: (args as { offset: number }).offset === 0
          ? [session("account-a-phone", "mobile"), session("desktop")]
          : [session("account-b-phone", "mobile_other_account"), session("im", "im_feishu")],
        total: 4,
      };
      throw new Error(`Unexpected IPC ${command}`);
    });
    const result = await loadTaskSources();
    expect(result.sessions.map((item) => item.id)).toEqual(["desktop", "im"]);
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "list_sessions").map(([, args]) => args)).toEqual([
      { limit: 100, offset: 0 }, { limit: 100, offset: 2 },
    ]);
  });

  it("scrubs mobile records from a previous snapshot even when the session source fails", async () => {
    const previous = snapshot();
    previous.sessions.push(session("old-account-phone", "mobile"), session("executor", "mobile_executor"));
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "list_tasks") return { tasks: [], total: 0 };
      throw new Error("session source unavailable");
    });
    const result = await loadTaskSources(previous);
    expect(result.sessions.map((item) => item.id)).not.toContain("old-account-phone");
    expect(result.sessions.map((item) => item.id)).not.toContain("executor");
    expect(result.sessions).toEqual(snapshot().sessions);
    expect(result.errors).toEqual([{ source: "sessions", message: "session source unavailable" }]);
  });

  it("never produces mobile rows or navigation targets from legacy raw snapshots or live signals", () => {
    const data = snapshot();
    data.sessions = [session("desktop"), ...["mobile", "mobile_executor", "mobile_other_account", " MoBiLe "].map((source, index) => ({
      ...session(`phone-${index}`, source), title: "PRIVATE_PHONE_TITLE", rolling_summary: "PRIVATE_PHONE_SUMMARY", workspace_root: "D:/PRIVATE_PHONE_WORKSPACE",
    }))];
    const tasks = aggregateTasks(data, { "phone-0": { running: true }, "phone-1": { outcome: "error" } });
    expect(tasks.filter((task) => task.target.type === "session").map((task) => task.title)).toEqual(["Session desktop"]);
    expect(filterTasks(tasks, { search: "PRIVATE_PHONE" })).toEqual([]);
    expect(JSON.stringify(tasks)).not.toMatch(/PRIVATE_PHONE|phone-/);
  });
});
