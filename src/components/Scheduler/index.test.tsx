import { act, render, screen, waitFor } from "@testing-library/react";
import { Provider } from "react-redux";
import { configureStore } from "@reduxjs/toolkit";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { schedulerSlice } from "../../store/slices/chat";
import type { ScheduledTask } from "../../services/tauri/chat";
import Scheduler from "./index";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

const task = (id: string): ScheduledTask => ({ id, name: `任务 ${id}`, status: "active", task_prompt: id, cron_expression: "0 9 * * *", run_count: 1, created_at: "2026-10-26", last_run_status: "success" });
const makeStore = (tasks: ScheduledTask[] = []) => configureStore({
  reducer: { scheduler: schedulerSlice.reducer },
  preloadedState: { scheduler: { tasks, loading: false } },
});
const handlers = new Map<string, (event: { payload: { status: string } }) => void>();

describe("Scheduler task-center focus and status subscriptions", () => {
  beforeEach(() => {
    handlers.clear();
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "list_tasks") return { tasks: [task("a"), task("b")], total: 2 };
      throw new Error(`Unexpected write IPC ${command}`);
    });
    vi.mocked(listen).mockReset();
    vi.mocked(listen).mockImplementation(async (name, handler) => {
      handlers.set(name, handler as (event: { payload: { status: string } }) => void);
      return vi.fn<() => void>();
    });
  });
  afterEach(() => vi.restoreAllMocks());

  it("focuses and scrolls a loaded real task without running or modifying any task", async () => {
    const scroll = vi.fn();
    vi.spyOn(HTMLElement.prototype, "scrollIntoView").mockImplementation(scroll);
    const store = makeStore();
    render(<Provider store={store}><Scheduler focusedTaskId="b" /></Provider>);
    const row = await screen.findByRole("article", { name: "任务 b" });
    await waitFor(() => expect(row).toHaveFocus());
    expect(row).toHaveAttribute("data-focused", "true");
    expect(scroll).toHaveBeenCalledWith({ block: "nearest" });
    expect(vi.mocked(invoke).mock.calls.map(([command]) => command)).toEqual(["list_tasks"]);
  });

  it("does not report a deleted target until the source list finishes loading", async () => {
    let resolve!: (value: unknown) => void;
    vi.mocked(invoke).mockImplementation(() => new Promise((done) => { resolve = done; }));
    render(<Provider store={makeStore()}><Scheduler focusedTaskId="deleted" /></Provider>);
    expect(screen.queryByText(/不存在或已删除/)).not.toBeInTheDocument();
    await act(async () => { resolve({ tasks: [], total: 0 }); });
    expect(await screen.findByRole("status")).toHaveTextContent("目标定时任务不存在或已删除");
  });

  it("distinguishes load failure from a deleted focused task", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("scheduler offline"));
    render(<Provider store={makeStore()}><Scheduler focusedTaskId="missing" /></Provider>);
    expect(await screen.findByRole("alert")).toHaveTextContent("scheduler offline");
    expect(screen.queryByText(/不存在或已删除/)).not.toBeInTheDocument();
  });

  it("merges consecutive events for different tasks without reverting the first update", async () => {
    const store = makeStore();
    render(<Provider store={store}><Scheduler /></Provider>);
    await waitFor(() => expect(handlers.size).toBe(2));
    act(() => { handlers.get("task_status_a")!({ payload: { status: "running" } }); });
    expect(store.getState().scheduler.tasks.find((item) => item.id === "a")?.last_run_status).toBe("running");
    act(() => { handlers.get("task_status_b")!({ payload: { status: "failed" } }); });
    expect(store.getState().scheduler.tasks.map((item) => [item.id, item.last_run_status])).toEqual([["a", "running"], ["b", "failed"]]);
    act(() => {
      handlers.get("task_status_a")!({ payload: { status: "success" } });
      handlers.get("task_status_b")!({ payload: { status: "running" } });
    });
    expect(store.getState().scheduler.tasks.map((item) => [item.id, item.last_run_status])).toEqual([["a", "success"], ["b", "running"]]);
  });

  it("does not resurrect removed tasks or overwrite fields changed since subscription", async () => {
    const store = makeStore();
    render(<Provider store={store}><Scheduler /></Provider>);
    await waitFor(() => expect(handlers.size).toBe(2));
    const oldHandler = handlers.get("task_status_b")!;
    act(() => { store.dispatch(schedulerSlice.actions.setTasks([{ ...task("a"), name: "新名称", status: "paused" }, task("b")])); });
    act(() => { oldHandler({ payload: { status: "running" } }); });
    expect(store.getState().scheduler.tasks[0]).toMatchObject({ name: "新名称", status: "paused" });
    act(() => { store.dispatch(schedulerSlice.actions.removeTask("b")); });
    act(() => { oldHandler({ payload: { status: "failed" } }); });
    expect(store.getState().scheduler.tasks.map((item) => item.id)).toEqual(["a"]);
  });

  it("unsubscribes delayed listener registrations and ignores events after unmount", async () => {
    const unlisten = vi.fn<() => void>();
    const registrations: Array<() => void> = [];
    vi.mocked(listen).mockImplementation((name, handler) => {
      handlers.set(name, handler as (event: { payload: { status: string } }) => void);
      return new Promise((resolve) => { registrations.push(() => resolve(unlisten)); });
    });
    const store = makeStore();
    const view = render(<Provider store={store}><Scheduler /></Provider>);
    await waitFor(() => expect(handlers.size).toBe(2));
    view.unmount();
    await act(async () => {
      registrations.forEach((resolve) => resolve());
      handlers.get("task_status_a")!({ payload: { status: "running" } });
    });
    expect(unlisten).toHaveBeenCalledTimes(2);
    expect(store.getState().scheduler.tasks[0].last_run_status).toBe("success");
  });

  it("does not replace Redux tasks when a pending initial load resolves after unmount", async () => {
    let resolve!: (value: unknown) => void;
    vi.mocked(invoke).mockImplementation(() => new Promise((done) => { resolve = done; }));
    const store = makeStore([task("existing")]);
    const view = render(<Provider store={store}><Scheduler focusedTaskId="a" /></Provider>);
    view.unmount();
    await act(async () => { resolve({ tasks: [task("a")], total: 1 }); });
    expect(store.getState().scheduler.tasks.map((item) => item.id)).toEqual(["existing"]);
  });
});
