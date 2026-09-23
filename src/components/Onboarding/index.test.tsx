import { Provider } from "react-redux";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { store } from "../../store";
import Onboarding from "./index";

const api = vi.hoisted(() => ({
  get: vi.fn(), save: vi.fn(), getDefaultWorkspace: vi.fn(), status: vi.fn(), restore: vi.fn(), open: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: api.open }));
vi.mock("../../services/tauri", () => ({
  settingsApi: { get: api.get, save: api.save, getDefaultWorkspace: api.getDefaultWorkspace, fetchModels: vi.fn() },
  accountSettingsSyncApi: { status: api.status, restore: api.restore },
  readRememberedMarketingUser: () => null,
  openPath: vi.fn(),
}));

beforeEach(() => {
  vi.clearAllMocks();
  api.get.mockResolvedValue({
    workspace_root: "C:\\Users\\test\\harmony",
    allow_outside_workspace: false,
    provider: "harmony",
    model: "deepseek-v4-flash",
    enabled_models: ["deepseek-v4-flash"],
    harmony_api_key: "",
  });
  api.getDefaultWorkspace.mockResolvedValue("C:\\Users\\test\\harmony");
  api.save.mockImplementation(async (updates) => ({ ...(await api.get()), ...updates }));
});

describe("first-run onboarding", () => {
  it("keeps API configuration visible and saves workspace restrictions after skipping the key", async () => {
    render(<Provider store={store}><Onboarding onComplete={vi.fn()} /></Provider>);
    await userEvent.click(screen.getByRole("button", { name: /开始使用/ }));
    expect(screen.getByText("配置 AI 提供商")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "同步云端配置" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "跳过" }));
    expect(screen.getByRole("textbox", { name: "工作区文件夹" })).toHaveValue("C:\\Users\\test\\harmony");
    expect(screen.getByText(/先把要处理的文件放入工作区文件夹/)).toBeInTheDocument();
    api.open.mockResolvedValue("D:\\HarmonyWork");
    await userEvent.click(screen.getByRole("button", { name: "选择文件夹" }));
    expect(screen.getByRole("textbox", { name: "工作区文件夹" })).toHaveValue("D:\\HarmonyWork");
    await userEvent.click(screen.getByRole("checkbox", { name: "允许读取/写入工作区外的文件" }));
    expect(screen.getByText(/可能读取或改动工作区外的文件/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /保存并继续/ }));
    await waitFor(() => expect(api.save).toHaveBeenCalledWith(expect.objectContaining({
      workspace_root: "D:\\HarmonyWork",
      allow_outside_workspace: true,
    })));
    expect(api.save.mock.calls[0][0]).not.toHaveProperty("harmony_api_key");
  });

  it("saves the API key when the user returns after skipping", async () => {
    render(<Provider store={store}><Onboarding onComplete={vi.fn()} /></Provider>);
    await userEvent.click(screen.getByRole("button", { name: /开始使用/ }));
    await userEvent.click(screen.getByRole("button", { name: "跳过" }));
    await userEvent.click(screen.getByRole("button", { name: /返回/ }));
    await userEvent.type(screen.getByPlaceholderText("sk-..."), "sk-test-key");
    await userEvent.click(screen.getByRole("button", { name: /下一步/ }));
    await userEvent.click(screen.getByRole("button", { name: /保存并继续/ }));
    await waitFor(() => expect(api.save).toHaveBeenCalledWith(expect.objectContaining({
      provider: "harmony",
      harmony_api_key: "sk-test-key",
    })));
  });
});
