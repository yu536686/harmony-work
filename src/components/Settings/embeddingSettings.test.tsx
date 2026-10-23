import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Settings from "./index";

const mocks = vi.hoisted(() => ({
  state: { settings: { settings: {} as Record<string, unknown> } },
  dispatch: vi.fn(),
  fetchModels: vi.fn(),
  save: vi.fn(),
  invoke: vi.fn(),
  isTauri: vi.fn(),
  translate: (key: string) => key,
}));

vi.mock("react-redux", () => ({
  useSelector: (select: (state: typeof mocks.state) => unknown) => select(mocks.state),
  useDispatch: () => mocks.dispatch,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: mocks.translate }) }));
vi.mock("../../i18n", () => ({ setLanguage: vi.fn() }));
vi.mock("../../store", () => ({ settingsActions: { setSettings: vi.fn(), setConfigured: vi.fn() } }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke, isTauri: mocks.isTauri }));
vi.mock("../../services/tauri", () => ({
  settingsApi: {
    getDefaultWorkspace: () => Promise.resolve("/workspace"),
    fetchModels: mocks.fetchModels,
    save: mocks.save,
  },
  gatewayApi: {
    list: () => Promise.resolve({ channels: [] }),
    connect: () => Promise.resolve({ channels: [] }),
  },
  enterpriseCapabilityApi: { status: () => Promise.resolve({ configured: false, missing_credentials: [] }) },
  readRememberedMarketingUser: () => null,
}));
vi.mock("./AccountSettingsSyncPanel", () => ({ default: () => null }));
vi.mock("../Tools", () => ({ default: () => null }));
vi.mock("../AuditLog", () => ({ default: () => null }));
vi.mock("../Debug", () => ({ default: () => null }));

const catalog = [
  { id: "chat", label: "Chat model", kind: "chat" },
  { id: "bge", label: "BGE M3", kind: "embedding" },
  { id: "qwen-vector", label: "Qwen vector", kind: "embedding" },
];
const originalUserAgent = navigator.userAgent;

function renderSettings(model = "") {
  mocks.state.settings.settings = {
    provider: "harmony", harmony_api_key: "keep-key", memory_embedding_model: model,
    workspace_root: "/keep-workspace", wecom_bot_secret: "keep-secret",
  };
  return render(<Settings theme="violet" setTheme={vi.fn()} initialSection="model-api" />);
}

function menu() {
  return document.querySelector('[data-menu="memory-embedding-model"]') as HTMLElement;
}

async function waitForCatalog() {
  await waitFor(() => expect(mocks.fetchModels).toHaveBeenCalled());
  await waitFor(() => expect(screen.queryByText("正在获取可用向量模型…")).not.toBeInTheDocument());
}

beforeEach(() => {
  vi.clearAllMocks();
  Object.defineProperty(navigator, "userAgent", { configurable: true, value: "Macintosh" });
  mocks.isTauri.mockReturnValue(true);
  mocks.invoke.mockResolvedValue(undefined);
  mocks.fetchModels.mockResolvedValue(catalog);
  mocks.save.mockImplementation(async (settings) => settings);
});

afterEach(() => {
  cleanup();
  Object.defineProperty(navigator, "userAgent", { configurable: true, value: originalUserAgent });
});

describe("Settings embedding model dropdown", () => {
  it("opens the macOS native menu, selects a model, then saves no-model without losing keys or workspace", async () => {
    renderSettings();
    await waitForCatalog();
    expect(within(menu()).getByRole("button", { name: "不使用向量模型" })).toBeVisible();
    expect(document.querySelector("select#memory-embedding-model")).toBeNull();
    fireEvent.click(within(menu()).getByRole("button"));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("show_macos_dropdown", expect.objectContaining({
      menuId: "memory-embedding-model",
      items: [
        expect.objectContaining({ id: "", selected: true }),
        expect.objectContaining({ id: "bge" }),
        expect.objectContaining({ id: "qwen-vector" }),
      ],
    })));
    act(() => window.dispatchEvent(new CustomEvent("harmony:native-dropdown-select", {
      detail: { menuId: "memory-embedding-model", id: "qwen-vector" },
    })));
    expect(within(menu()).getByRole("button", { name: "Qwen vector" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "settings.saveChanges" }));
    await waitFor(() => expect(mocks.save).toHaveBeenCalledWith(expect.objectContaining({
      memory_embedding_model: "qwen-vector", harmony_api_key: "keep-key",
      wecom_bot_secret: "keep-secret", workspace_root: "/keep-workspace",
    })));
    fireEvent.click(within(menu()).getByRole("button"));
    act(() => window.dispatchEvent(new CustomEvent("harmony:native-dropdown-select", {
      detail: { menuId: "memory-embedding-model", id: "" },
    })));
    fireEvent.click(screen.getByRole("button", { name: "settings.saved" }));
    await waitFor(() => expect(mocks.save).toHaveBeenLastCalledWith(expect.objectContaining({
      memory_embedding_model: "", harmony_api_key: "keep-key", workspace_root: "/keep-workspace",
    })));
  });

  it("preserves a saved model on catalog failure and still permits no-model through fallback", async () => {
    mocks.fetchModels.mockRejectedValue(new Error("offline"));
    mocks.invoke.mockRejectedValue(new Error("native bridge unavailable"));
    renderSettings("saved-offline");
    await waitForCatalog();
    expect(screen.getByText("模型列表暂不可用，请检查 Harmony Key 后重试。")).toBeVisible();
    expect(within(menu()).getByRole("button", { name: "saved-offline（已保存）" })).toBeVisible();
    fireEvent.click(within(menu()).getByRole("button"));
    const noModel = await screen.findByRole("option", { name: "不使用向量模型" });
    fireEvent.click(noModel);
    expect(within(menu()).getByRole("button", { name: "不使用向量模型" })).toHaveAttribute("aria-expanded", "false");
  });

  it("retains no-model during explicit catalog refresh on the non-native path", async () => {
    mocks.isTauri.mockReturnValue(false);
    renderSettings("bge");
    await waitForCatalog();
    fireEvent.click(within(menu()).getByRole("button"));
    fireEvent.click(screen.getByRole("option", { name: "不使用向量模型" }));
    fireEvent.click(screen.getAllByRole("button", { name: "settings.fetchModels" })[0]);
    await waitFor(() => expect(mocks.fetchModels).toHaveBeenCalledTimes(2));
    expect(within(menu()).getByRole("button", { name: "不使用向量模型" })).toBeVisible();
    expect(mocks.invoke).not.toHaveBeenCalled();
  });
});
