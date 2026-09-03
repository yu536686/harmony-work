import { describe, expect, it } from "vitest";
import { DESIGN_TOKENS, getSidebarWidth } from "./designTokens";

describe("design tokens", () => {
  it("keeps the documented navigation widths stable", () => {
    expect(DESIGN_TOKENS.layout.sidebarExpanded).toBe(280);
    expect(DESIGN_TOKENS.layout.sidebarCollapsed).toBe(64);
    expect(getSidebarWidth(false)).toBe(DESIGN_TOKENS.layout.sidebarExpanded);
    expect(getSidebarWidth(true)).toBe(DESIGN_TOKENS.layout.sidebarCollapsed);
  });

  it("uses short, accessible motion durations", () => {
    expect(DESIGN_TOKENS.motion.fast).toBe(150);
    expect(DESIGN_TOKENS.motion.standard).toBe(220);
    expect(DESIGN_TOKENS.motion.slow).toBe(320);
  });
});
