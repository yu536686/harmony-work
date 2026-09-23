import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CreativeJob } from "../../services/tauri";
import CreativePage from "./index";
import { save } from "@tauri-apps/plugin-dialog";

const mocks = vi.hoisted(() => ({
  listJobs: vi.fn(),
  getJob: vi.fn(),
  musicConfig: vi.fn(),
  composeMusicPrompt: vi.fn(),
  adoptMusicArtifact: vi.fn(),
  saveMusicFeedback: vi.fn(),
  harvisSaveResult: vi.fn(),
  harvisPreviewResult: vi.fn(),
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
      musicConfig: mocks.musicConfig,
      composeMusicPrompt: mocks.composeMusicPrompt,
      adoptMusicArtifact: mocks.adoptMusicArtifact,
      saveMusicFeedback: mocks.saveMusicFeedback,
      harvisSaveResult: mocks.harvisSaveResult,
      harvisPreviewResult: mocks.harvisPreviewResult,
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
  adoptedArtifactId: null,
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
    localStorage.clear();
    vi.mocked(save).mockReset();
    mocks.harvisSaveResult.mockReset();
    mocks.harvisPreviewResult.mockReset();
    mocks.listJobs.mockReset();
    mocks.getJob.mockReset();
    mocks.musicConfig.mockReset();
    mocks.composeMusicPrompt.mockReset();
    mocks.adoptMusicArtifact.mockReset();
    mocks.saveMusicFeedback.mockReset();
    mocks.musicConfig.mockResolvedValue({
      businessLines: [{ id: "watch-poetry", title: "腕表如诗", description: "腕表配乐", fields: [] }],
      brands: [{ id: "chopard", name: "萧邦", collections: ["Happy Sport"] }]
    });
  });

  it("saves a local generation to the selected path and handles cancellation and errors", async () => {
    localStorage.setItem("workbuddy.harvis.tasks", JSON.stringify([{
      taskId: "resp-test", kind: "music", status: "succeeded", prompt: "piano",
      progress: 100, resultUrl: "https://cdn.example/music.m4a", createdAt: Date.now()
    }]));
    mocks.listJobs.mockResolvedValue({ jobs: [] });
    render(<CreativePage initialKind="music" user={user} onStageToChat={vi.fn()} />);
    const download = await screen.findByRole("button", { name: "下载成品" });
    vi.mocked(save).mockResolvedValueOnce(null);
    await userEvent.click(download);
    await waitFor(() => expect(download).not.toBeDisabled());
    expect(mocks.harvisSaveResult).not.toHaveBeenCalled();
    vi.mocked(save).mockResolvedValueOnce("C:\\Users\\test\\Desktop\\music.m4a");
    mocks.harvisSaveResult.mockResolvedValueOnce({ localPath: "C:\\Users\\test\\Desktop\\music.m4a", sizeBytes: 12 });
    await userEvent.click(download);
    expect(await screen.findByText(/已保存到/)).toBeInTheDocument();
    expect(mocks.harvisSaveResult).toHaveBeenCalledWith("https://cdn.example/music.m4a", "C:\\Users\\test\\Desktop\\music.m4a");
    vi.mocked(save).mockResolvedValueOnce("C:\\Users\\test\\Desktop\\music.m4a");
    mocks.harvisSaveResult.mockRejectedValueOnce(new Error("disk full"));
    await userEvent.click(download);
    expect(await screen.findByText(/disk full/)).toBeInTheDocument();
    expect(download).not.toBeDisabled();
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

    const view = render(<CreativePage initialKind="image" user={user} onStageToChat={vi.fn()} />);

    expect(await screen.findByRole("button", { name: "下载 七夕海报.png" })).toBeInTheDocument();
    expect(mocks.getJob).toHaveBeenCalledWith("image-1");
    expect(screen.queryByText("music prompt")).not.toBeInTheDocument();

    view.unmount();
    render(<CreativePage initialKind="music" user={user} onStageToChat={vi.fn()} />);

    await userEvent.click(await screen.findByRole("button", { name: /music prompt/ }));

    expect(await screen.findByRole("button", { name: "下载 品牌音乐.mp3" })).toBeInTheDocument();
    await waitFor(() => expect(mocks.getJob).toHaveBeenCalledWith("music-1"));
    expect(screen.queryByText("image prompt")).not.toBeInTheDocument();
  });

  it("filters local results by page and opens a clicked image in the preview", async () => {
    localStorage.setItem("workbuddy.harvis.tasks", JSON.stringify([
      { taskId: "image-1", kind: "image", status: "succeeded", prompt: "portrait", progress: 100, resultUrl: "https://cdn.example/portrait.png", createdAt: Date.now() },
      { taskId: "music-1", kind: "music", status: "succeeded", prompt: "piano", progress: 100, resultUrl: "https://cdn.example/music.mp3", createdAt: Date.now() },
    ]));
    mocks.listJobs.mockResolvedValue({ jobs: [] });
    mocks.harvisPreviewResult.mockResolvedValue({ dataUrl: "data:image/png;base64,aGVsbG8=" });
    render(<CreativePage initialKind="image" user={user} onStageToChat={vi.fn()} />);

    expect(screen.getByText("portrait")).toBeInTheDocument();
    expect(screen.queryByText("piano")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "预览成品" }));
    expect(await screen.findByRole("dialog", { name: "Harvis 图片成品" })).toBeInTheDocument();
    expect(mocks.harvisPreviewResult).toHaveBeenCalledWith("https://cdn.example/portrait.png", "image");
  });

  it("shows the assistant rationale, confidence, and retrieved cases next to the editable Suno prompt", async () => {
    mocks.listJobs.mockResolvedValue({ jobs: [] });
    mocks.composeMusicPrompt.mockResolvedValue({
      prompt: "Instrumental luxury neo-classical, 72 BPM, piano and harp, no vocals",
      analysisZh: "这款正装腕表适合克制、诗意的低动态配乐。",
      confidence: 0.86,
      evidenceCases: [{ id: "case-1", title: "腕表如诗·正装大表", genre: "新古典" }],
    });

    render(<CreativePage initialKind="music" user={user} onStageToChat={vi.fn()} />);
    await userEvent.click(await screen.findByRole("button", { name: "助手模式" }));
    await userEvent.click(screen.getByRole("button", { name: "生成提示词" }));

    expect(await screen.findByText("这款正装腕表适合克制、诗意的低动态配乐。")).toBeInTheDocument();
    expect(screen.getByText("置信度 86%")).toBeInTheDocument();
    expect(screen.getByText(/腕表如诗·正装大表/)).toBeInTheDocument();
    expect(screen.getByDisplayValue("Instrumental luxury neo-classical, 72 BPM, piano and harp, no vocals")).toBeInTheDocument();
  });

  it("requires adoption before submitting a highly rated work as knowledge", async () => {
    const musicJob = {
      ...baseJob("music-feedback", "music"),
      artifacts: [{ id: "audio-feedback", kind: "audio" as const, assetId: "asset-feedback", fileName: "候选音乐.mp3", mimeType: "audio/mpeg", byteSize: 2048, durationSeconds: 45, previewUrl: null }]
    };
    mocks.listJobs.mockResolvedValue({ jobs: [musicJob] });
    mocks.getJob.mockResolvedValue({ job: musicJob });
    mocks.adoptMusicArtifact.mockResolvedValue({ jobId: musicJob.id, artifactId: "audio-feedback" });
    mocks.saveMusicFeedback.mockResolvedValue({ artifactId: "audio-feedback", rating: 5, candidateId: "candidate-1" });

    render(<CreativePage initialKind="music" user={user} onStageToChat={vi.fn()} />);

    await userEvent.click(await screen.findByRole("button", { name: "采用此版本" }));
    await userEvent.click(screen.getByRole("button", { name: "5 星" }));
    await userEvent.click(screen.getByLabelText("提交为知识候选"));
    await userEvent.click(screen.getByRole("button", { name: "保存评分" }));

    await waitFor(() => expect(mocks.saveMusicFeedback).toHaveBeenCalledWith("audio-feedback", 5, [], "", true));
    expect(await screen.findByText("评分已保存，作品已进入待审核知识候选")).toBeInTheDocument();
  });
});
