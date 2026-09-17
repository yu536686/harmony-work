import { describe, expect, it } from "vitest";
import {
  SETTINGS_SECTIONS,
  getLegacySettingsSection,
} from "./settingsSections";

describe("settings sections", () => {
  it("keeps the seven sections in the product navigation order", () => {
    expect(SETTINGS_SECTIONS.map((section) => section.label)).toEqual([
      "账户与云端",
      "模型与 API",
      "渠道与连接",
      "工作区与安全",
      "Agent 与技能",
      "外观与交互",
      "系统与诊断",
    ]);
  });

  it("assigns each original settings block to one stable section", () => {
    expect(SETTINGS_SECTIONS.map((section) => section.fieldIds)).toEqual([
      ["account-settings-sync"],
      ["ai-provider", "named-llm-providers"],
      ["im-gateway", "email"],
      ["workspace", "security"],
      ["agent-config", "ssh-servers"],
      ["appearance"],
      ["runtime-dependencies", "tools", "audit", "debug"],
    ]);
  });

  it.each(["tools", "audit", "debug"])("keeps legacy %s under system diagnostics only", (route) => {
    expect(getLegacySettingsSection(route)).toBe("system-diagnostics");
    expect(
      SETTINGS_SECTIONS.filter((section) => section.legacyRoutes.includes(route)),
    ).toHaveLength(1);
  });
});
