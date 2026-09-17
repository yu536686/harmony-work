import { describe, expect, it } from "vitest";
import {
  WORKSPACE_NAVIGATION,
  normalizeWorkspaceRoute,
} from "./workspaceNavigation";

describe("workspace navigation", () => {
  it("defines the Work, Collaboration, and System navigation groups", () => {
    expect(WORKSPACE_NAVIGATION.work.map((item) => item.id)).toEqual([
      "chat",
      "creative",
      "marketing-assets",
      "skills",
    ]);
    expect(WORKSPACE_NAVIGATION.collaboration.map((item) => item.id)).toEqual([
      "team",
      "members",
      "experts",
      "scheduler",
      "memory",
    ]);
    expect(WORKSPACE_NAVIGATION.system.map((item) => item.id)).toEqual(["settings"]);
  });

  it.each([
    ["market", { route: "skills" }],
    ["tools", { route: "settings", settingsSection: "agent-skills" }],
    ["audit", { route: "settings", settingsSection: "system-diagnostics" }],
    ["debug", { route: "settings", settingsSection: "system-diagnostics" }],
  ] as const)("normalizes the legacy %s route", (legacyRoute, expected) => {
    expect(normalizeWorkspaceRoute(legacyRoute)).toEqual(expected);
  });
});
