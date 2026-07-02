import { useEffect, useState, type FormEvent } from "react";
import {
  marketingApi,
  openPath,
  systemApi,
  windowApi,
  type EnvironmentBootstrapItem,
  type EnvironmentBootstrapResult,
  type MarketingUser,
} from "../../services/tauri";
import "./style.css";

const REGISTER_URL = "http://www.harmonywatch.cn:8090";

type BootstrapStep = "environment" | "login";

export default function Bootstrap({
  onComplete,
  onUserChange,
}: {
  onComplete: () => void;
  onUserChange: (user: MarketingUser | null) => void;
}) {
  const [step, setStep] = useState<BootstrapStep>("environment");
  const [status, setStatus] = useState<EnvironmentBootstrapResult | null>(null);
  const [installingKey, setInstallingKey] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [loginMessage, setLoginMessage] = useState("");

  const refresh = async () => {
    setRefreshing(true);
    try {
      setStatus(await systemApi.checkEnvironmentBootstrap());
    } finally {
      setRefreshing(false);
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const installItem = async (item: EnvironmentBootstrapItem) => {
    if (item.available || installingKey) return;
    setInstallingKey(item.key);
    try {
      setStatus(await systemApi.installEnvironmentBootstrapItem(item.key));
    } finally {
      setInstallingKey(null);
    }
  };

  const login = async (event: FormEvent) => {
    event.preventDefault();
    setLoginMessage("");
    try {
      const user = await marketingApi.login(email.trim(), password);
      onUserChange(user);
      if (user.approvalStatus === "active") {
        onComplete();
        return;
      }
      if (user.approvalStatus === "pending") {
        setLoginMessage("账号已注册，但尚未通过管理员审批。审批通过后即可访问素材库、市场和团队功能。");
      } else {
        setLoginMessage("账号当前不可用，请联系管理员处理。");
      }
    } catch (error) {
      setLoginMessage(String(error));
    }
  };

  const readyCount = status?.items.filter((item) => item.available).length ?? 0;
  const totalCount = status?.items.length ?? 0;
  const allReady = totalCount > 0 && readyCount === totalCount;

  return (
    <main className="bootstrap-screen">
      <section className="bootstrap-shell">
        <header className="bootstrap-hero">
          <span className="eyebrow">Xiaoheng WorkBuddy Setup</span>
          <h1>{step === "environment" ? "初始化运行环境" : "登录小亨账号"}</h1>
          <p>
            {step === "environment"
              ? "请逐项安装 Git、Node.js、Python 和 WeCom CLI。WorkBuddy 会优先使用安装目录旁边的本地安装包，找不到时才尝试在线安装。"
              : "登录后可访问线上营销工作台、素材库、技能市场和团队等高阶内容；跳过后仍可进入基础对话。"}
          </p>
          <div className="bootstrap-steps" aria-label="setup progress">
            <span className={step === "environment" ? "active" : ""}>1 环境安装</span>
            <span className={step === "login" ? "active" : ""}>2 账号登录</span>
          </div>
        </header>

        {step === "environment" && (
          <section className="card bootstrap-panel">
            <div className="bootstrap-panel-head">
              <div>
                <span className="eyebrow">Environment</span>
                <h2>环境状态</h2>
              </div>
              <strong>{readyCount}/{totalCount}</strong>
            </div>

            <div className="bootstrap-runtime-list">
              {(status?.items ?? []).map((item) => {
                const installing = installingKey === item.key;
                return (
                  <article className="bootstrap-runtime" key={item.key}>
                    <span className={`bootstrap-dot ${item.available ? "ready" : ""}`} />
                    <div className="bootstrap-runtime-main">
                      <strong>{item.name}</strong>
                      <small>{item.version || item.details}</small>
                      {item.local_installer && <small>本地安装包：{item.local_installer}</small>}
                    </div>
                    <button
                      className={`btn ${item.available ? "btn-secondary" : "btn-primary"} bootstrap-runtime-btn`}
                      disabled={item.available || !!installingKey}
                      onClick={() => void installItem(item)}
                    >
                      {item.available ? "已安装" : installing ? "安装中..." : "安装"}
                    </button>
                  </article>
                );
              })}
            </div>

            <div className="bootstrap-paths">
              <strong>离线安装包目录</strong>
              {(status?.offline_dirs ?? []).map((dir) => <code key={dir}>{dir}</code>)}
              <p>支持：Git/Node/Python 的 .exe/.msi 安装包，以及 @wecom/cli 的 .tgz 包。</p>
            </div>

            <div className="bootstrap-actions">
              <button className="btn btn-secondary" onClick={() => void refresh()} disabled={refreshing || !!installingKey}>
                {refreshing ? "检测中..." : "重新检测"}
              </button>
              <button className="btn btn-primary" onClick={() => setStep("login")} disabled={!allReady || !!installingKey}>
                下一步
              </button>
            </div>
          </section>
        )}

        {step === "login" && (
          <section className="card bootstrap-panel bootstrap-login-panel">
            <div className="bootstrap-panel-head">
              <div>
                <span className="eyebrow">Account</span>
                <h2>登录或注册</h2>
              </div>
            </div>
            <form className="bootstrap-login" onSubmit={login}>
              <label>邮箱<input className="input" value={email} onChange={(event) => setEmail(event.target.value)} autoComplete="email" /></label>
              <label>密码<input className="input" type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="current-password" /></label>
              {loginMessage && <div className="bootstrap-message">{loginMessage}</div>}
              <div className="bootstrap-login-actions">
                <button className="btn btn-primary" disabled={!email.trim() || !password}>登录</button>
                <button className="btn btn-secondary" type="button" onClick={() => void openPath(REGISTER_URL)}>注册</button>
                <button className="btn btn-secondary" type="button" onClick={() => void windowApi.quitApp()}>退出</button>
                <button className="btn btn-secondary" type="button" onClick={onComplete}>跳过</button>
              </div>
            </form>
            <p className="bootstrap-note">
              注册会打开线上营销工作台网页。新注册账号需要管理员审批，未审批账号会显示提示并暂不进入高阶功能。
            </p>
          </section>
        )}

        {!!status?.logs.length && (
          <section className="card bootstrap-log">
            <span className="eyebrow">Log</span>
            <pre>{status.logs.join("\n")}</pre>
          </section>
        )}
      </section>
    </main>
  );
}
