import { useCallback, useEffect, useState } from "react";
import {
  accountSettingsSyncApi,
  marketingApi,
  type AccountSettingsSyncStatus,
  type MarketingAccountOverview,
  type MarketingUser,
} from "../../services/tauri";
import "./Account.css";

type AccountPageProps = {
  user: MarketingUser | null;
  onUserChange: (user: MarketingUser | null) => void;
};

function maskApiKey(key: string): string {
  const prefix = key.startsWith("sk-") ? "sk-" : "";
  const body = key.slice(prefix.length);
  return `${prefix}${"*".repeat(Math.max(8, body.length))}`;
}

function approvalLabel(status: MarketingUser["approvalStatus"]): string {
  return { pending: "待审批", active: "已开通", disabled: "已停用" }[status];
}

export default function AccountPage({ user, onUserChange }: AccountPageProps) {
  const [overview, setOverview] = useState<MarketingAccountOverview | null>(null);
  const [cloudSettings, setCloudSettings] = useState<AccountSettingsSyncStatus | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [showKey, setShowKey] = useState(false);

  const load = useCallback(async () => {
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
    if (overviewResult.status === "fulfilled") {
      setOverview(overviewResult.value);
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
  }, [load]);

  if (!user) {
    return (
      <section className="account-page">
        <header className="account-header">
          <h1>个人账户</h1>
        </header>
        <div className="account-empty">请先登录工作台账号。</div>
      </section>
    );
  }

  if (loading && !overview && !error) {
    return (
      <section className="account-page">
        <div className="account-empty">加载中…</div>
      </section>
    );
  }

  const balance = overview?.balance ?? null;
  const usage = overview?.usage ?? null;
  const apiKey = overview?.gatewayApiKey ?? null;

  return (
    <section className="account-page">
      <header className="account-header">
        <h1>个人账户</h1>
        <span className="account-role">{user.userRole === "admin" ? "管理员" : "成员"}</span>
      </header>

      {error && <div className="account-error">{error}</div>}

      <div className="account-identity">
        <strong>{user.name}</strong>
        <span>{user.email}</span>
        <em>{approvalLabel(user.approvalStatus)}</em>
      </div>

      <div className="account-grid">
        <div className="account-card">
          <label>积分余额</label>
          <div className="account-value">{balance === null ? "未开通" : String(balance)}</div>
        </div>
        <div className="account-card">
          <label>用量统计</label>
          {usage ? (
            <ul className="account-usage">
              {Object.entries(usage).map(([key, value]) => (
                <li key={key}>
                  {key}: {value}
                </li>
              ))}
            </ul>
          ) : (
            <div className="account-value">暂无用量数据</div>
          )}
        </div>
      </div>

      <div className="account-card">
        <label>个人 Harmony API Key</label>
        {apiKey ? (
          <div className="account-key-row">
            <code>{showKey ? apiKey : maskApiKey(apiKey)}</code>
            <button className="btn-secondary" onClick={() => setShowKey((value) => !value)}>
              {showKey ? "隐藏" : "显示"}
            </button>
          </div>
        ) : (
          <div className="account-value">未生成 API Key</div>
        )}
      </div>

      {cloudSettings && (
        <div className="account-card">
          <label>云端配置</label>
          <div className="account-value">
            {cloudSettings.snapshot
              ? `版本 ${cloudSettings.snapshot.revision} · ${cloudSettings.snapshot.updatedAt}`
              : "尚未备份"}
          </div>
        </div>
      )}
    </section>
  );
}
