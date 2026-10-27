import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import CreativePage from "./index";
import { marketingApi } from "../../services/tauri/marketing";

const mocks = vi.hoisted(() => ({
  listJobs: vi.fn(), musicConfig: vi.fn(), harvisGenerate: vi.fn(), harvisQueryTask: vi.fn(), harvisQueryTaskContent: vi.fn(), harvisPreviewResult: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
vi.mock("../../services/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../services/tauri")>();
  return { ...actual, creativeApi: { ...actual.creativeApi, ...mocks } };
});

const user = { id: "user-1", name: "测试用户", email: "test@example.com", userRole: "user" as const, accountType: "standard" as const, approvalStatus: "active" as const };
const completed = (kind: "music" | "image") => ({ ownerId: user.id, taskId: `${kind}-1`, kind, status: "succeeded", prompt: `${kind} prompt`, progress: 100, resultUrl: `https://cdn.example/${kind}.png`, createdAt: Date.now() });

describe("CreativePage personal-key results", () => {
  afterEach(() => vi.useRealTimers());
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem("workbuddy.marketingUser", JSON.stringify(user));
    vi.clearAllMocks();
    mocks.musicConfig.mockImplementation(() => new Promise(() => {}));
    mocks.harvisQueryTask.mockImplementation((taskId: string) => Promise.resolve({
      status: taskId.endsWith("queued") ? "queued" : "processing",
      progress: taskId.endsWith("queued") ? null : 37,
    }));
  });

  it("shows only local results for the current kind without loading online jobs", () => {
    localStorage.setItem("workbuddy.harvis.tasks", JSON.stringify([completed("image"), completed("music")]));
    render(<CreativePage initialKind="image" user={user} />);
    expect(screen.getByText("创作成果")).toBeInTheDocument();
    expect(screen.getByText("image prompt")).toBeInTheDocument();
    expect(screen.queryByText("music prompt")).not.toBeInTheDocument();
    expect(mocks.listJobs).not.toHaveBeenCalled();
  });

  it("previews the selected image result", async () => {
    localStorage.setItem("workbuddy.harvis.tasks", JSON.stringify([completed("image")]));
    mocks.harvisPreviewResult.mockResolvedValue({ dataUrl: "data:image/png;base64,aGVsbG8=" });
    render(<CreativePage initialKind="image" user={user} />);
    await userEvent.click(screen.getByRole("button", { name: "预览成品" }));
    expect(await screen.findByRole("dialog", { name: "Harvis 图片成品" })).toBeInTheDocument();
  });

  it("persists adoption and rating locally", async () => {
    localStorage.setItem("workbuddy.harvis.tasks", JSON.stringify([completed("music")]));
    const view = render(<CreativePage initialKind="music" user={user} />);
    expect(screen.getByRole("button", { name: "5 星" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "采用作品" }));
    await userEvent.click(screen.getByRole("button", { name: "5 星" }));
    expect(JSON.parse(localStorage.getItem("workbuddy.harvis.tasks") ?? "[]")[0]).toMatchObject({ adopted: true, rating: 5 });
    view.unmount();
    render(<CreativePage initialKind="music" user={user} />);
    expect(screen.getByRole("button", { name: "已采用" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "5 星" })).toHaveClass("active");
  });

  it("shows submitted and processing progress without inventing a percentage", async () => {
    localStorage.setItem("workbuddy.harvis.tasks", JSON.stringify([
      { ownerId: user.id, taskId: "image-queued", kind: "image", status: "pending", prompt: "排队中的图片", progress: 0, createdAt: Date.now() },
      { ownerId: user.id, taskId: "image-running", kind: "image", status: "processing", prompt: "制作中的图片", progress: 37, createdAt: Date.now() },
    ]));
    render(<CreativePage initialKind="image" user={user} />);
    await act(async () => { await Promise.resolve(); });
    expect(screen.getByText("排队中")).toBeInTheDocument();
    expect(screen.getByText("进度待更新")).toBeInTheDocument();
    expect(screen.getByText("37%")).toBeInTheDocument();
    expect(screen.getAllByRole("progressbar", { name: "生成进度" })).toHaveLength(2);
  });

  it("submits a task without exposing its ID or key details", async () => {
    mocks.harvisGenerate.mockResolvedValue({ task_id: "secret-task-123" });
    render(<CreativePage initialKind="music" user={user} />);
    await userEvent.click(screen.getByRole("button", { name: "开始制作" }));
    await waitFor(() => expect(JSON.parse(localStorage.getItem("workbuddy.harvis.tasks") ?? "[]")[0]?.taskId).toBe("secret-task-123"));
    expect(screen.queryByText(/secret-task-123|个人 Harmony Key|任务已提交/)).not.toBeInTheDocument();
  });

  it("coalesces manual and timer refreshes while a task query is pending", async () => {
    vi.useFakeTimers();
    localStorage.setItem("workbuddy.harvis.tasks", JSON.stringify([
      { ...completed("image"), status: "processing", progress: 20, resultUrl: undefined },
    ]));
    let resolve!: (value: Record<string, unknown>) => void;
    mocks.harvisQueryTask.mockImplementation(() => new Promise((done) => { resolve = done; }));
    render(<CreativePage initialKind="image" user={user} />);
    await act(async () => {
      screen.getByRole("button", { name: "刷新成果" }).click();
      await vi.advanceTimersByTimeAsync(16_000);
    });
    const calls = mocks.harvisQueryTask.mock.calls.length;
    await act(async () => { resolve({ status: "succeeded", url: "https://cdn.example/final.png" }); });
    expect(calls).toBe(1);
    expect(JSON.parse(localStorage.getItem("workbuddy.harvis.tasks")!)[0].status).toBe("succeeded");
  });

  it("keeps query failure separate from generation failure and retries", async () => {
    localStorage.setItem("workbuddy.harvis.tasks", JSON.stringify([
      { ...completed("image"), status: "processing", progress: 20, resultUrl: undefined },
    ]));
    mocks.harvisQueryTask.mockRejectedValueOnce(new Error("network offline"));
    render(<CreativePage initialKind="image" user={user} />);
    await waitFor(() => expect(JSON.parse(localStorage.getItem("workbuddy.harvis.tasks")!)[0]).toMatchObject({
      status: "processing", queryError: "network offline",
    }));
    expect(JSON.parse(localStorage.getItem("workbuddy.harvis.tasks")!)[0].errorMessage).toBeUndefined();
    expect(screen.getByText(/查询暂不可用/)).toBeInTheDocument();
    mocks.harvisQueryTask.mockResolvedValue({ status: "succeeded", url: "https://cdn.example/final.png" });
    await userEvent.click(screen.getByRole("button", { name: "刷新成果" }));
    await waitFor(() => expect(screen.getByText("已完成")).toBeInTheDocument());
    expect(screen.queryByText(/查询暂不可用/)).not.toBeInTheDocument();
  });

  it("accepts a task-center result target and previews the selected local task", async () => {
    localStorage.setItem("workbuddy.harvis.tasks", JSON.stringify([completed("image")]));
    mocks.harvisPreviewResult.mockResolvedValue({ dataUrl: "data:image/png;base64,aGVsbG8=" });
    const view = render(<CreativePage initialKind="image" user={user} focusedTaskId="image-1" focusResult />);
    expect(await screen.findByRole("dialog", { name: "Harvis 图片成品" })).toBeInTheDocument();
    expect(document.querySelector('[data-task-id="image-1"]')).toHaveAttribute("data-focused", "true");
    expect(mocks.harvisGenerate).not.toHaveBeenCalled();
    view.rerender(<CreativePage initialKind="image" user={user} focusedTaskId="image-1" focusResult />);
    expect(screen.getByRole("dialog", { name: "Harvis 图片成品" })).toBeInTheDocument();
    expect(mocks.harvisPreviewResult).toHaveBeenCalledTimes(1);
  });

  it("opens a focused result even under StrictMode effect replay", async () => {
    localStorage.setItem("workbuddy.harvis.tasks", JSON.stringify([completed("image")]));
    mocks.harvisPreviewResult.mockResolvedValue({ dataUrl: "data:image/png;base64,aGVsbG8=" });
    render(<StrictMode><CreativePage initialKind="image" user={user} focusedTaskId="image-1" focusResult /></StrictMode>);
    expect(await screen.findByRole("dialog", { name: "Harvis 图片成品" })).toBeInTheDocument();
  });

  it("never focuses or previews a task from another kind with the same requested task ID", async () => {
    localStorage.setItem("workbuddy.harvis.tasks", JSON.stringify([{ ...completed("music"), taskId: "shared-id" }]));
    render(<CreativePage initialKind="image" user={user} focusedTaskId="shared-id" focusResult />);
    await act(async () => { await Promise.resolve(); });
    expect(mocks.harvisPreviewResult).not.toHaveBeenCalled();
    expect(document.querySelector('[data-focused="true"]')).toBeNull();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("persists a submission resolving after unmount in its original kind", async () => {
    let resolve!: (value: Record<string, unknown>) => void;
    mocks.harvisGenerate.mockImplementation(() => new Promise((done) => { resolve = done; }));
    const view = render(<CreativePage initialKind="music" user={user} />);
    await userEvent.click(screen.getByRole("button", { name: "开始制作" }));
    view.unmount();
    localStorage.setItem("workbuddy.harvis.tasks", JSON.stringify([completed("image")]));
    await act(async () => { resolve({ task_id: "late-music" }); });
    expect(JSON.parse(localStorage.getItem("workbuddy.harvis.tasks")!)).toEqual(expect.arrayContaining([
      expect.objectContaining({ taskId: "late-music", kind: "music", status: "pending" }),
      expect.objectContaining({ taskId: "image-1", kind: "image", status: "succeeded" }),
    ]));
  });

  it.each(["B", "user-1"])("does not persist a submission from the previous login after logout and login as %s", async id => {
    localStorage.setItem("workbuddy.marketingUser", JSON.stringify(user));
    let resolve!: (value: Record<string, unknown>) => void;
    mocks.harvisGenerate.mockImplementation(() => new Promise(done => { resolve = done; }));
    const view = render(<CreativePage initialKind="music" user={user} />);
    await userEvent.click(screen.getByRole("button", { name: "开始制作" }));
    await act(async () => { await marketingApi.logout(); });
    act(() => {
      localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ ...user, id }));
      window.dispatchEvent(new StorageEvent("storage", { key: "workbuddy.marketingUser" }));
    });
    view.rerender(<CreativePage initialKind="music" user={{ ...user, id }} />);
    await act(async () => { resolve({ task_id: "late-private-A", url: "https://cdn.example/A.mp3" }); });
    expect(JSON.parse(localStorage.getItem("workbuddy.harvis.tasks") ?? "[]").some((task: { taskId: string }) => task.taskId === "late-private-A")).toBe(false);
    expect(screen.queryByText("已完成")).not.toBeInTheDocument();
  });
});
import { StrictMode } from "react";
