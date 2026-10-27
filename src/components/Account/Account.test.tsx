import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Provider } from "react-redux";
import { beforeEach, describe, expect, it, vi } from "vitest";
import AccountPage from "./index";
import type { MarketingUser } from "../../services/tauri";
import { store } from "../../store";

const api = vi.hoisted(() => ({
  accountOverview: vi.fn(),
  status: vi.fn(),
  save: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  marketingApi: { accountOverview: api.accountOverview },
  accountSettingsSyncApi: { status: api.status },
  settingsApi: { save: api.save },
}));

function renderAccount(user: MarketingUser | null, onUserChange = vi.fn()) {
  return render(<Provider store={store}><AccountPage user={user} onUserChange={onUserChange} onOpenLogin={vi.fn()} onOpenSettings={vi.fn()} /></Provider>);
}

const activeUser: MarketingUser = {
  id: "user-1",
  name: "测试用户",
  email: "tester@example.com",
  userRole: "user",
  accountType: "standard",
  approvalStatus: "active",
};

beforeEach(() => {
  api.accountOverview.mockReset();
  api.status.mockReset();
  api.save.mockReset();
  api.accountOverview.mockResolvedValue({
    user: activeUser,
    balance: 12.5,
    usage: { quota: 500000, rpm: 3, tpm: 2400 },
    gatewayApiKey: "sk-harmony-secret",
  });
  api.status.mockResolvedValue({
    snapshot: { revision: 3, schemaVersion: 2, updatedAt: "2026-09-17T12:00:00Z", updatedDeviceName: "Windows WorkBuddy" },
    locallyConfigured: true,
  });
});

describe("AccountPage", () => {
  const otherUser = { ...activeUser, id: "user-2", name: "Other" };
  function switchTo(view: ReturnType<typeof renderAccount>, user: MarketingUser | null, onUserChange = vi.fn()) {
    view.rerender(<Provider store={store}><AccountPage user={user} onUserChange={onUserChange} onOpenLogin={vi.fn()} onOpenSettings={vi.fn()} /></Provider>);
  }

  it("immediately removes revealed secrets and confirmation on account switch, even offline", async () => {
    const view = renderAccount(activeUser);
    fireEvent.click(await screen.findByRole("button", { name: "查看 API Key" }));
    fireEvent.click(screen.getByRole("button", { name: "用于本机" }));
    api.accountOverview.mockRejectedValue(new Error("offline"));
    switchTo(view, otherUser);
    expect(screen.queryByText("sk-harmony-secret")).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await screen.findByText(/offline/);
    expect(screen.getByRole("button", { name: "复制 Key" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "用于本机" })).toBeDisabled();
  });

  it("discards an old overview after A logout and B login", async () => {
    let resolve!: (value: unknown) => void;
    api.accountOverview.mockReturnValueOnce(new Promise(r => { resolve = r; }));
    const view = renderAccount(activeUser);
    switchTo(view, null);
    api.accountOverview.mockResolvedValueOnce({ user: otherUser, balance: 2, gatewayApiKey: "sk-B" });
    switchTo(view, otherUser);
    await screen.findByText("2.00");
    await act(async () => resolve({ user: activeUser, balance: 999, gatewayApiKey: "sk-A" }));
    expect(screen.queryByText("999.00")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "查看 API Key" }));
    expect(screen.getByText("sk-B")).toBeInTheDocument();
  });

  it("does not log B out when an old A request rejects with 401", async () => {
    let reject!: (reason: unknown) => void;
    api.accountOverview.mockReturnValueOnce(new Promise((_, r) => { reject = r; }));
    const onUserChange = vi.fn();
    const view = renderAccount(activeUser, onUserChange);
    api.accountOverview.mockResolvedValueOnce({ user: otherUser, balance: 2, gatewayApiKey: "sk-B" });
    switchTo(view, otherUser, onUserChange);
    await screen.findByText("2.00");
    await act(async () => reject("HTTP 401"));
    expect(onUserChange).not.toHaveBeenCalled();
  });

  it("rejects a response whose account identity does not match", async () => {
    renderAccount(otherUser);
    await waitFor(() => expect(screen.queryByText("加载账户数据…")).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: "复制 Key" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "用于本机" })).toBeDisabled();
  });

  it("shows identity, balance and a masked API key by default", async () => {
    renderAccount(activeUser);

    await waitFor(() => expect(screen.getByText("测试用户")).toBeInTheDocument());
    expect(screen.getByText("tester@example.com")).toBeInTheDocument();
    expect(screen.getByText("12.50")).toBeInTheDocument();

    // Key must be masked and must not expose the full secret.
    expect(screen.queryByText("sk-harmony-secret")).not.toBeInTheDocument();
  });

  it("reveals the API key only after the user explicitly clicks show", async () => {
    renderAccount(activeUser);

    await waitFor(() => expect(screen.getByRole("button", { name: "查看 API Key" })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "查看 API Key" }));

    expect(screen.getByText("sk-harmony-secret")).toBeInTheDocument();
  });

  it("keeps identity and balance visible when a non-critical stat fails", async () => {
    api.accountOverview.mockResolvedValue({
      user: activeUser,
      balance: 8,
      usage: null,
      gatewayApiKey: null,
    });

    renderAccount(activeUser);

    await waitFor(() => expect(screen.getByText("8.00")).toBeInTheDocument());
    expect(screen.getByText("测试用户")).toBeInTheDocument();
    expect(screen.getByText("未配置")).toBeInTheDocument();
  });

  it("clears the expired session and shows a login prompt on 401/403", async () => {
    api.accountOverview.mockRejectedValue("登录已过期（HTTP 401），请重新登录");
    const onUserChange = vi.fn();

    renderAccount(activeUser, onUserChange);

    await waitFor(() => expect(screen.getByText(/登录已过期/)).toBeInTheDocument());
    expect(onUserChange).toHaveBeenCalledWith(null);
  });

  it("requires confirmation before applying the Harmony key locally", async () => {
    api.save.mockResolvedValue({ provider: "harmony", harmony_api_key: "sk-harmony-secret" });
    renderAccount(activeUser);
    await waitFor(() => expect(screen.getByRole("button", { name: "用于本机" })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "用于本机" }));
    expect(api.save).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "确认使用" }));
    await waitFor(() => expect(api.save).toHaveBeenCalledWith(expect.objectContaining({ provider: "harmony", harmony_api_key: "sk-harmony-secret" })));
  });
});
