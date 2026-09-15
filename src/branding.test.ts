import { describe, expect, it } from "vitest";
import { AGENT_BRAND, PRODUCT_BRAND } from "./branding";

describe("branding contract", () => {
  it("separates the product brand from the default agent identity", () => {
    expect(PRODUCT_BRAND.displayName).toBe("Harmony Work");
    expect(AGENT_BRAND.displayNameZh).toBe("小亨");
    expect(AGENT_BRAND.composerPlaceholderZh).toBe("给小亨发送消息");
  });
});
