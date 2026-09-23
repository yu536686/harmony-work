import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import CreativePage from "./index";

const mocks = vi.hoisted(() => ({
  listJobs: vi.fn(), musicConfig: vi.fn(), harvisGenerate: vi.fn(), harvisPreviewResult: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
vi.mock("../../services/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../services/tauri")>();
  return { ...actual, creativeApi: { ...actual.creativeApi, ...mocks } };
});

const user = { id: "user-1", name: "测试用户", email: "test@example.com", userRole: "user" as const, approvalStatus: "active" as const };
const completed = (kind: "music" | "image") => ({ taskId: `${kind}-1`, kind, status: "succeeded", prompt: `${kind} prompt`, progress: 100, resultUrl: `https://cdn.example/${kind}.png`, createdAt: Date.now() });

describe("CreativePage personal-key results", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    mocks.musicConfig.mockImplementation(() => new Promise(() => {}));
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

  it("submits a task without exposing its ID or key details", async () => {
    mocks.harvisGenerate.mockResolvedValue({ task_id: "secret-task-123" });
    render(<CreativePage initialKind="music" user={user} />);
    await userEvent.click(screen.getByRole("button", { name: "开始制作" }));
    await waitFor(() => expect(JSON.parse(localStorage.getItem("workbuddy.harvis.tasks") ?? "[]")[0]?.taskId).toBe("secret-task-123"));
    expect(screen.queryByText(/secret-task-123|个人 Harmony Key|任务已提交/)).not.toBeInTheDocument();
  });
});
