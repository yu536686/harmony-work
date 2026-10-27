import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { saveHarvisTasks, type HarvisTask } from "../../services/tauri/creative";
import TaskCenter from "./index";

vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
const mediaTask = (kind: "video" | "tts", resultUrl?: string): HarvisTask => ({
  ownerId: "test-owner", taskId: "shared", kind, prompt: `${kind} 成果`, status: "succeeded", progress: 100,
  resultUrl: resultUrl ?? `https://cdn.example/${kind}.${kind === "video" ? "mp4" : "wav"}?token=a%2Fb`, createdAt: 1,
});
const openResult = async (kind: "video" | "tts") => {
  const row = await screen.findByRole("article", { name: `${kind} 成果` });
  await userEvent.click(within(row).getByRole("button", { name: "查看成果" }));
  return screen.findByRole("dialog");
};

describe("TaskCenter video and TTS local result access", () => {
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ id: "test-owner" }));
    vi.mocked(save).mockReset();
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "list_sessions") return { sessions: [], total: 0 };
      if (command === "list_tasks") return { tasks: [], total: 0 };
      if (command === "harvis_save_result") return { localPath: "/tmp/saved-media.mp4", sizeBytes: 6 };
      throw new Error(`Unexpected IPC ${command}`);
    });
  });

  it.each(["video", "tts"] as const)("previews the existing %s with native controls without unsupported App navigation or a submit", async (kind) => {
    const task = mediaTask(kind);
    saveHarvisTasks([task]);
    const onNavigate = vi.fn();
    render(<TaskCenter onNavigate={onNavigate} refreshIntervalMs={0} />);
    const dialog = await openResult(kind);
    const media = dialog.querySelector(kind === "video" ? "video" : "audio")!;
    expect(media).toHaveAttribute("src", task.resultUrl);
    expect(media).toHaveAttribute("controls");
    expect(media).toHaveAttribute("crossorigin", "anonymous");
    expect(media).not.toHaveAttribute("autoplay");
    expect(dialog.querySelector("iframe, object, embed")).toBeNull();
    expect(onNavigate).not.toHaveBeenCalled();
    expect(vi.mocked(invoke).mock.calls.every(([command]) => ["list_sessions", "list_tasks"].includes(command))).toBe(true);
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("shows the actual task when the unsupported-kind source has no result yet", async () => {
    saveHarvisTasks([{ ...mediaTask("video"), status: "failed", resultUrl: undefined, errorMessage: "provider rejected video" }]);
    const onNavigate = vi.fn();
    render(<TaskCenter onNavigate={onNavigate} refreshIntervalMs={0} />);
    const row = await screen.findByRole("article", { name: "video 成果" });
    await userEvent.click(within(row).getByRole("button", { name: "打开来源" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("provider rejected video")).toBeInTheDocument();
    expect(within(dialog).getByText("任务尚无可用成品")).toBeInTheDocument();
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it.each(["javascript:alert(1)", "file:///C:/secret.mp4", "data:text/html;base64,PHNjcmlwdD4=", "https://user:pass@cdn.example/video.mp4"])("blocks an unsafe result without attaching it to media or save: %s", async (resultUrl) => {
    saveHarvisTasks([mediaTask("video", resultUrl)]);
    render(<TaskCenter onNavigate={vi.fn()} refreshIntervalMs={0} />);
    const dialog = await openResult("video");
    expect(within(dialog).getByRole("alert")).toHaveTextContent("无法安全预览或保存");
    expect(dialog.querySelector("video, audio, iframe, a[href]")).toBeNull();
    expect(within(dialog).getByRole("button", { name: "保存成品" })).toBeDisabled();
  });

  it("reports playback errors and saves through the existing native download API", async () => {
    const task = mediaTask("video");
    saveHarvisTasks([task]);
    vi.mocked(save).mockResolvedValue("/tmp/chosen-video.mp4");
    render(<TaskCenter onNavigate={vi.fn()} refreshIntervalMs={0} />);
    const dialog = await openResult("video");
    fireEvent.error(dialog.querySelector("video")!);
    expect(within(dialog).getByRole("alert")).toHaveTextContent("媒体加载失败");
    await userEvent.click(within(dialog).getByRole("button", { name: "保存成品" }));
    expect(invoke).toHaveBeenCalledWith("harvis_save_result", { resultUrl: task.resultUrl, targetPath: "/tmp/chosen-video.mp4" });
    expect(await within(dialog).findByText(/已保存/)).toBeInTheDocument();
  });

  it("isolates identical IDs by kind and reports removal while the media dialog is open", async () => {
    saveHarvisTasks([mediaTask("video"), mediaTask("tts")]);
    render(<TaskCenter onNavigate={vi.fn()} refreshIntervalMs={0} />);
    let dialog = await openResult("video");
    expect(dialog.querySelector("video")).toHaveAttribute("src", mediaTask("video").resultUrl);
    await userEvent.keyboard("{Escape}");
    dialog = await openResult("tts");
    expect(dialog.querySelector("audio")).toHaveAttribute("src", mediaTask("tts").resultUrl);
    act(() => saveHarvisTasks([], "tts"));
    expect(within(dialog).getByRole("alert")).toHaveTextContent("任务不存在或已删除");
  });

  it("keeps music result navigation on the existing App callback", async () => {
    saveHarvisTasks([{ ...mediaTask("tts"), kind: "music", prompt: "music 成果" }]);
    const onNavigate = vi.fn();
    render(<TaskCenter onNavigate={onNavigate} refreshIntervalMs={0} />);
    const row = await screen.findByRole("article", { name: "music 成果" });
    await userEvent.click(within(row).getByRole("button", { name: "查看成果" }));
    expect(onNavigate).toHaveBeenCalledWith({ type: "harvis", taskId: "shared", kind: "music", view: "result" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
