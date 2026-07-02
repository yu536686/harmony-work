import { useEffect, useState } from "react";
import {
  marketingApi,
  openPath,
  systemApi,
  type EnvironmentBootstrapResult,
  type MarketingUser,
} from "../../services/tauri";
import "./style.css";

const REGISTER_URL = "http://www.harmonywatch.cn:8090";

export default function Bootstrap({ onComplete, onUserChange }: {
  onComplete: () => void;
  onUserChange: (user: MarketingUser | null) => void;
}) {
  const [status, setStatus] = useState<EnvironmentBootstrapResult | null>(null);
  const [running, setRunning] = useState(false);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [loginMessage, setLoginMessage] = useState("");

  const refresh = async () => {
    setStatus(await systemApi.checkEnvironmentBootstrap());
  };

  useEffect(() => {
    void refresh();
  }, []);

  const runBootstrap = async () => {
    setRunning(true);
    try {
      setStatus(await systemApi.runEnvironmentBootstrap());
    } finally {
      setRunning(false);
    }
  };

  const login = async (event: React.FormEvent) => {
    event.preventDefault();
    setLoginMessage("");
    try {
      const user = await marketingApi.login(email.trim(), password);
      onUserChange(user);
      if (user.approvalStatus === "active") {
        setLoginMessage("登录成功，账号已启用。");
      } else if (user.approvalStatus === "pending") {
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

  return (
    <main className="bootstrap-screen">
      <section className="bootstrap-hero">
        <span className="eyebrow">Xiaoheng WorkBuddy Setup</span>
        <h1>初始化运行环境</h1>
        <p>
          WorkBuddy 会优先使用安装目录旁边的本地安装包完成 Git、Node.js、Python 和 WeCom CLI 初始化；
          找不到本地安装包时，才会尝试 winget/npm 在线安装。
        </p>
        <div className="bootstrap-actions">
          <button className="btn btn-primary" onClick={() => void runBootstrap()} disabled={running}>
            {running ? "初始化中..." : "一键初始化"}
          </button>
          <button className="btn btn-secondary" onClick={() => void refresh()} disabled={running}>重新检测</button>
          <button className="btn btn-secondary" onClick={onComplete}>跳过，进入基础对话</button>
        </div>
      </section>

      <section className="bootstrap-grid">
        <div className="card bootstrap-panel">
          <div className="bootstrap-panel-head">
            <div>
              <span className="eyebrow">Environment</span>
              <h2>环境状态</h2>
            </div>
            <strong>{readyCount}/{totalCount}</strong>
          </div>
          <div className="bootstrap-runtime-list">
            {(status?.items ?? []).map((item) => (
              <article className="bootstrap-runtime" key={item.key}>
                <span className={`bootstrap-dot ${item.available ? "ready" : ""}`} />
                <div>
                  <strong>{item.name}</strong>
                  <small>{item.version || item.details}</small>
                  {item.local_installer && <small>本地安装包：{item.local_installer}</small>}
                </div>
                <em>{item.available ? "已就绪" : item.source === "local" ? "可离线安装" : "待安装"}</em>
              </article>
            ))}
          </div>
          <div className="bootstrap-paths">
            <strong>离线安装包目录</strong>
            {(status?.offline_dirs ?? []).map((dir) => <code key={dir}>{dir}</code>)}
            <p>支持：Git/Node/Python 的 .exe/.msi 安装包，以及 @wecom/cli 的 .tgz 包。</p>
          </div>
        </div>

        <div className="card bootstrap-panel">
          <div className="bootstrap-panel-head">
            <div>
              <span className="eyebrow">Account</span>
              <h2>登录账号</h2>
            </div>
          </div>
          <form className="bootstrap-login" onSubmit={login}>
            <label>邮箱<input className="input" value={email} onChange={(event) => setEmail(event.target.value)} autoComplete="email" /></label>
            <label>密码<input className="input" type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="current-password" /></label>
            {loginMessage && <div className="bootstrap-message">{loginMessage}</div>}
            <button className="btn btn-primary" disabled={!email.trim() || !password}>登录</button>
            <button className="btn btn-secondary" type="button" onClick={() => void openPath(REGISTER_URL)}>
              注册账号
            </button>
          </form>
          <p className="bootstrap-note">
            注册会打开线上营销工作台网页。新注册账号需要管理员审批，未审批账号可以登录但不能访问高阶内容。
          </p>
        </div>
      </section>

      {!!status?.logs.length && (
        <section className="card bootstrap-log">
          <span className="eyebrow">Log</span>
          <pre>{status.logs.join("\n")}</pre>
        </section>
      )}
    </main>
  );
}
