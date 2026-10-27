export interface MainChatCapability { selectable: boolean; wireApi: "responses"; message: string }

/** Main conversations use Codex Responses; vision/embedding adapters have their own contracts. */
export function mainChatCapability(provider: string): MainChatCapability {
  if (provider === "harmony" || provider === "openai") {
    return { selectable: true, wireApi: "responses", message: "主会话使用 Responses API，请选择该接口支持的模型。" };
  }
  if (provider === "custom") {
    return { selectable: true, wireApi: "responses", message: "自定义接口必须支持 /v1/responses；仅 Chat Completions 兼容的接口无法用于主会话。" };
  }
  return { selectable: false, wireApi: "responses", message: "此提供商的默认接口尚未接入主会话所需的 Responses 协议。可通过支持 Responses 的网关配置为自定义接口；原配置与密钥会保留。" };
}
