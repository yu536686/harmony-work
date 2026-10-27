import type { Settings } from "../../services/tauri";

const SECRET_FIELDS = new Set([
  "anthropic_api_key", "openai_api_key", "deepseek_api_key", "qwen_api_key",
  "minimax_api_key", "zhipu_api_key", "kimi_api_key", "siliconflow_api_key",
  "harmony_api_key", "vision_api_key", "feishu_app_secret", "wecom_bot_secret",
  "dingtalk_app_secret", "dingtalk_robot_code", "telegram_bot_token",
  "matrix_access_token", "webhook_auth_token", "wechat_gateway_token",
  "wechat_bot_token", "smtp_password",
]);

export function changedSettingsFields(baseline: Settings, draft: Settings): Partial<Settings> {
  return Object.fromEntries(Object.entries(draft).filter(([key, value]) =>
    key !== "is_configured" && JSON.stringify(value) !== JSON.stringify(baseline[key as keyof Settings]),
  ));
}

// Private UI wire type: null clears a secret; omission and blank retain it on older callers.
export type SettingsPatch = Record<string, unknown>;

export function buildSettingsPatch(baseline: Settings, draft: Settings): SettingsPatch {
  const patch: SettingsPatch = changedSettingsFields(baseline, draft);
  for (const key of Object.keys(patch)) {
    if (SECRET_FIELDS.has(key) && patch[key] === "") patch[key] = null;
  }
  for (const [field, secrets] of [["ssh_servers", ["password", "private_key"]], ["llm_providers", ["api_key"]]] as const) {
    const entries = patch[field];
    if (!Array.isArray(entries)) continue;
    const previous = baseline[field] ?? [];
    patch[field] = entries.map((entry: Record<string, unknown>) => {
      const old = previous.find(item => item.id === entry.id) as unknown as Record<string, unknown> | undefined;
      const next = { ...entry };
      for (const secret of secrets) {
        if (old && entry[secret] === old[secret]) delete next[secret];
        else if (old?.[secret] && entry[secret] === "") next[secret] = null;
      }
      return next;
    });
  }
  return patch;
}
