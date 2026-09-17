import type { PrimaryChannelId, PrimaryChannelSetupResult } from "../../services/tauri/config";

export type { PrimaryChannelId, PrimaryChannelSetupResult };

export function canReceiveMessages(result: PrimaryChannelSetupResult) {
  return result.state === "connected";
}

export function primaryChannelStatusLabel(result: PrimaryChannelSetupResult) {
  if (result.state === "connected" && result.nextAction) return "IM 已连接，企业能力待授权";
  switch (result.state) {
    case "connected": return "已连接";
    case "initializing": return "正在初始化";
    case "awaiting-user-action": return "等待授权";
    case "ready": return "已就绪";
    case "repair-required": return "需要修复";
    case "unconfigured": return "尚未配置";
  }
}
