import { useMemo, useState } from "react";
import { useDispatch } from "react-redux";
import { useTranslation } from "react-i18next";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { settingsActions } from "../../store";
import { openPath, settingsApi } from "../../services/tauri";
import { setLanguage } from "../../i18n";

interface Props {
  onComplete: () => void;
}

type Step = "welcome" | "provider" | "policy" | "done";

const PROVIDERS = [
  {
    id: "anthropic",
    label: "Anthropic (Claude)",
    baseUrl: "https://api.anthropic.com/v1",
    keyField: "anthropic_api_key",
    placeholder: "claude-sonnet-4-5",
    keyPlaceholder: "sk-ant-...",
    guide: "https://console.anthropic.com/settings/keys",
    guideText: "进入 Anthropic Console 创建 API Key。",
  },
  {
    id: "openai",
    label: "OpenAI (GPT)",
    baseUrl: "https://api.openai.com/v1",
    keyField: "openai_api_key",
    placeholder: "gpt-4o",
    keyPlaceholder: "sk-...",
    guide: "https://platform.openai.com/api-keys",
    guideText: "进入 OpenAI Platform 创建 API Key。",
  },
  {
    id: "deepseek",
    label: "DeepSeek（深度求索）",
    baseUrl: "https://api.deepseek.com/v1",
    keyField: "deepseek_api_key",
    placeholder: "deepseek-chat",
    keyPlaceholder: "sk-...",
    guide: "https://platform.deepseek.com/api_keys",
    guideText: "进入 DeepSeek 开放平台创建 API Key。",
  },
  {
    id: "qwen",
    label: "通义千问 (Qwen)",
    baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    keyField: "qwen_api_key",
    placeholder: "qwen3-max",
    keyPlaceholder: "sk-...",
    guide: "https://dashscope.console.aliyun.com/apiKey",
    guideText: "进入阿里云百炼控制台创建 API Key。",
  },
  {
    id: "minimax",
    label: "MiniMax",
    baseUrl: "https://api.minimax.io/v1",
    keyField: "minimax_api_key",
    placeholder: "MiniMax-M2.5",
    keyPlaceholder: "sk-...",
    guide: "https://platform.minimaxi.com/user-center/basic-information/interface-key",
    guideText: "进入 MiniMax 开放平台创建 API Key。",
  },
  {
    id: "zhipu",
    label: "智谱 GLM",
    baseUrl: "https://api.z.ai/api/paas/v4",
    keyField: "zhipu_api_key",
    placeholder: "glm-5",
    keyPlaceholder: "sk-...",
    guide: "https://www.bigmodel.cn/usercenter/proj-mgmt/apikeys",
    guideText: "进入智谱开放平台创建 API Key。",
  },
  {
    id: "kimi",
    label: "Kimi / Moonshot",
    baseUrl: "https://api.moonshot.cn/v1",
    keyField: "kimi_api_key",
    placeholder: "kimi-k2.5",
    keyPlaceholder: "sk-...",
    guide: "https://platform.moonshot.cn/console/api-keys",
    guideText: "进入 Moonshot 控制台创建 API Key。",
  },
  {
    id: "siliconflow",
    label: "SiliconFlow",
    baseUrl: "https://api.siliconflow.cn/v1",
    keyField: "siliconflow_api_key",
    placeholder: "deepseek-ai/DeepSeek-V3",
    keyPlaceholder: "sk-...",
    guide: "https://cloud.siliconflow.cn/account/ak",
    guideText: "进入 SiliconFlow 控制台创建 API Key。",
  },
  {
    id: "custom",
    label: "自定义 OpenAI 兼容接口",
    baseUrl: "",
    keyField: "openai_api_key",
    placeholder: "model-name",
    keyPlaceholder: "sk-...",
    guide: "",
    guideText: "填写兼容 OpenAI /v1 接口的 Base URL 和 API Key。",
  },
];

function uniqStrings(values: string[]) {
  return Array.from(new Set(values.map((value) => value.trim()).filter(Boolean)));
}

function providerMeta(provider: string) {
  return PROVIDERS.find((item) => item.id === provider) ?? PROVIDERS[0];
}

