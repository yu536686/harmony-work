import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Settings from "./index";
import { mainChatCapability } from "../../utils/providerCapabilities";

const mocks = vi.hoisted(() => ({
  state: { settings: { settings: {} as Record<string, unknown> } },
  dispatch: vi.fn(),
  fetchModels: vi.fn(),
  save: vi.fn(),
  get: vi.fn(),
  startLogin: vi.fn(),
  invoke: vi.fn(),
  isTauri: vi.fn(),
  diagnosticCheck: vi.fn(),
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
    get: mocks.get,
  },
  wechatApi: { startLogin: mocks.startLogin },
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
vi.mock("../../services/runtimeDiagnostics", () => ({
  runtimeDiagnostics: { check: mocks.diagnosticCheck, repair: vi.fn(), export: vi.fn() },
}));

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
  mocks.save.mockImplementation(async (patch) => ({ ...mocks.state.settings.settings, ...patch }));
  mocks.diagnosticCheck.mockResolvedValue({ schema_version: 1, checked_at: "2026-10-26T00:00:00Z", platform: "test", healthy: true, items: [] });
});

describe("Settings main chat provider capabilities", () => {
  const unsupported = mainChatCapability("deepseek").message;
  const legacyNamed = {
    id: "legacy", label: "Legacy", provider: "deepseek", model: "old-model",
    enabled_models: ["old-model"], available_models: ["old-model", "new-model"],
    api_key: "keep-named-secret", base_url: "", max_tokens: 0,
  };
  function selectFor(label: string) {
    return screen.getByText(label, { selector: "label" }).parentElement!.querySelector("select")!;
  }
  function renderLegacy() {
    mocks.state.settings.settings = {
      provider: "deepseek", model: "old-model", enabled_models: ["old-model"],
      available_models: ["old-model", "new-model"], deepseek_api_key: "keep-main-secret",
      workspace_root: "/workspace", language: "zh", vision_use_main_llm: false,
      llm_providers: [legacyNamed],
    };
    return render(<Settings theme="violet" setTheme={vi.fn()} initialSection="model-api" />);
  }

  it("disables unsupported main/named provider options with the shared message, not vision options", async () => {
    renderLegacy();
    const main = selectFor("settings.provider");
    expect(within(main).getByRole("option", { name: /settings.providerDeepseek$/ })).toBeDisabled();
    expect(within(main).getByRole("option", { name: /settings.providerDeepseek$/ })).toHaveAttribute("title", unsupported);
    expect(screen.getByText(unsupported)).toBeVisible();
    expect(within(main).getByRole("option", { name: /settings.providerOpenai$/ })).toBeEnabled();
    expect(within(selectFor("settings.visionProvider")).getByRole("option", { name: "settings.providerDeepseek" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "settings.namedLlmAddBtn" }));
    expect(within(selectFor("settings.namedLlmProviderTypeLabel")).getByRole("option", { name: /settings.providerDeepseek$/ })).toBeDisabled();
  });

  it("rejects a changed model on an unsupported legacy main provider before invoking save", async () => {
    renderLegacy();
    fireEvent.change(selectFor("settings.defaultModel"), { target: { value: "new-model" } });
    fireEvent.click(screen.getByRole("button", { name: "settings.saveChanges" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(unsupported));
    expect(mocks.save).not.toHaveBeenCalled();
  });

  it("rejects a programmatically selected unsupported main provider at the save boundary", async () => {
    renderSettings();
    fireEvent.change(selectFor("settings.provider"), { target: { value: "deepseek" } });
    fireEvent.click(screen.getByRole("button", { name: "settings.saveChanges" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(unsupported));
    expect(mocks.save).not.toHaveBeenCalled();
  });

  it("permits an unrelated language save while preserving unsupported legacy routes and secrets", async () => {
    renderLegacy();
    fireEvent.click(screen.getByRole("tab", { name: "外观与交互" }));
    fireEvent.change(selectFor("settings.language"), { target: { value: "en" } });
    fireEvent.click(screen.getByRole("button", { name: "settings.saveChanges" }));
    await waitFor(() => expect(mocks.save).toHaveBeenCalledWith({ language: "en" }));
  });

  it("rejects adding a new unsupported named provider", async () => {
    renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "settings.namedLlmAddBtn" }));
    fireEvent.change(screen.getByPlaceholderText("my-gpt4"), { target: { value: "new-provider" } });
    fireEvent.change(selectFor("settings.namedLlmProviderTypeLabel"), { target: { value: "deepseek" } });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(unsupported));
    expect(screen.getByRole("button", { name: "common.save" })).toBeInTheDocument();
    expect(mocks.save).not.toHaveBeenCalled();
  });

  it("permits renaming a legacy named provider without clearing its keep-blank Key", async () => {
    renderLegacy();
    fireEvent.click(screen.getByRole("button", { name: "settings.namedLlmEdit" }));
    fireEvent.change(screen.getByPlaceholderText("settings.namedLlmLabelPlaceholder"), { target: { value: "Renamed" } });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    fireEvent.click(screen.getByRole("button", { name: "settings.saveChanges" }));
    await waitFor(() => expect(mocks.save).toHaveBeenCalled());
    const provider = mocks.save.mock.calls[0][0].llm_providers[0];
    expect(provider.label).toBe("Renamed");
    expect(provider).not.toHaveProperty("api_key");
  });
});

