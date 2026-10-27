// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ArtifactPreview from "./ArtifactPreview";

const api = vi.hoisted(() => ({ getArtifactPreview: vi.fn() }));
vi.mock("../../services/tauri", () => ({ chatApi: api, openPath: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc: (path: string) => `http://asset.localhost/${path}` }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./AnnotationLayer", () => ({ default: ({ children }: { children: React.ReactNode }) => children }));

describe("isolated HTML artifact preview", () => {
  const base = "http://harmony-preview.localhost/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/";
  const artifact = { id: "a", name: "Report", uri: "C:/workspace/index.html" } as never;
  beforeEach(() => {
    api.getArtifactPreview.mockReset();
    api.getArtifactPreview.mockResolvedValue({
      kind: "html", uri: "C:/workspace/index.html", preview_url: `${base}index.html`,
      preview_base_url: base, is_outside_workspace: false,
      content: "<h1>Deck</h1><script>location='https://example.com'</script>",
      media_type: "text/html", size_bytes: 10, truncated: false,
    });
  });

  it("defaults to sanitized srcdoc without scripts or same-origin rights", async () => {
    render(<ArtifactPreview sessionId="s" artifact={artifact} />);
    await waitFor(() => expect(screen.getByTitle("Report")).toBeInTheDocument());
    const frame = screen.getByTitle("Report");
    expect(frame.getAttribute("src")).toBeNull();
    expect(frame.getAttribute("srcdoc")).toContain("Deck");
    expect(frame.getAttribute("srcdoc")).not.toContain("<script");
    expect(frame.getAttribute("sandbox")).toBe("");
    expect(frame.getAttribute("sandbox")).not.toContain("allow-same-origin");
    expect(api.getArtifactPreview).toHaveBeenLastCalledWith("s", "C:/workspace/index.html", false, false);
  });

  it("requires explicit interactive consent and remounts when returning to static", async () => {
    render(<ArtifactPreview sessionId="s" artifact={artifact} />);
    const staticFrame = await screen.findByTitle("Report");
    fireEvent.click(screen.getByRole("button", { name: "启用交互并允许联网" }));
    expect(api.getArtifactPreview).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("alertdialog")).toHaveTextContent("网页可联网和跳转");
    fireEvent.click(screen.getByRole("button", { name: "确认启用交互并允许联网" }));
    await waitFor(() => expect(screen.getByTitle("Report").getAttribute("sandbox")).toContain("allow-scripts"));
    const interactiveFrame = screen.getByTitle("Report");
    expect(interactiveFrame).not.toBe(staticFrame);
    expect(interactiveFrame.getAttribute("sandbox")).not.toContain("allow-same-origin");
    expect(interactiveFrame.getAttribute("src")).toBe(`${base}index.html`);
    expect(interactiveFrame.getAttribute("srcdoc")).toBeNull();
    expect(api.getArtifactPreview).toHaveBeenLastCalledWith("s", "C:/workspace/index.html", true, false);
    fireEvent.click(screen.getByRole("button", { name: "返回静态预览" }));
    await waitFor(() => expect(screen.getByTitle("Report").getAttribute("sandbox")).toBe(""));
    expect(screen.getByTitle("Report")).not.toBe(interactiveFrame);
    expect(screen.getByTitle("Report").getAttribute("src")).toBeNull();
  });

  it("requires a second explicit confirmation before granting external directory access", async () => {
    api.getArtifactPreview.mockResolvedValue({
      kind: "html", uri: "C:/outside/index.html", content: "<h1>Outside</h1>",
      preview_url: `${base}index.html`, preview_base_url: base, is_outside_workspace: true,
      size_bytes: 10, truncated: false,
    });
    render(<ArtifactPreview sessionId="s" artifact={artifact} />);
    await screen.findByTitle("Report");
    fireEvent.click(screen.getByRole("button", { name: "允许读取同目录资源" }));
    expect(api.getArtifactPreview).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("alertdialog")).toHaveTextContent("同目录及其子目录中的其他文件均可被网页读取");
    fireEvent.click(screen.getByRole("button", { name: "确认允许读取同目录资源" }));
    await waitFor(() => expect(api.getArtifactPreview).toHaveBeenLastCalledWith("s", "C:/workspace/index.html", false, true));
  });

  it("does not reuse consent for a new session or artifact", async () => {
    const view = render(<ArtifactPreview sessionId="s" artifact={artifact} />);
    await screen.findByTitle("Report");
    fireEvent.click(screen.getByRole("button", { name: "启用交互并允许联网" }));
    fireEvent.click(screen.getByRole("button", { name: "确认启用交互并允许联网" }));
    await waitFor(() => expect(screen.getByTitle("Report").getAttribute("sandbox")).toContain("allow-scripts"));
    view.rerender(<ArtifactPreview sessionId="other" artifact={artifact} />);
    await waitFor(() => expect(api.getArtifactPreview).toHaveBeenLastCalledWith("other", "C:/workspace/index.html", false, false));
    expect((await screen.findByTitle("Report")).getAttribute("sandbox")).toBe("");
    view.rerender(<ArtifactPreview sessionId="s" artifact={artifact} />);
    await waitFor(() => expect(api.getArtifactPreview).toHaveBeenLastCalledWith("s", "C:/workspace/index.html", false, false));
    expect((await screen.findByTitle("Report")).getAttribute("sandbox")).toBe("");
  });
});