export default function Onboarding({ onComplete }: Props) {
  const { t, i18n } = useTranslation();
  const dispatch = useDispatch();
  const [step, setStep] = useState<Step>("welcome");
  const [provider, setProvider] = useState("anthropic");
  const [apiKey, setApiKey] = useState("");
  const [customBaseUrl, setCustomBaseUrl] = useState("");
  const [model, setModel] = useState("claude-sonnet-4-5");
  const [enabledModels, setEnabledModels] = useState<string[]>(["claude-sonnet-4-5"]);
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [modelLoading, setModelLoading] = useState(false);
  const [workspace, setWorkspace] = useState("");
  const [policyMode, setPolicyMode] = useState("balanced");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  const meta = useMemo(() => providerMeta(provider), [provider]);
  const modelOptions = useMemo(
    () => uniqStrings([model, ...enabledModels, ...availableModels, meta.placeholder]),
    [availableModels, enabledModels, meta.placeholder, model],
  );

  const browseWorkspace = async () => {
    try {
      const selected = await openDialog({ directory: true, multiple: false, title: t("onboarding.workspaceBrowseTitle") });
      if (selected && typeof selected === "string") {
        setWorkspace(selected);
      }
    } catch {
      // dialog cancelled or not available
    }
  };

  const currentLang = i18n.language === "en" ? "en" : "zh";
  const toggleLang = () => {
    const next = currentLang === "zh" ? "en" : "zh";
    setLanguage(next);
  };

  const changeProvider = (next: string) => {
    const nextMeta = providerMeta(next);
    setProvider(next);
    setModel(nextMeta.placeholder);
    setEnabledModels([nextMeta.placeholder]);
    setAvailableModels([]);
    setError("");
  };

  const fetchModels = async () => {
    if (!apiKey.trim()) {
      setError(t("onboarding.apiKeyRequired"));
      return;
    }
    const baseUrl = provider === "custom" ? customBaseUrl.trim() : meta.baseUrl;
    if (provider === "custom" && !baseUrl) {
      setError("自定义提供商需要先填写 Base URL。");
      return;
    }
    setModelLoading(true);
    setError("");
    try {
      const infos = await settingsApi.fetchModels(provider, apiKey.trim(), baseUrl);
      const ids = uniqStrings(infos.map((item) => item.id));
      setAvailableModels(ids);
      const nextModel = ids[0] || model || meta.placeholder;
      setModel(nextModel);
      setEnabledModels(uniqStrings([nextModel, ...ids.slice(0, 4)]));
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setModelLoading(false);
    }
  };

  const toggleEnabledModel = (value: string) => {
    setEnabledModels((prev) => {
      if (prev.includes(value)) {
        return prev.filter((item) => item !== value);
      }
      return uniqStrings([...prev, value]);
    });
  };

  const handleSave = async () => {
    if (!apiKey.trim()) {
      setError(t("onboarding.apiKeyRequired"));
      return;
    }
    if (!model.trim()) {
      setError("请选择默认模型。");
      return;
    }
    setSaving(true);
    setError("");
    try {
      const normalizedEnabledModels = uniqStrings([model, ...enabledModels]);
      const updates: Record<string, unknown> = {
        provider,
        model,
        enabled_models: normalizedEnabledModels,
        available_models: availableModels,
        policy_mode: policyMode,
        [meta.keyField]: apiKey.trim(),
      };
      if (provider === "custom") updates.custom_base_url = customBaseUrl.trim();
      if (workspace.trim()) updates.workspace_root = workspace;

      const settings = await settingsApi.save(updates);
      dispatch(settingsActions.setSettings(settings));
      dispatch(settingsActions.setConfigured(true));
      setStep("done");
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div style={{ display: "flex", alignItems: step === "provider" ? "flex-start" : "center", justifyContent: "center", height: "100vh", minHeight: 0, background: "var(--bg-primary)", padding: 24, paddingBottom: step === "provider" ? 88 : 24, position: "relative", overflowY: "auto", overflowX: "hidden", boxSizing: "border-box" }}>
      <button
        onClick={toggleLang}
        style={{
          position: "fixed",
          top: 16,
          right: 16,
          padding: "6px 14px",
          borderRadius: 20,
          border: "1px solid var(--border-color)",
          background: "var(--bg-secondary)",
          color: "var(--text-secondary)",
          cursor: "pointer",
          fontSize: 13,
          fontWeight: 500,
          zIndex: 100,
        }}
      >
        {currentLang === "zh" ? "EN" : "中文"}
      </button>
      <div style={{ maxWidth: 520, width: "100%" }}>
        {step === "welcome" && (
          <div style={{ textAlign: "center" }}>
            <img src="/piscis.png" alt="小亨 WorkBuddy" style={{ width: 96, height: 96, objectFit: "contain", borderRadius: 24, marginBottom: 18 }} />
            <h1 style={{ fontSize: 28, fontWeight: 700, color: "var(--text-primary)", marginBottom: 12 }}>
              {t("onboarding.welcomeTitle")}
            </h1>
            <p style={{ color: "var(--text-secondary)", marginBottom: 32, lineHeight: 1.7 }}>
              {t("onboarding.welcomeDesc")}
            </p>
            <button className="btn btn-primary" style={{ padding: "12px 32px", fontSize: 16 }} onClick={() => setStep("provider")}>
              {t("onboarding.getStarted")}
            </button>
          </div>
        )}

        {step === "provider" && (
          <div>
            <h2 style={{ fontSize: 22, fontWeight: 600, color: "var(--text-primary)", marginBottom: 8 }}>
              {t("onboarding.configureTitle")}
            </h2>
            <p style={{ color: "var(--text-secondary)", marginBottom: 24, fontSize: 14 }}>
              {t("onboarding.configureDesc")}
            </p>

            {error && (
              <div style={{ padding: "10px 14px", background: "rgba(248,113,113,0.1)", border: "1px solid var(--error)", borderRadius: "var(--radius)", color: "var(--error)", marginBottom: 16, fontSize: 13 }}>
                {error}
              </div>
            )}

            <div className="form-group">
              <label className="label">{t("onboarding.aiProvider")}</label>
              <select className="input" value={provider} onChange={(e) => changeProvider(e.target.value)}>
                {PROVIDERS.map((item) => (
                  <option key={item.id} value={item.id}>{item.label}</option>
                ))}
              </select>
            </div>

            {provider === "custom" && (
              <div className="form-group">
                <label className="label">Base URL</label>
                <input className="input" value={customBaseUrl} onChange={(e) => setCustomBaseUrl(e.target.value)} placeholder="https://example.com/v1" />
              </div>
            )}

            <div className="form-group">
              <label className="label">{t("onboarding.apiKey")}</label>
              <input
                className="input"
                type="password"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder={meta.keyPlaceholder}
                autoFocus
              />
              <div style={{ display: "flex", justifyContent: "space-between", gap: 12, alignItems: "center", marginTop: 6 }}>
                <p style={{ fontSize: 12, color: "var(--text-muted)", margin: 0 }}>{meta.guideText}</p>
                {meta.guide && (
                  <button type="button" className="btn btn-secondary" style={{ minHeight: 30, padding: "0 10px", fontSize: 12 }} onClick={() => void openPath(meta.guide)}>
                    获取 API Key
                  </button>
                )}
              </div>
            </div>

            <div className="form-group">
              <div style={{ display: "flex", justifyContent: "space-between", gap: 12, alignItems: "center" }}>
                <label className="label" style={{ margin: 0 }}>可用模型</label>
                <button type="button" className="btn btn-secondary" style={{ minHeight: 32, padding: "0 12px" }} onClick={() => void fetchModels()} disabled={modelLoading || !apiKey.trim()}>
                  {modelLoading ? "获取中..." : "获取模型列表"}
                </button>
              </div>
              <select className="input" value={model} onChange={(e) => {
                setModel(e.target.value);
                setEnabledModels((prev) => uniqStrings([e.target.value, ...prev]));
              }}>
                {modelOptions.map((item) => (
                  <option key={item} value={item}>{item}</option>
                ))}
              </select>
              <p style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 4 }}>
                这里选择启动/默认模型；下方勾选允许在会话底部切换的模型。
              </p>
            </div>

            <div className="form-group">
              <label className="label">启用模型</label>
              <div style={{ display: "grid", gap: 8, maxHeight: 150, overflow: "auto", padding: 10, border: "1px solid var(--border)", borderRadius: "var(--radius)", background: "var(--bg-secondary)" }}>
                {modelOptions.map((item) => (
                  <label key={item} style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, color: "var(--text-secondary)" }}>
                    <input
                      type="checkbox"
                      checked={enabledModels.includes(item) || item === model}
                      disabled={item === model}
                      onChange={() => toggleEnabledModel(item)}
                    />
                    <span>{item}</span>
                    {item === model && <small style={{ color: "var(--text-muted)" }}>默认</small>}
                  </label>
                ))}
              </div>
            </div>

            <div className="form-group">
              <label className="label">{t("onboarding.workspace")}</label>
              <div style={{ display: "flex", gap: 8 }}>
                <input
                  className="input"
                  value={workspace}
                  onChange={(e) => setWorkspace(e.target.value)}
                  placeholder={t("onboarding.workspacePlaceholder")}
                  style={{ flex: 1 }}
                />
                <button type="button" className="btn btn-secondary" onClick={browseWorkspace} style={{ whiteSpace: "nowrap", padding: "0 14px" }}>
                  {t("onboarding.workspaceBrowse")}
                </button>
              </div>
              <p style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 4 }}>
                {t("onboarding.workspaceHelp")}
              </p>
            </div>

            <div style={{ display: "flex", gap: 8, justifyContent: "space-between", marginTop: 24 }}>
              <button className="btn btn-secondary" onClick={() => setStep("welcome")}>{t("onboarding.backBtn")}</button>
              <button className="btn btn-primary" onClick={() => setStep("policy")} disabled={!apiKey.trim() || !model.trim()}>
                {t("onboarding.nextBtn")}
              </button>
            </div>
          </div>
        )}

        {step === "policy" && (
          <div>
            <h2 style={{ fontSize: 22, fontWeight: 600, color: "var(--text-primary)", marginBottom: 8 }}>
              {t("onboarding.policyTitle")}
            </h2>
            <p style={{ color: "var(--text-secondary)", marginBottom: 24, fontSize: 14 }}>
              {t("onboarding.policyDesc")}
            </p>

            {[
              { value: "strict", label: t("onboarding.policyStrict"), desc: t("onboarding.policyStrictDesc") },
              { value: "balanced", label: t("onboarding.policyBalanced"), desc: t("onboarding.policyBalancedDesc") },
              { value: "dev", label: t("onboarding.policyDev"), desc: t("onboarding.policyDevDesc") },
            ].map((opt) => (
              <div
                key={opt.value}
                onClick={() => setPolicyMode(opt.value)}
                style={{
                  padding: "14px 16px",
                  borderRadius: "var(--radius)",
                  border: `2px solid ${policyMode === opt.value ? "var(--accent)" : "var(--border)"}`,
                  marginBottom: 10,
                  cursor: "pointer",
                  background: policyMode === opt.value ? "rgba(var(--accent-rgb),0.06)" : "transparent",
                  transition: "border-color 0.15s",
                }}
              >
                <div style={{ fontWeight: 600, color: "var(--text-primary)", marginBottom: 2 }}>{opt.label}</div>
                <div style={{ fontSize: 13, color: "var(--text-secondary)" }}>{opt.desc}</div>
              </div>
            ))}

            {error && (
              <div style={{ padding: "10px 14px", background: "rgba(248,113,113,0.1)", border: "1px solid var(--error)", borderRadius: "var(--radius)", color: "var(--error)", marginBottom: 16, fontSize: 13 }}>
                {error}
              </div>
            )}

            <div style={{ display: "flex", gap: 8, justifyContent: "space-between", marginTop: 24 }}>
              <button className="btn btn-secondary" onClick={() => setStep("provider")}>{t("onboarding.backBtn")}</button>
              <button className="btn btn-primary" onClick={handleSave} disabled={saving}>
                {saving ? t("common.saving") : t("onboarding.saveAndContinue")}
              </button>
            </div>
          </div>
        )}

        {step === "done" && (
          <div style={{ textAlign: "center" }}>
            <div style={{ fontSize: 64, marginBottom: 16 }}>OK</div>
            <h2 style={{ fontSize: 22, fontWeight: 600, color: "var(--text-primary)", marginBottom: 12 }}>
              {t("onboarding.doneTitle")}
            </h2>
            <p style={{ color: "var(--text-secondary)", marginBottom: 32 }}>
              {t("onboarding.doneDesc")}
            </p>
            <button className="btn btn-primary" style={{ padding: "12px 32px", fontSize: 16 }} onClick={onComplete}>
              {t("onboarding.startChatting")}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
