import { describe, expect, it } from "vitest";
import type { LlmProviderConfig, Settings } from "../../services/tauri";
import { mainChatSelectionError, mainChatSettingsError } from "./mainChatSettings";

describe("Settings main chat save guard", () => {
  const legacy = { provider: "deepseek", model: "old", enabled_models: [], custom_base_url: "" };
  const named = { id: "legacy", label: "Legacy", provider: "deepseek", model: "old", enabled_models: ["old"], api_key: "keep", base_url: "" } as LlmProviderConfig;

  it("uses the shared Responses matrix for new configurations", () => {
    for (const provider of ["openai", "harmony", "custom"]) {
      expect(mainChatSelectionError(undefined, { provider, model: "m" })).toBeNull();
    }
    for (const provider of ["deepseek", "anthropic", "qwen", "minimax", "zhipu", "kimi", "siliconflow", "unknown"]) {
      expect(mainChatSelectionError(undefined, { provider, model: "m" })).toMatch(/Responses/);
    }
  });

  it("preserves legacy routes during metadata and credential maintenance", () => {
    const previous: Partial<Settings> = { ...legacy, deepseek_api_key: "keep", language: "zh", llm_providers: [named] };
    expect(mainChatSettingsError(previous, { ...previous, language: "en", deepseek_api_key: "rotated" })).toBeNull();
    expect(mainChatSettingsError(previous, { ...previous, llm_providers: [{ ...named, label: "Renamed", api_key: "rotated" }] })).toBeNull();
    expect(mainChatSettingsError(previous, { ...previous, llm_providers: [] })).toBeNull();
  });

  it("does not confuse model-set normalization with an unsupported new selection", () => {
    expect(mainChatSelectionError(legacy, { ...legacy, enabled_models: ["old", "old"] })).toBeNull();
    expect(mainChatSelectionError({ ...legacy, enabled_models: ["a", "b"] }, { ...legacy, enabled_models: ["b", "old", "a"] })).toBeNull();
    expect(mainChatSelectionError(legacy, { ...legacy, enabled_models: ["new"] })).toMatch(/Responses/);
  });

  it("rejects changed unsupported routes at the top-level save boundary", () => {
    const previous: Partial<Settings> = { provider: "harmony", llm_providers: [named] };
    expect(mainChatSettingsError(previous, { ...previous, llm_providers: [{ ...named, model: "new" }] })).toContain("Legacy:");
    expect(mainChatSettingsError(previous, { ...previous, llm_providers: [named, { ...named, id: "new" }] })).toMatch(/Responses/);
    expect(mainChatSelectionError(legacy, { ...legacy, custom_base_url: "https://different.example/v1" })).toMatch(/Responses/);
    expect(mainChatSelectionError(legacy, { ...legacy, provider: "custom", custom_base_url: "https://gateway.example/v1" })).toBeNull();
  });

  it("does not impose the main-chat protocol on independent vision or embedding settings", () => {
    const previous: Partial<Settings> = { ...legacy, vision_provider: "openai", vision_model: "old-vision" };
    expect(mainChatSettingsError(previous, { ...previous, vision_provider: "qwen", vision_model: "new-vision", memory_embedding_model: "vector" })).toBeNull();
  });
});
