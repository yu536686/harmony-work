import { useCallback, useEffect, useRef, useState } from "react";
import { useDispatch } from "react-redux";
import ConfirmDialog from "../ConfirmDialog";
import { isMacNativeUI } from "../ui/useMacNativeMenu";
import { settingsActions } from "../../store";
import {
  accountSettingsSyncApi,
  marketingApi,
  settingsApi,
  type AccountSettingsSyncStatus,
  type MarketingAccountOverview,
  type MarketingUser,
} from "../../services/tauri";
import "./Account.css";

type AccountPageProps = {
  user: MarketingUser | null;
  onUserChange: (user: MarketingUser | null) => void;
  onOpenSettings: () => void;
  onOpenLogin: () => void;
};

const QUOTA_PER_YUAN = 500000;

function maskApiKey(key: string): string {
  return key.length > 12
    ? `${key.slice(0, 8)}••••••••••••${key.slice(-4)}`
    : "sk-••••••••••••••••";
}

function approvalLabel(status: MarketingUser["approvalStatus"]): string {
  return { pending: "待审批", active: "已启用", disabled: "已停用" }[status];
}

function formatToken(value: number): string {
  return value >= 1000 ? `${(value / 1000).toFixed(1)}K` : String(value);
}

function fallbackCopy(text: string): boolean {
  try {
    const textarea = document.createElement("textarea");
    textarea.value = text;
    textarea.setAttribute("readonly", "");
    textarea.style.position = "fixed";
    textarea.style.opacity = "0";
    document.body.appendChild(textarea);
    textarea.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(textarea);
    return ok;
  } catch {
    return false;
  }
}

export default function AccountPage({ user, onUserChange, onOpenSettings, onOpenLogin }: AccountPageProps) {
  // Account-scoped state must disappear in the same render as an identity change.
  return <AccountSession key={user?.id ?? "signed-out"} user={user} onUserChange={onUserChange} onOpenSettings={onOpenSettings} onOpenLogin={onOpenLogin} />;
}

