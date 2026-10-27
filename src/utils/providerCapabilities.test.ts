import { describe, expect, it } from "vitest";
import { mainChatCapability } from "./providerCapabilities";

describe("main chat provider contract", () => {
  it("does not advertise Messages or Chat Completions endpoints as Responses", () => {
    for (const provider of ["anthropic", "qwen", "minimax", "zhipu", "kimi", "siliconflow", "deepseek"]) {
      expect(mainChatCapability(provider).selectable).toBe(false);
      expect(mainChatCapability(provider).message).toMatch(/Responses/);
    }
  });
  it("allows configured Responses gateways without claiming every custom endpoint works", () => {
    for (const provider of ["harmony", "openai", "custom"]) {
      expect(mainChatCapability(provider).selectable).toBe(true);
      expect(mainChatCapability(provider).wireApi).toBe("responses");
    }
    expect(mainChatCapability("custom").message).toContain("仅 Chat Completions");
    expect(mainChatCapability("unknown").selectable).toBe(false);
  });
});
