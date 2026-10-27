import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import CommandPalette from "./index";

const session = { id: "s-1", title: "季度方案会话", source: "im_feishu", status: "idle", created_at: "2026-10-26", updated_at: "2026-10-26", message_count: 5, workspace_root: "/work/季度项目", pool_session_id: "team", policy_mode: "strict" };
const results = [
  { id: "session:s-1", kind: "session", title: session.title, source: "im_feishu", session, artifactId: null, workspaceRoot: session.workspace_root },
  { id: "artifact:s-1:a-1", kind: "artifact", title: "季度成果.xlsx", source: "im_feishu", session, artifactId: "a-1", workspaceRoot: session.workspace_root },
  { id: "workspace:/work/季度项目", kind: "workspace", title: "/work/季度项目", source: "im_feishu", session, artifactId: null, workspaceRoot: session.workspace_root },
];

describe("global CommandPalette", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue({ results, hasMore: false });
  });
  it("focuses real global search and shows session/artifact/workspace provenance", async () => {
    render(<CommandPalette open onClose={vi.fn()} onNavigate={vi.fn()} />);
    const input = screen.getByRole("combobox", { name: "全局搜索" });
    expect(input).toHaveFocus();
    fireEvent.change(input, { target: { value: "季度" } });
    expect(await screen.findByRole("option", { name: /季度成果.xlsx/ })).toHaveTextContent("季度方案会话");
    expect(screen.getByRole("option", { name: /季度成果.xlsx/ })).toHaveTextContent("飞书");
    expect(screen.getByRole("option", { name: /项目.*季度项目/ })).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("global_search", { query: "季度", limit: 32 });
  });
  it("supports keyboard selection and revalidates complete owner metadata plus artifactId before navigation", async () => {
    const onNavigate = vi.fn();
    const onClose = vi.fn();
    render(<CommandPalette open onClose={onClose} onNavigate={onNavigate} />);
    const input = screen.getByRole("combobox");
    fireEvent.change(input, { target: { value: "季度" } });
    await screen.findByRole("option", { name: /季度成果.xlsx/ });
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(onNavigate).toHaveBeenCalledWith({ type: "session", session, artifactId: "a-1" }));
    expect(onClose).toHaveBeenCalledOnce();
    expect(vi.mocked(invoke).mock.calls.filter(([, args]) => (args as { query?: string }).query === "季度").length).toBeGreaterThanOrEqual(2);
  });
  it("reuses real settings sections without a paid or remote call", async () => {
    vi.mocked(invoke).mockResolvedValue({ results: [], hasMore: false });
    const onNavigate = vi.fn();
    render(<CommandPalette open onClose={vi.fn()} onNavigate={onNavigate} />);
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "API" } });
    await userEvent.click(await screen.findByRole("option", { name: /模型与 API/ }));
    expect(onNavigate).toHaveBeenCalledWith({ type: "settings", settingsSection: "model-api" });
    expect(vi.mocked(invoke).mock.calls.every(([name]) => name === "global_search")).toBe(true);
  });
  it("drops older query responses rather than replacing the current results", async () => {
    let finishOld: (value: unknown) => void = () => {};
    vi.mocked(invoke).mockImplementation((_name, args) => (args as { query: string }).query === "old"
      ? new Promise(resolve => { finishOld = resolve; })
      : Promise.resolve({ results: [{ ...results[0], title: "current result" }], hasMore: false }));
    render(<CommandPalette open onClose={vi.fn()} onNavigate={vi.fn()} />);
    const input = screen.getByRole("combobox");
    fireEvent.change(input, { target: { value: "old" } });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("global_search", { query: "old", limit: 32 }));
    fireEvent.change(input, { target: { value: "new" } });
    await screen.findByRole("option", { name: /current result/ });
    await act(async () => finishOld({ results: [{ ...results[0], title: "stale result" }], hasMore: false }));
    expect(screen.queryByRole("option", { name: /stale result/ })).not.toBeInTheDocument();
  });
  it("does not navigate a result removed during account switch or deletion", async () => {
    const onNavigate = vi.fn();
    render(<CommandPalette open onClose={vi.fn()} onNavigate={onNavigate} />);
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "季度" } });
    const artifact = await screen.findByRole("option", { name: /季度成果.xlsx/ });
    vi.mocked(invoke).mockResolvedValue({ results: [], hasMore: false });
    await userEvent.click(artifact);
    expect(await screen.findByRole("alert")).toHaveTextContent("结果已变化");
    expect(onNavigate).not.toHaveBeenCalled();
  });
  it("closes on Escape and reopens without retaining results or the old query", async () => {
    const onClose = vi.fn();
    const view = render(<CommandPalette open onClose={onClose} onNavigate={vi.fn()} />);
    const input = screen.getByRole("combobox");
    fireEvent.change(input, { target: { value: "季度" } });
    await screen.findByRole("option", { name: /季度成果.xlsx/ });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
    view.rerender(<CommandPalette open={false} onClose={onClose} onNavigate={vi.fn()} />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    vi.mocked(invoke).mockResolvedValue({ results: [], hasMore: false });
    view.rerender(<CommandPalette open onClose={onClose} onNavigate={vi.fn()} />);
    expect(screen.getByRole("combobox")).toHaveValue("");
    expect(screen.queryByRole("option", { name: /季度成果.xlsx/ })).not.toBeInTheDocument();
  });
  it("keeps static settings usable when local IPC fails, with an actionable retry", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("command global_search not found"));
    render(<CommandPalette open onClose={vi.fn()} onNavigate={vi.fn()} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("global_search");
    expect(screen.getByRole("option", { name: /模型与 API/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重新搜索" })).toBeEnabled();
  });
});
