import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { saveHarvisTasks } from "../../services/tauri/creative";
import TaskCenter from "./index";
import { marketingApi } from "../../services/tauri/marketing";

describe("TaskCenter standalone page", () => {
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ id: "test-owner" }));
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "list_sessions") return { sessions: [
        { id: "chat-1", source: "chat", title: "工作方案", status: "idle", message_count: 2, created_at: "2026-10-24", updated_at: "2026-10-25", workspace_root: "D:/archive/old-project" },
        { id: "im-1", source: "im_feishu", title: "飞书消息", status: "running", message_count: 2, created_at: "2026-10-24", updated_at: "2026-10-25" },
      ], total: 2 };
      if (command === "list_tasks") return { tasks: [{ id: "timer-1", name: "门店日报", task_prompt: "日报", status: "active", last_run_status: "failed", cron_expression: "0 9 * * *", run_count: 1, created_at: "2026-10-24" }], total: 1 };
      if (command === "list_session_artifacts") return [{ id: "a-1", session_id: "chat-1", name: "方案.md", artifact_type: "file", uri: "/workspace/方案.md", content_summary: "方案", created_at: "2026-10-25" }];
      throw new Error(`Unexpected IPC ${command}`);
    });
    saveHarvisTasks([{ ownerId: "test-owner", taskId: "h-1", kind: "image", prompt: "橱窗海报", status: "succeeded", progress: 100, resultUrl: "https://cdn.example/poster.png", createdAt: 1 }]);
  });

  it("shows real sources, filters and sends typed source/result/error navigation callbacks", async () => {
    const onNavigate = vi.fn();
    render(<TaskCenter onNavigate={onNavigate} refreshIntervalMs={0} />);
    expect(await screen.findByText("工作方案")).toBeInTheDocument();
    expect(screen.getByText("飞书消息")).toBeInTheDocument();
    expect(vi.mocked(invoke).mock.calls.some(([command]) => command === "marketing_creative_jobs")).toBe(false);
    const image = screen.getByRole("article", { name: "橱窗海报" });
    await userEvent.click(within(image).getByRole("button", { name: "打开来源" }));
    expect(onNavigate).toHaveBeenLastCalledWith({ type: "harvis", taskId: "h-1", kind: "image", view: "task" });
    await userEvent.click(within(image).getByRole("button", { name: "查看成果" }));
    expect(onNavigate).toHaveBeenLastCalledWith({ type: "harvis", taskId: "h-1", kind: "image", view: "result" });
    const schedule = screen.getByRole("article", { name: "门店日报" });
    await userEvent.click(within(schedule).getByRole("button", { name: "查看错误" }));
    expect(onNavigate).toHaveBeenLastCalledWith({ type: "scheduler", taskId: "timer-1", view: "error" });
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "任务状态" }), "running");
    expect(screen.queryByText("工作方案")).not.toBeInTheDocument();
    expect(screen.getByText("飞书消息")).toBeInTheDocument();
    await userEvent.type(screen.getByRole("searchbox", { name: "搜索任务" }), "不存在");
    expect(screen.getByText("没有匹配的任务")).toBeInTheDocument();
  });

  it("keeps local sessions usable signed out while hiding unowned and other-account Harvis history without polling", async () => {
    localStorage.removeItem("workbuddy.marketingUser");
    localStorage.setItem("workbuddy.harvis.tasks", JSON.stringify([
      { taskId: "legacy", kind: "image", prompt: "legacy private", status: "pending", progress: 0, createdAt: 1 },
      { taskId: "account-A", ownerId: "A", kind: "video", prompt: "A private video", status: "processing", progress: 20, createdAt: 2 },
    ]));
    render(<TaskCenter onNavigate={vi.fn()} refreshIntervalMs={0} />);
    expect(await screen.findByText("工作方案")).toBeInTheDocument();
    expect(screen.queryAllByText(/legacy private|A private video/)).toHaveLength(0);
    expect(vi.mocked(invoke).mock.calls.some(([command]) => String(command).startsWith("harvis_"))).toBe(false);
  });

  it("removes A's media and history on logout without hiding local rows or showing it to B", async () => {
    localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ id: "A" }));
    saveHarvisTasks([{ ownerId: "A", taskId: "A-video", kind: "video", prompt: "private A video", status: "succeeded", progress: 100, resultUrl: "https://cdn.example/A.mp4", createdAt: 1 }]);
    render(<TaskCenter onNavigate={vi.fn()} refreshIntervalMs={0} />);
    const row = await screen.findByRole("article", { name: "private A video" });
    await userEvent.click(within(row).getByRole("button", { name: "查看成果" }));
    expect(screen.getByRole("dialog").querySelector("video")).toHaveAttribute("src", "https://cdn.example/A.mp4");
    await act(async () => { await marketingApi.logout(); });
    expect(screen.queryAllByText("private A video")).toHaveLength(0);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByText("工作方案")).toBeInTheDocument();
    act(() => {
      localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ id: "B" }));
      window.dispatchEvent(new StorageEvent("storage", { key: "workbuddy.marketingUser" }));
    });
    expect(screen.queryAllByText("private A video")).toHaveLength(0);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("loads actual session artifacts on demand and exposes their source contract", async () => {
    const onNavigate = vi.fn();
    render(<TaskCenter onNavigate={onNavigate} refreshIntervalMs={0} />);
    const row = await screen.findByRole("article", { name: "工作方案" });
    await userEvent.click(within(row).getByRole("button", { name: "会话成果" }));
    await userEvent.click(await screen.findByRole("button", { name: "方案.md" }));
    expect(onNavigate).toHaveBeenLastCalledWith(expect.objectContaining({ type: "session", sessionId: "chat-1", source: "chat", view: "artifacts", artifactId: "a-1", session: expect.objectContaining({ id: "chat-1", workspace_root: "D:/archive/old-project" }) }));
    expect(invoke).toHaveBeenCalledWith("list_session_artifacts", { sessionId: "chat-1", limit: 100 });
  });

  it("shows partial load errors without hiding Harvis results or treating them as failed", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("service offline"));
    render(<TaskCenter onNavigate={vi.fn()} refreshIntervalMs={0} />);
    expect(await screen.findAllByRole("alert")).toHaveLength(2);
    expect(screen.getByRole("article", { name: "橱窗海报" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "刷新任务" })).toBeEnabled();
  });

  it("reads same-window Harvis changes and reloads durable state on remount", async () => {
    const props = { onNavigate: vi.fn(), refreshIntervalMs: 0 };
    const view = render(<TaskCenter {...props} />);
    await screen.findByText("工作方案");
    act(() => saveHarvisTasks([{ ownerId: "test-owner", taskId: "new", kind: "music", prompt: "持久化配乐", status: "succeeded", progress: 100, adopted: true, rating: 4, createdAt: 2 }], "music"));
    expect(await screen.findByText("持久化配乐")).toBeInTheDocument();
    view.unmount();
    render(<TaskCenter {...props} />);
    await waitFor(() => expect(screen.getByText("持久化配乐")).toBeInTheDocument());
    expect(screen.getByText(/已采用.*4/)).toBeInTheDocument();
  });

  it("reloads session artifacts when reopened instead of caching an empty result forever", async () => {
    const original = vi.mocked(invoke).getMockImplementation()!;
    let artifactsRequested = 0;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_session_artifacts" && ++artifactsRequested === 1) return [];
      return original(command, args);
    });
    render(<TaskCenter onNavigate={vi.fn()} refreshIntervalMs={0} />);
    const row = await screen.findByRole("article", { name: "工作方案" });
    const toggle = within(row).getByRole("button", { name: "会话成果" });
    await userEvent.click(toggle);
    expect(await screen.findByText("暂无已登记成果")).toBeInTheDocument();
    await userEvent.click(toggle);
    await userEvent.click(toggle);
    expect(await screen.findByRole("button", { name: "方案.md" })).toBeInTheDocument();
    expect(artifactsRequested).toBe(2);
  });

  it("does not render or expose artifact controls for unscoped mobile sessions", async () => {
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_sessions") return { sessions: [
        { id: "old-phone", source: "mobile", title: "旧账号手机私密任务", status: "running", message_count: 2, created_at: "2026-10-24", updated_at: "2026-10-25" },
      ], total: 1 };
      return original(command, args);
    });
    render(<TaskCenter onNavigate={vi.fn()} refreshIntervalMs={0} />);
    await waitFor(() => expect(screen.getByRole("button", { name: "刷新任务" })).toBeEnabled());
    expect(screen.queryByText("旧账号手机私密任务")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "会话成果" })).not.toBeInTheDocument();
    expect(vi.mocked(invoke).mock.calls.some(([command]) => command === "list_session_artifacts")).toBe(false);
  });

  it("resets cached source rows and ignores account-A's late response when the parent remounts for account B", async () => {
    const onNavigate = vi.fn();
    const view = render(<TaskCenter key="account-a" onNavigate={onNavigate} refreshIntervalMs={0} />);
    await screen.findByText("工作方案");
    let resolveOld!: (value: unknown) => void;
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "list_sessions") return new Promise((resolve) => { resolveOld = resolve; });
      return Promise.resolve({ tasks: [], total: 0 });
    });
    await userEvent.click(screen.getByRole("button", { name: "刷新任务" }));
    vi.mocked(invoke).mockRejectedValue(new Error("account B source offline"));
    view.rerender(<TaskCenter key="account-b" onNavigate={onNavigate} refreshIntervalMs={0} />);
    await screen.findAllByRole("alert");
    expect(screen.queryByText("工作方案")).not.toBeInTheDocument();
    expect(screen.queryByText("飞书消息")).not.toBeInTheDocument();
    expect(screen.queryByText("门店日报")).not.toBeInTheDocument();
    await act(async () => resolveOld({ sessions: [
      { id: "a-phone-late", source: "mobile", title: "迟到的旧账号手机任务", status: "idle", message_count: 2, created_at: "2026-10-24", updated_at: "2026-10-25" },
      { id: "a-desktop-late", source: "chat", title: "迟到的旧快照", status: "idle", message_count: 2, created_at: "2026-10-24", updated_at: "2026-10-25" },
    ], total: 2 }));
    expect(screen.queryByText(/迟到的旧/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "会话成果" })).not.toBeInTheDocument();
    expect(onNavigate).not.toHaveBeenCalled();
  });
});
