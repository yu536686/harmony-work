import { describe, expect, it } from "vitest";
import { canInvokeSkill, type SkillLifecycle } from "./skillLifecycle";

const readySkill: SkillLifecycle = {
  installed: true,
  enabled: true,
  dependenciesReady: true,
  runtimeState: "ready",
};

describe("canInvokeSkill", () => {
  it("requires installation, enablement, dependencies, and verified runtime", () => {
    expect(canInvokeSkill(readySkill)).toBe(true);
    expect(canInvokeSkill({ ...readySkill, installed: false })).toBe(false);
    expect(canInvokeSkill({ ...readySkill, enabled: false })).toBe(false);
    expect(canInvokeSkill({ ...readySkill, dependenciesReady: false })).toBe(false);
    expect(canInvokeSkill({ ...readySkill, runtimeState: "unverified" })).toBe(false);
  });
});
