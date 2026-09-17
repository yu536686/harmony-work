import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
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

  it("provides the sidebar presentation and drives App navigation from the model", () => {
    const items = Object.values(WORKSPACE_NAVIGATION).flat();
    const appSource = readFileSync(path.join(process.cwd(), "src", "App.tsx"), "utf8");

    expect(items.every((item) => (
      item.label !== undefined
      && item.icon !== undefined
      && item.activeRoutes.length > 0
      && item.action !== undefined
    ))).toBe(true);
    expect(appSource).toContain("WORKSPACE_NAVIGATION.work.map");
    expect(appSource).toContain("WORKSPACE_NAVIGATION.collaboration.map");
    expect(appSource).toContain("WORKSPACE_NAVIGATION.system.map");
  });

  it.each([
    ["market", { route: "skills" }],
    ["tools", { route: "settings", settingsSection: "system-diagnostics" }],
    ["audit", { route: "settings", settingsSection: "system-diagnostics" }],
    ["debug", { route: "settings", settingsSection: "system-diagnostics" }],
  ] as const)("normalizes the legacy %s route", (legacyRoute, expected) => {
    expect(normalizeWorkspaceRoute(legacyRoute)).toEqual(expected);
  });
});
