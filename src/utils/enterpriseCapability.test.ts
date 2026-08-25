import { describe, expect, it } from "vitest";
import { preferEnterpriseCapabilityDiagnostics } from "./enterpriseCapability";

describe("preferEnterpriseCapabilityDiagnostics", () => {
  it("uses the live backend diagnostics instead of a fixed fallback", () => {
    expect(
      preferEnterpriseCapabilityDiagnostics(
        ["已通过官方 auth show 状态检查，确认本机凭据已完成初始化。"],
        "请运行 wecom-cli init",
      ),
    ).toEqual(["已通过官方 auth show 状态检查，确认本机凭据已完成初始化。"]);
  });

  it("uses the translated fallback when the backend returned no diagnostics", () => {
    expect(preferEnterpriseCapabilityDiagnostics([], "连接测试失败")).toEqual([
      "连接测试失败",
    ]);
  });
});
