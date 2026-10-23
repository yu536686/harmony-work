import { act, fireEvent, render, screen, waitFor, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { UpdateAbout, UpdateBadge } from "./UpdateControls";
import { createUpdateStore, type UpdateSnapshot } from "./updateStore";

const initial: UpdateSnapshot = { phase: "idle", currentVersion: "1.1.1", channel: "stable", available: null, downloaded: 0, total: null, error: null, nextCheckAt: null };
const release: UpdateSnapshot = { ...initial, phase: "available", available: { version: "1.1.2-rc.1", notes: "<script>unsafe</script>\n覆盖升级验证", date: null, downloadUrl: "https://download.harmonywatch.cn/releases/test.exe" } };
afterEach(cleanup);
function setup() {
  let event: (s: UpdateSnapshot) => void = () => {};
  let result = initial;
  const calls: string[] = [];
  const store = createUpdateStore({
    listen: async (fn) => { event = fn; return () => {}; },
    invoke: async <T,>(command: string) => { calls.push(command); return (command === "update_check" ? result : initial) as T; },
  });
  return { store, calls, emit: (s: UpdateSnapshot) => act(() => event(s)), result: (s: UpdateSnapshot) => { result = s; } };
}
describe("update entry points", () => {
  it("manual check reports latest and opens release details when available", async () => {
    const h = setup(); render(<UpdateAbout store={h.store} enabled />);
    await waitFor(() => expect(h.calls).toContain("update_status"));
    fireEvent.click(screen.getByRole("button", { name: "检查更新" }));
    await screen.findByText("已是最新版本");
    h.result(release); fireEvent.click(screen.getByRole("button", { name: "检查更新" }));
    await screen.findByRole("dialog");
    expect(screen.getByText(/<script>unsafe<\/script>\s+覆盖升级验证/)).toBeInTheDocument();
    expect(document.querySelector("script")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "立即更新" }));
    await waitFor(() => expect(h.calls).toContain("update_install"));
  });
  it("background availability shows badge without opening a card; cancel retains badge", async () => {
    const h = setup(); render(<UpdateBadge store={h.store} enabled />);
    await waitFor(() => expect(h.calls).toContain("update_status"));
    h.emit(release);
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "可用更新" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("button", { name: "可用更新" })).toBeInTheDocument();
  });
  it("displays progress and prevents duplicate installation", async () => {
    const h = setup(); render(<UpdateBadge store={h.store} enabled />);
    await waitFor(() => expect(h.calls).toContain("update_status"));
    h.emit({ ...release, phase: "downloading", downloaded: 50, total: 100 });
    fireEvent.click(screen.getByRole("button", { name: "可用更新" }));
    expect(screen.getByRole("progressbar")).toHaveAttribute("value", "50");
    expect(screen.queryByRole("button", { name: "立即安装" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "取消下载" }));
    await waitFor(() => expect(h.calls).toContain("update_cancel"));
  });
  it("blocks another installation while the native pre-download check runs", async () => {
    const h = setup(); render(<UpdateBadge store={h.store} enabled />);
    await waitFor(() => expect(h.calls).toContain("update_status"));
    h.emit({ ...release, phase: "checking" });
    fireEvent.click(screen.getByRole("button", { name: "可用更新" }));
    expect(screen.queryByRole("button", { name: "立即安装" })).toBeNull();
  });
  it("keeps keyboard focus in the dialog during installation handoff", async () => {
    const h = setup(); render(<UpdateBadge store={h.store} enabled />);
    await waitFor(() => expect(h.calls).toContain("update_status"));
    h.emit(release); fireEvent.click(screen.getByRole("button", { name: "可用更新" }));
    h.emit({ ...release, phase: "installing" });
    fireEvent.keyDown(document, { key: "Tab" });
    expect(screen.getByRole("dialog")).toHaveFocus();
  });
});