function AccountSession({ user, onUserChange, onOpenSettings, onOpenLogin }: AccountPageProps) {
  const dispatch = useDispatch();
  const generation = useRef(0);
  const [overview, setOverview] = useState<MarketingAccountOverview | null>(null);
  const [cloudSettings, setCloudSettings] = useState<AccountSettingsSyncStatus | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [showKey, setShowKey] = useState(false);
  const [copied, setCopied] = useState(false);
  const [confirmUseKey, setConfirmUseKey] = useState(false);
  const [savingKey, setSavingKey] = useState(false);
  const [keyMessage, setKeyMessage] = useState("");

  const load = useCallback(async () => {
    const request = ++generation.current;
    if (!user) {
      setLoading(false);
      return;
    }
    setLoading(true);
    setError("");
    const [overviewResult, statusResult] = await Promise.allSettled([
      marketingApi.accountOverview(),
      accountSettingsSyncApi.status(),
    ]);
    if (request !== generation.current) return;
    if (overviewResult.status === "fulfilled") {
      if (overviewResult.value.user.id === user.id) {
        setOverview(overviewResult.value);
      } else {
        setOverview(null);
        setError("账户已变更，请重新加载账户数据。");
      }
    } else {
      const message = String(overviewResult.reason);
      setError(message);
      if (/401|403|登录已过期/.test(message)) {
        onUserChange(null);
      }
    }
    if (statusResult.status === "fulfilled") {
      setCloudSettings(statusResult.value);
    }
    setLoading(false);
  }, [user, onUserChange]);

  useEffect(() => {
    void load();
    return () => { generation.current++; };
  }, [load]);

  if (!user) {
    return (
      <section className="account-page">
        <div className="account-shell">
          <div className="account-breadcrumb">
            <span>个人账户</span>
          </div>
          <div className="account-empty-state">
            <div className="account-empty-art">H</div>
            <h2>请先登录工作台账号</h2>
            <p>登录后即可查看积分、用量与 Harmony API Key。</p>
            <button className="account-btn account-btn-primary" onClick={onOpenLogin}>去登录</button>
          </div>
        </div>
      </section>
    );
  }

  if (loading && !overview && !error) {
    return (
      <section className="account-page">
        <div className="account-shell">
          <div className="account-breadcrumb">
            <span>个人账户</span>
          </div>
          <div className="account-loading">加载账户数据…</div>
        </div>
      </section>
    );
  }

  const balance = overview?.balance ?? null;
  const usage = overview?.usage ?? null;
  const apiKey = overview?.user.id === user.id ? overview.gatewayApiKey : null;

  const stats = [
    {
      name: "已计费消耗",
      value: usage && typeof usage.quota === "number" ? (usage.quota / QUOTA_PER_YUAN).toFixed(4) : "—",
      unit: "积分",
      foot: "统计周期以网关为准",
    },
    {
      name: "请求速率",
      value: usage && typeof usage.rpm === "number" ? String(usage.rpm) : "—",
      unit: "次/分",
      foot: "网关当前速率",
    },
    {
      name: "Token 速率",
      value: usage && typeof usage.tpm === "number" ? formatToken(usage.tpm) : "—",
      unit: "Token/分",
      foot: "网关当前速率",
    },
  ];

  const useHarmonyKey = async () => {
    if (!apiKey) return;
    const request = generation.current;
    setSavingKey(true);
    setKeyMessage("");
    try {
      const updated = await settingsApi.save({
        provider: "harmony",
        model: "deepseek-v4-flash",
        enabled_models: ["deepseek-v4-flash"],
        harmony_api_key: apiKey,
      });
      if (request !== generation.current) return;
      dispatch(settingsActions.setSettings(updated));
      dispatch(settingsActions.setConfigured(true));
      setConfirmUseKey(false);
      setKeyMessage("Harmony 已设为本机默认模型服务。");
    } catch (reason) {
      if (request === generation.current) setKeyMessage(`配置失败：${String(reason)}`);
    } finally {
      if (request === generation.current) setSavingKey(false);
    }
  };

  const copyKey = async () => {
    if (!apiKey) return;
    const request = generation.current;
    let ok = false;
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(apiKey);
        ok = true;
      } else {
        ok = fallbackCopy(apiKey);
      }
    } catch {
      if (request !== generation.current) return;
      ok = fallbackCopy(apiKey);
    }
    if (ok && request === generation.current) {
      setCopied(true);
      window.setTimeout(() => {
        if (request === generation.current) setCopied(false);
      }, 1500);
    }
  };

  return (
    <section className="account-page">
      <div className="account-shell">
        <div className="account-breadcrumb">
          <span>个人账户</span>
          <span className="account-live">
            <i className="account-live-dot" /> 已登录 · 实时数据
          </span>
        </div>

        <header className="account-intro">
          <div>
            <p className="account-eyebrow">YOUR WORK, IN HARMONY</p>
            <h1>个人账户与用量</h1>
            <p className="account-subtitle">你的积分、额度、凭据与云端配置，在一个清晰的空间里。</p>
          </div>
          <span className="account-avatar account-avatar-lg">{user.name.slice(0, 1).toUpperCase()}</span>
        </header>

        {error && <div className="account-error">{error}</div>}

        <div className="account-points-hero">
          <article className="account-card account-points-card">
            <div className="account-card-top">
              <span className="account-eyebrow">MY POINTS</span>
              <span className="account-badge">个人积分</span>
            </div>
            <div className="account-points-value">
              {balance === null ? "—" : balance.toFixed(2)}
              <small>积分</small>
            </div>
            <p className="account-card-foot">当前可用积分余额</p>
            <span className="account-seal" aria-hidden="true">H</span>
          </article>
        </div>

        <div className="account-stats">
          {stats.map((stat) => (
            <article className="account-card account-metric" key={stat.name}>
              <div className="account-card-top">
                <span className="account-metric-name">{stat.name}</span>
              </div>
              <div className="account-metric-value">
                {stat.value}
                {stat.unit && <small>{stat.unit}</small>}
              </div>
              <div className="account-metric-foot">{stat.foot}</div>
            </article>
          ))}
        </div>

        <div className="account-grid">
          <article className="account-card account-access-card">
            <div className="account-card-top">
              <h2>Harmony API Key</h2>
              <span className="account-badge account-accent">
                <i className="account-live-dot" />
                {apiKey ? "实时凭据" : "未配置"}
              </span>
            </div>
            <p className="account-card-foot">
              连接 Harmony Work 与 Harmony 模型服务。
              <br />
              凭据默认隐藏，只有你主动查看时才会展开。
            </p>
            <div className="account-key-row">
              <code className="account-key-text">{showKey && apiKey ? apiKey : maskApiKey(apiKey ?? "sk-placeholder")}</code>
              <button
                type="button"
                className="account-icon-button"
                aria-pressed={showKey}
                aria-label={showKey ? "隐藏 API Key" : "查看 API Key"}
                onClick={() => setShowKey((value) => !value)}
              >
                {showKey ? "隐藏" : "显示"}
              </button>
            </div>
            <div className="account-actions">
              <button type="button" className="account-btn account-btn-primary" onClick={() => void copyKey()} disabled={!apiKey}>
                {copied ? "已复制" : "复制 Key"}
              </button>
              <button type="button" className="account-btn" onClick={() => setConfirmUseKey(true)} disabled={!apiKey}>用于本机</button>
            </div>
            {keyMessage && <p className="account-card-foot" role="status">{keyMessage}</p>}
            <p className="account-fineprint">Key 仅在本机查看，请妥善保管。</p>
          </article>

          <article className="account-card account-profile-card">
            <div className="account-profile-head">
              <span className="account-avatar account-avatar-sm">{user.name.slice(0, 1).toUpperCase()}</span>
              <div>
                <h2>{user.name}</h2>
                <small>你的个人工作空间</small>
              </div>
            </div>
            <dl className="account-profile-list">
              <div>
                <dt>账户邮箱</dt>
                <dd>{user.email}</dd>
              </div>
              <div>
                <dt>账户状态</dt>
                <dd>
                  <span className="account-badge account-accent">{approvalLabel(user.approvalStatus)}</span>
                </dd>
              </div>
              <div>
                <dt>访问范围</dt>
                <dd>个人账户与授权服务</dd>
              </div>
            </dl>
          </article>
        </div>

        {cloudSettings && (
          <article className="account-card account-cloud-card">
            <div className="account-card-top">
              <h2>云端配置</h2>
              <span className="account-badge">
                {cloudSettings.locallyConfigured ? "本机已配置" : "仅云端"}
              </span>
            </div>
            <p className="account-card-foot">
              {cloudSettings.snapshot
                ? `最近备份版本 ${cloudSettings.snapshot.revision} · ${cloudSettings.snapshot.updatedAt}`
                : "尚未创建云端备份"}
            </p>
            <button type="button" className="account-btn" onClick={onOpenSettings}>管理云端配置</button>
          </article>
        )}
      </div>
      {confirmUseKey && isMacNativeUI() && (
        <ConfirmDialog
          open
          title="将 Harmony 用于本机？"
          message="会把 Harmony 设为默认提供商，并将当前账号的 Key 保存到本机加密设置。现有其他提供商配置仍会保留。"
          confirmLabel="确认使用"
          variant="primary"
          loading={savingKey}
          onConfirm={() => void useHarmonyKey()}
          onCancel={() => setConfirmUseKey(false)}
        />
      )}
      {confirmUseKey && !isMacNativeUI() && (
        <div className="account-confirm-backdrop" role="presentation">
          <section className="account-confirm" role="dialog" aria-modal="true" aria-labelledby="account-confirm-title">
            <h2 id="account-confirm-title">将 Harmony 用于本机？</h2>
            <p>会把 Harmony 设为默认提供商，并将当前账号的 Key 保存到本机加密设置。现有其他提供商配置仍会保留。</p>
            <div className="account-actions">
              <button type="button" className="account-btn" onClick={() => setConfirmUseKey(false)} disabled={savingKey}>取消</button>
              <button type="button" className="account-btn account-btn-primary" onClick={() => void useHarmonyKey()} disabled={savingKey}>{savingKey ? "正在配置…" : "确认使用"}</button>
            </div>
          </section>
        </div>
      )}
    </section>
  );
}