afterEach(() => {
  cleanup();
  Object.defineProperty(navigator, "userAgent", { configurable: true, value: originalUserAgent });
});

describe("Settings embedding model dropdown", () => {
  it("mounts Diagnostics first only in its active section and unmounts it on leaving", async () => {
    renderSettings();
    expect(screen.queryByRole("region", { name: "环境自检" })).not.toBeInTheDocument();
    expect(mocks.diagnosticCheck).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("tab", { name: "系统与诊断" }));
    const diagnostics = await screen.findByRole("region", { name: "环境自检" });
    expect(document.getElementById("settings-section-system-diagnostics")?.firstElementChild).toBe(diagnostics);
    await waitFor(() => expect(mocks.diagnosticCheck).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("tab", { name: "模型与 API" }));
    expect(screen.queryByRole("region", { name: "环境自检", hidden: true })).not.toBeInTheDocument();
    expect(mocks.diagnosticCheck).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("tab", { name: "系统与诊断" }));
    await waitFor(() => expect(mocks.diagnosticCheck).toHaveBeenCalledTimes(2));
  });

  it("keeps an explicit WeChat clear when clear and save share a React batch", async () => {
    mocks.state.settings.settings = { workspace_root: "/workspace", wechat_enabled: true, wechat_bot_token: "bound", wechat_bot_id: "bot" };
    render(<Settings theme="violet" setTheme={vi.fn()} initialSection="channels" />);
    const clear = await screen.findByRole("button", { name: "清除微信凭据" });
    const save = screen.getByRole("button", { name: "settings.saveChanges" });
    act(() => {
      fireEvent.click(clear);
      fireEvent.click(save);
    });
    await waitFor(() => expect(mocks.save).toHaveBeenCalledWith(expect.objectContaining({
      wechat_enabled: false, wechat_bot_token: null, wechat_bot_id: "",
    })));
  });

  it("explicitly clears bound WeChat credentials with null rather than a stale blank", async () => {
    mocks.state.settings.settings = { workspace_root: "/workspace", wechat_enabled: true, wechat_bot_token: "bound", wechat_bot_id: "bot" };
    render(<Settings theme="violet" setTheme={vi.fn()} initialSection="channels" />);
    fireEvent.click(await screen.findByRole("button", { name: "清除微信凭据" }));
    fireEvent.click(screen.getByRole("button", { name: "settings.saveChanges" }));
    await waitFor(() => expect(mocks.save).toHaveBeenCalledWith(expect.objectContaining({
      wechat_enabled: false, wechat_bot_token: null, wechat_bot_id: "",
    })));
  });

  it("refreshes bound WeChat credentials and never sends the stale empty token", async () => {
    mocks.state.settings.settings = { workspace_root: "/workspace", wechat_enabled: true, wechat_bot_token: "" };
    mocks.startLogin.mockResolvedValue({ connected: true });
    mocks.get.mockResolvedValue({ ...mocks.state.settings.settings, wechat_bot_token: "fresh-token", wechat_bot_id: "bound-bot" });
    render(<Settings theme="violet" setTheme={vi.fn()} initialSection="channels" />);
    fireEvent.click(await screen.findByRole("button", { name: "settings.wechatBindBtn" }));
    await waitFor(() => expect(mocks.get).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "settings.saveChanges" }));
    await waitFor(() => expect(mocks.save).toHaveBeenCalled());
    expect(mocks.save.mock.calls[0][0]).not.toHaveProperty("wechat_bot_token");
  });

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
    await waitFor(() => expect(mocks.save).toHaveBeenCalledWith({ memory_embedding_model: "qwen-vector" }));
    fireEvent.click(within(menu()).getByRole("button"));
    act(() => window.dispatchEvent(new CustomEvent("harmony:native-dropdown-select", {
      detail: { menuId: "memory-embedding-model", id: "" },
    })));
    fireEvent.click(screen.getByRole("button", { name: "settings.saved" }));
    await waitFor(() => expect(mocks.save).toHaveBeenLastCalledWith({ memory_embedding_model: "" }));
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
