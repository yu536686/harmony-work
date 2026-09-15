import { describe, expect, it } from "vitest";
import { HARMONY_API_BASE, MARKETING_BASE, MARKET_BASE, migratePublicServiceUrl } from "./serviceUrls";

describe("public service migration", () => {
  it("uses HTTPS without legacy public ports", () => {
    expect(HARMONY_API_BASE).toBe("https://api.harmonywatch.cn/v1");
    expect(MARKETING_BASE).toBe("https://portal.harmonywatch.cn");
    expect(MARKET_BASE).toBe("https://market.harmonywatch.cn");
  });
  it.each([
    ["https://api.harmonywatch.cn:13001/v1", "https://api.harmonywatch.cn/v1"],
    ["http://61.150.31.150:8090/api/assets/a?token=x%2Fy#preview", "https://portal.harmonywatch.cn/api/assets/a?token=x%2Fy#preview"],
    ["http://www.harmonywatch.cn:8090", "https://portal.harmonywatch.cn"],
    ["http://61.150.31.150:8787/api/skills", "https://market.harmonywatch.cn/api/skills"],
  ])("migrates %s while retaining paths and query values", (input, expected) => {
    expect(migratePublicServiceUrl(input)).toBe(expected);
    expect(migratePublicServiceUrl(expected)).toBe(expected);
  });
  it.each([
    "http://192.168.11.29:8090", "http://192.168.11.33:5244/file",
    "https://custom.example:13001/v1", "https://api.harmonywatch.cn.evil.example:13001/v1",
    "https://api.harmonywatch.cn:9443/v1", "", "/api/assets/x", "data:image/png;base64,AAA",
  ])("does not rewrite unrelated or LAN address %s", (input) => {
    expect(migratePublicServiceUrl(input)).toBe(input);
  });
});
