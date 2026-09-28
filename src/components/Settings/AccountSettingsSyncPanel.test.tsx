import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import AccountSettingsSyncPanel from "./AccountSettingsSyncPanel";
import type { MarketingUser } from "../../services/tauri";

const api = vi.hoisted(() => ({
  catalog: vi.fn(),
  preview: vi.fn(),
  backupSelected: vi.fn(),
  restoreSelected: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  accountSettingsSyncApi: api,
}));

const activeUser: MarketingUser = {
  id: "user-1",
  name: "测试用户",
  email: "tester@example.com",
  userRole: "user",
  accountType: "standard",
  approvalStatus: "active",
};

beforeEach(() => {
  api.catalog.mockReset();
  api.preview.mockReset();
  api.backupSelected.mockReset();
  api.restoreSelected.mockReset();
  api.catalog.mockResolvedValue({
    snapshot: { revision: 3, schemaVersion: 2, updatedAt: "2026-09-01T12:00:00Z", updatedDeviceName: "Windows WorkBuddy" },
    localOnlyItems: ["工作区与工作区外文件访问权限"],
    categories: [{
      id: "aiProvider",
      label: "AI 提供商与模型",
      description: "默认提供商、模型、API Key。",
      configuredFields: ["DeepSeek API Key"],
      fields: [{ label: "DeepSeek API Key", configured: true, sensitive: true }],
    }],
  });
  api.preview.mockResolvedValue({
    snapshot: { revision: 3, schemaVersion: 2, updatedAt: "2026-09-01T12:00:00Z", updatedDeviceName: "Windows WorkBuddy" },
    categories: [{
      id: "aiProvider",
      label: "AI 提供商与模型",
      description: "默认提供商、模型、API Key。",
      configuredFields: ["DeepSeek API Key"],
      fields: [{ label: "DeepSeek API Key", configured: true, sensitive: true }],
    }],
  });
});

describe("AccountSettingsSyncPanel", () => {
  it("enables selected actions only after a category is chosen and masks sensitive values", async () => {
    render(<AccountSettingsSyncPanel user={activeUser} onSettingsApplied={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "查看云端配置" }));

    await waitFor(() => expect(screen.getByLabelText("AI 提供商与模型")).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "上传所选配置" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "恢复所选配置" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "展开 AI 提供商与模型 配置项" }));
    expect(screen.getByText("DeepSeek API Key")).toBeInTheDocument();
    expect(screen.queryByText("sk-secret")).not.toBeInTheDocument();

    fireEvent.click(screen.getByLabelText("AI 提供商与模型"));

    expect(screen.getByRole("button", { name: "上传所选配置" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "恢复所选配置" })).toBeEnabled();
  });
});
