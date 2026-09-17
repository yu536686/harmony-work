import { describe, expect, it } from "vitest";
import {
  canReceiveMessages,
  primaryChannelStatusLabel,
  type PrimaryChannelSetupResult,
} from "./channelLifecycle";

describe("primary channel lifecycle", () => {
  it.each<[PrimaryChannelSetupResult["state"], boolean]>([
    ["unconfigured", false],
    ["initializing", false],
    ["awaiting-user-action", false],
    ["ready", false],
    ["repair-required", false],
    ["connected", true],
  ])("only treats %s as a message-ready channel when appropriate", (state, expected) => {
    expect(canReceiveMessages({ channel: "wecom", state, capabilities: [] })).toBe(expected);
  });

  it("keeps CLI authorization and IM delivery status distinguishable", () => {
    const result: PrimaryChannelSetupResult = {
      channel: "wecom",
      state: "connected",
      capabilities: [],
      nextAction: "完成企业微信 CLI 授权后可使用通讯录、文档和日程工具。",
    };

    expect(primaryChannelStatusLabel(result)).toBe("IM 已连接，企业能力待授权");
  });
});
