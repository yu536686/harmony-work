import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import FileReview from "./index";

const modified = {
  path: "src/a.ts", kind: "modified", before: { size: 4, hash: "before", binary: false, skipped: null },
  after: { size: 5, hash: "after", binary: false, skipped: null }, expectedAfterHash: "after", canRestore: true, restored: false,
};
const response = () => ({
  turnKey: "turn-1", status: "complete", turns: [{ turnKey: "turn-1", status: "complete" }], warnings: [],
  changes: [modified, { ...modified, path: "new.txt", kind: "added", before: null }, { ...modified, path: "gone.txt", kind: "deleted", after: null, expectedAfterHash: "absent" }],
});
const textDiff = { kind: "text", beforeText: "old\nsame\n", afterText: "new\nsame\n", before: modified.before, after: modified.after, message: null };

describe("FileReview", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation(async (name) => {
      if (name === "journal_review") return response();
      if (name === "journal_diff") return textDiff;
      if (name === "journal_restore_file") return undefined;
      throw new Error(`Unexpected IPC: ${name}`);
    });
  });

  it("lists real add/delete/modify changes and loads only the selected diff", async () => {
    render(<FileReview sessionId="s" workspaceRoot="/workspace" />);
    await screen.findByRole("button", { name: /src\/a.ts/ });
    expect(screen.getByRole("button", { name: /new.txt/ })).toHaveTextContent("新增");
    expect(screen.getByRole("button", { name: /gone.txt/ })).toHaveTextContent("删除");
    expect(invoke).not.toHaveBeenCalledWith("journal_diff", expect.anything());
    await userEvent.click(screen.getByRole("button", { name: /src\/a.ts/ }));
    expect(await screen.findByText("old")).toBeInTheDocument();
    expect(screen.getByText("new")).toBeInTheDocument();
    expect(vi.mocked(invoke).mock.calls.filter(([name]) => name === "journal_diff")).toHaveLength(1);
    expect(invoke).toHaveBeenCalledWith("journal_diff", { sessionId: "s", workspaceRoot: "/workspace", turnKey: "turn-1", path: "src/a.ts" });
  });

  it("requires explicit per-file confirmation, supports cancel, and sends immutable hash", async () => {
    render(<FileReview sessionId="s" workspaceRoot="/workspace" />);
    await userEvent.click(await screen.findByRole("button", { name: /src\/a.ts/ }));
    await screen.findByText("old");
    await userEvent.click(screen.getByRole("button", { name: "恢复此文件" }));
    expect(invoke).not.toHaveBeenCalledWith("journal_restore_file", expect.anything());
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "取消" }));
    await userEvent.click(screen.getByRole("button", { name: "恢复此文件" }));
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "确认恢复" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("journal_restore_file", {
      sessionId: "s", workspaceRoot: "/workspace", turnKey: "turn-1", path: "src/a.ts", expectedAfterHash: "after", confirmed: true,
    }));
    expect(await screen.findByRole("status")).toHaveTextContent("已恢复");
  });

  it("preserves conflicts and offers refresh instead of force restore", async () => {
    vi.mocked(invoke).mockImplementation(async (name) => {
      if (name === "journal_restore_file") throw new Error("CONFLICT: later edits preserved");
      return name === "journal_review" ? response() : textDiff;
    });
    render(<FileReview sessionId="s" workspaceRoot="/workspace" />);
    await userEvent.click(await screen.findByRole("button", { name: /src\/a.ts/ }));
    await screen.findByText("old");
    await userEvent.click(screen.getByRole("button", { name: "恢复此文件" }));
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "确认恢复" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("later edits preserved");
    expect(screen.getByRole("button", { name: "刷新变更" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: /强制/ })).not.toBeInTheDocument();
  });

  it("shows binary summary without rendering text diff", async () => {
    vi.mocked(invoke).mockImplementation(async (name) => name === "journal_review" ? response() : { ...textDiff, kind: "binary", beforeText: null, afterText: null, message: "Binary file" });
    render(<FileReview sessionId="s" workspaceRoot="/workspace" />);
    await userEvent.click(await screen.findByRole("button", { name: /src\/a.ts/ }));
    expect(await screen.findByText(/二进制/)).toBeInTheDocument();
    expect(screen.queryByText("old")).not.toBeInTheDocument();
    expect(screen.getByText(/4 B/)).toBeInTheDocument();
  });

  it("discards stale asynchronous lists and diffs after scope changes", async () => {
    let resolveOld: (value: unknown) => void = () => {};
    vi.mocked(invoke).mockImplementation((name, args) => {
      if ((args as { sessionId: string }).sessionId === "old") return new Promise(resolve => { resolveOld = resolve; });
      return Promise.resolve(name === "journal_review" ? { ...response(), changes: [{ ...modified, path: "current" }] } : textDiff);
    });
    const view = render(<FileReview sessionId="old" workspaceRoot="/old" />);
    view.rerender(<FileReview sessionId="new" workspaceRoot="/new" />);
    await screen.findByRole("button", { name: /current/ });
    resolveOld(response());
    await waitFor(() => expect(screen.queryByRole("button", { name: /src\/a.ts/ })).not.toBeInTheDocument());
  });

  it("displays actionable load failures and retries", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("WORKSPACE_MISMATCH: reopen workspace"));
    render(<FileReview sessionId="s" workspaceRoot="/workspace" />);
    expect(await screen.findByRole("alert")).toHaveTextContent("reopen workspace");
    fireEvent.click(screen.getByRole("button", { name: "刷新变更" }));
    await screen.findByRole("button", { name: /src\/a.ts/ });
  });

  it("ignores a slow diff when another file is selected", async () => {
    let finishFirst: (value: unknown) => void = () => {};
    vi.mocked(invoke).mockImplementation((name, args) => {
      if (name === "journal_review") return Promise.resolve(response());
      if ((args as { path: string }).path === "src/a.ts") return new Promise(resolve => { finishFirst = resolve; });
      return Promise.resolve({ ...textDiff, beforeText: "", afterText: "selected file" });
    });
    render(<FileReview sessionId="s" workspaceRoot="/workspace" />);
    await userEvent.click(await screen.findByRole("button", { name: /src\/a.ts/ }));
    await userEvent.click(screen.getByRole("button", { name: /new.txt/ }));
    await screen.findByText("selected file");
    finishFirst(textDiff);
    await waitFor(() => expect(screen.queryByText("old")).not.toBeInTheDocument());
    expect(screen.getByText("selected file")).toBeInTheDocument();
  });

  it("shows durable capture-unavailable state instead of claiming an empty changeset", async () => {
    vi.mocked(invoke).mockResolvedValue({
      ...response(), status: "unavailable", changes: [], warnings: [],
      captureError: "FILE_LIMIT: narrow workspace", captureActive: false,
    });
    render(<FileReview sessionId="s" workspaceRoot="/workspace" />);
    expect(await screen.findByRole("alert")).toHaveTextContent("FILE_LIMIT");
    expect(screen.getByText("本轮文件审阅不可用")).toBeInTheDocument();
    expect(screen.queryByText("暂无可审阅的文件变更")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "刷新变更" })).toBeEnabled();
  });
});
