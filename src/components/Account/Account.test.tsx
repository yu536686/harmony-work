import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import AccountPage from "./index";
import type { MarketingUser } from "../../services/tauri";

const api = vi.hoisted(() => ({
  accountOverview: vi.fn(),
  status: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  marketingApi: { accountOverview: api.accountOverview },
  accountSettingsSyncApi: { status: api.status },
}));

const activeUser: MarketingUser = {
  id: "user-1",
  name: "测试用户",
  email: "tester@example.com",
  userRole: "user",
  approvalStatus: "active",
};

beforeEach(() => {
  api.accountOverview.mockReset();
  api.status.mockReset();
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
  it("shows identity, balance and a masked API key by default", async () => {
    render(<AccountPage user={activeUser} onUserChange={vi.fn()} />);

    await waitFor(() => expect(screen.getByText("测试用户")).toBeInTheDocument());
    expect(screen.getByText("tester@example.com")).toBeInTheDocument();
    expect(screen.getByText("12.50")).toBeInTheDocument();

    // Key must be masked and must not expose the full secret.
    expect(screen.queryByText("sk-harmony-secret")).not.toBeInTheDocument();
  });

  it("reveals the API key only after the user explicitly clicks show", async () => {
    render(<AccountPage user={activeUser} onUserChange={vi.fn()} />);

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

    render(<AccountPage user={activeUser} onUserChange={vi.fn()} />);

    await waitFor(() => expect(screen.getByText("8.00")).toBeInTheDocument());
    expect(screen.getByText("测试用户")).toBeInTheDocument();
    expect(screen.getByText("未配置")).toBeInTheDocument();
  });

  it("clears the expired session and shows a login prompt on 401/403", async () => {
    api.accountOverview.mockRejectedValue("登录已过期（HTTP 401），请重新登录");
    const onUserChange = vi.fn();

    render(<AccountPage user={activeUser} onUserChange={onUserChange} />);

    await waitFor(() => expect(screen.getByText(/登录已过期/)).toBeInTheDocument());
    expect(onUserChange).toHaveBeenCalledWith(null);
  });
});
