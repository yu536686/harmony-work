import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CreativeJob } from "../../services/tauri";
import CreativePage from "./index";

const mocks = vi.hoisted(() => ({
  listJobs: vi.fn(),
  getJob: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));

vi.mock("../../services/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../services/tauri")>();
  return {
    ...actual,
    creativeApi: {
      ...actual.creativeApi,
      listJobs: mocks.listJobs,
      getJob: mocks.getJob,
    },
  };
});

const baseJob = (id: string, kind: "image" | "music"): CreativeJob => ({
  id,
  kind,
  mode: kind === "image" ? "text" : "free",
  status: "succeeded",
  progress: 100,
  progressMessage: "已完成",
  promptSnapshot: `${kind} prompt`,
  options: {},
  errorMessage: "",
  createdAt: "2026-08-22T08:00:00Z",
  updatedAt: "2026-08-22T08:01:00Z",
  completedAt: "2026-08-22T08:01:00Z",
  artifacts: [],
});

const user = {
  id: "user-1",
  name: "测试用户",
  email: "test@example.com",
  userRole: "user" as const,
  approvalStatus: "active" as const,
};

describe("CreativePage completed job details", () => {
  beforeEach(() => {
    mocks.listJobs.mockReset();
    mocks.getJob.mockReset();
  });

  it("loads the selected completed job detail so image and music downloads are visible", async () => {
    const imageSummary = baseJob("image-1", "image");
    const musicSummary = baseJob("music-1", "music");
    mocks.listJobs.mockResolvedValue({ jobs: [imageSummary, musicSummary] });
    mocks.getJob.mockImplementation(async (id: string) => ({
      job: {
        ...(id === "image-1" ? imageSummary : musicSummary),
        artifacts: id === "image-1"
          ? [{ id: "artifact-image", kind: "image", assetId: "asset-image", fileName: "七夕海报.png", mimeType: "image/png", byteSize: 1024, durationSeconds: null, previewUrl: null }]
          : [{ id: "artifact-audio", kind: "audio", assetId: "asset-audio", fileName: "品牌音乐.mp3", mimeType: "audio/mpeg", byteSize: 2048, durationSeconds: 45, previewUrl: null }],
      },
    }));

    render(<CreativePage initialKind="image" user={user} onStageToChat={vi.fn()} />);

    expect(await screen.findByRole("button", { name: "下载 七夕海报.png" })).toBeInTheDocument();
    expect(mocks.getJob).toHaveBeenCalledWith("image-1");

    await userEvent.click(screen.getByText("music prompt"));

    expect(await screen.findByRole("button", { name: "下载 品牌音乐.mp3" })).toBeInTheDocument();
    await waitFor(() => expect(mocks.getJob).toHaveBeenCalledWith("music-1"));
  });
});
