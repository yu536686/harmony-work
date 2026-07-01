import { useEffect, useMemo, useState } from "react";
import { useDispatch, useSelector } from "react-redux";
import { RootState, chatActions, sessionsActions } from "../store";
import {
  chatApi,
  marketingUserContext,
  marketingApi,
  readRememberedMarketingUser,
  sessionsApi,
  type MarketingAsset,
  type MarketingAssetSummary,
  type MarketingCampaignSummary,
  type MarketingUser,
} from "../services/tauri";
import "./Marketing.css";

function formatBytes(value: number | string | undefined) {
  const bytes = Number(value ?? 0);
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = bytes;
  let index = 0;
  while (size >= 1024 && index < units.length - 1) {
    size /= 1024;
    index += 1;
  }
  return `${size.toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
}

function metric(value: unknown) {
  return Number(value ?? 0).toLocaleString("zh-CN");
}

export default function Marketing({ user, onUserChange, onNavigateToChat }: {
  user: MarketingUser | null;
  onUserChange: (user: MarketingUser | null) => void;
  onNavigateToChat: () => void;
}) {
  const dispatch = useDispatch();
  const activeSessionId = useSelector((s: RootState) => s.sessions.activeSessionId);
  const settings = useSelector((s: RootState) => s.settings.settings);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [loginError, setLoginError] = useState("");
  const [loading, setLoading] = useState(false);
  const [summary, setSummary] = useState<MarketingAssetSummary | null>(null);
  const [campaign, setCampaign] = useState<MarketingCampaignSummary | null>(null);
  const [folders, setFolders] = useState<string[]>([]);
  const [folder, setFolder] = useState("");
  const [query, setQuery] = useState("");
  const [assets, setAssets] = useState<MarketingAsset[]>([]);
  const [assetTotal, setAssetTotal] = useState(0);
  const [message, setMessage] = useState("");

  const active = user?.approvalStatus === "active";

  const loadAll = async () => {
    if (!active) return;
    setLoading(true);
    setMessage("");
    try {
      const [nextSummary, nextFolders, nextAssets, nextCampaign] = await Promise.all([
        marketingApi.summary(),
        marketingApi.folders(),
        marketingApi.assets({ folder, q: query, limit: 36, sort: "modified_desc" }),
        marketingApi.campaignSummary(1440),
      ]);
      setSummary(nextSummary);
      setFolders(nextFolders.folders ?? []);
      setAssets(nextAssets.assets ?? []);
      setAssetTotal(nextAssets.total ?? 0);
      setCampaign(nextCampaign);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadAll();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, folder]);

  const totals = summary?.totals ?? {};
  const platformTotals = useMemo(() => {
    const source = campaign?.platforms ?? [];
    return source.reduce<{ views: number; likes: number; spend: number; posts: number }>((acc, item) => {
      acc.views += Number(item.views ?? 0);
      acc.likes += Number(item.likes ?? 0);
      acc.spend += Number(item.spend ?? 0);
      acc.posts += Number(item.postCount ?? item.post_count ?? 0);
      return acc;
    }, { views: 0, likes: 0, spend: 0, posts: 0 });
  }, [campaign]);

  const handleLogin = async (event: React.FormEvent) => {
    event.preventDefault();
    setLoginError("");
    setLoading(true);
    try {
      const next = await marketingApi.login(email.trim(), password);
      onUserChange(next);
    } catch (error) {
      setLoginError(String(error));
    } finally {
      setLoading(false);
    }
  };

  const handleLogout = async () => {
    await marketingApi.logout().catch(() => {});
    onUserChange(null);
    setSummary(null);
    setCampaign(null);
    setAssets([]);
  };

  const ensureChatSession = async () => {
    if (activeSessionId) return activeSessionId;
    const session = await sessionsApi.create("线上营销分析", "chat");
    dispatch(sessionsActions.addSession(session));
    dispatch(sessionsActions.setActiveSession(session.id));
    return session.id;
  };

  const sendToChat = async (content: string, attachment?: { path: string; mediaType: string; filename: string }) => {
    const sessionId = await ensureChatSession();
    dispatch(sessionsActions.setActiveSession(sessionId));
    dispatch(chatActions.clearFrozenBubble(sessionId));
    dispatch(chatActions.clearStreaming(sessionId));
    dispatch(chatActions.clearToolSteps(sessionId));
    dispatch(chatActions.setRunning({ sessionId, running: true }));
    onNavigateToChat();
    await chatApi.send(sessionId, content, {
      attachments: attachment
        ? [{ path: attachment.path, media_type: attachment.mediaType, filename: attachment.filename }]
        : [],
      userContext: marketingUserContext(user),
      clearPlan: true,
    });
  };

  const sendAssetToChat = async (asset: MarketingAsset) => {
    setMessage(`正在准备素材：${asset.fileName}`);
    const prepared = await marketingApi.prepareAssetForChat(asset.id, asset.fileName, settings?.workspace_root);
    const prompt = [
      `当前登录用户：${user?.name}（${user?.email}，${user?.userRole}）`,
      "请读取并分析这个线上营销素材，结合素材库上下文给出可执行建议。",
      "",
      `素材名称：${asset.fileName}`,
      `素材类型：${asset.assetType} / ${asset.mimeType}`,
      `素材库路径：${asset.rootFolder}/${asset.relativePath}`,
      `已缓存到本机会话可访问路径：${prepared.localPath}`,
      `来源接口：${prepared.sourceUrl}`,
    ].join("\n");
    await sendToChat(prompt, {
      path: prepared.localPath,
      mediaType: prepared.mediaType,
      filename: prepared.fileName,
    });
  };

  const sendDashboardToChat = async () => {
    const prompt = [
      `当前登录用户：${user?.name}（${user?.email}，${user?.userRole}）`,
      "请作为线上营销数据分析 Agent，基于以下 WorkBuddy 原生数据看板摘要，输出风险、机会、下一步投放/内容动作，并给出优先级。",
      "",
      `素材总数：${metric(totals.total_count)}，视频：${metric(totals.video_count)}，图片：${metric(totals.image_count)}，近 7 天新增：${metric(totals.week_count)}`,
      `素材总容量：${formatBytes(totals.total_size as number)}`,
      `24小时内容数：${platformTotals.posts}，播放/曝光：${platformTotals.views}，点赞：${platformTotals.likes}，消耗：${platformTotals.spend}`,
      "",
      "平台汇总：",
      JSON.stringify(campaign?.platforms ?? [], null, 2),
      "",
      "素材文件夹：",
      JSON.stringify(summary?.folders ?? [], null, 2),
    ].join("\n");
    await sendToChat(prompt);
  };

  if (!user) {
    const remembered = readRememberedMarketingUser();
    return (
      <section className="marketing-page">
        <header className="feature-topbar">
          <h1 className="feature-topbar-title">线上营销</h1>
          {remembered && <span className="marketing-pill">上次登录：{remembered.name}</span>}
        </header>
        <div className="marketing-auth">
          <form className="marketing-login card" onSubmit={handleLogin}>
            <span className="eyebrow">Marketing Login</span>
            <h2>登录后解锁素材库、数据看板、技能市场和团队协作</h2>
            <p>未登录状态下 WorkBuddy 只保留基础对话能力。登录使用线上营销工作台原有用户数据库。</p>
            <label>邮箱<input className="input" value={email} onChange={(event) => setEmail(event.target.value)} autoComplete="email" /></label>
            <label>密码<input className="input" type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="current-password" /></label>
            {loginError && <div className="marketing-error">{loginError}</div>}
            <button className="btn-primary" disabled={loading || !email.trim() || !password}>登录</button>
          </form>
        </div>
      </section>
    );
  }

  if (!active) {
    return (
      <section className="marketing-page">
        <header className="feature-topbar">
          <h1 className="feature-topbar-title">线上营销</h1>
          <button className="btn-secondary" onClick={handleLogout}>退出登录</button>
        </header>
        <div className="marketing-auth">
          <div className="card marketing-login">
            <span className="eyebrow">Account Pending</span>
            <h2>{user.name}，你的账号还不能访问高阶内容</h2>
            <p>当前状态：{user.approvalStatus}。请等待管理员启用后再访问素材库、看板和团队能力。</p>
          </div>
        </div>
      </section>
    );
  }

  return (
    <section className="marketing-page">
      <header className="feature-topbar">
        <div>
          <h1 className="feature-topbar-title">线上营销</h1>
          <p className="marketing-subtitle">素材库、投放数据和小亨 Agent 已接入 WorkBuddy 原生工作流。</p>
        </div>
        <div className="marketing-actions">
          <span className="marketing-pill">{user.name} · {user.userRole}</span>
          <button className="btn-secondary" onClick={() => void loadAll()} disabled={loading}>刷新</button>
          <button className="btn-secondary" onClick={handleLogout}>退出</button>
        </div>
      </header>

      <div className="marketing-body">
        {message && <div className="marketing-notice">{message}</div>}
        <div className="marketing-metrics">
          <div className="card marketing-metric"><span>素材总数</span><strong>{metric(totals.total_count)}</strong><small>{formatBytes(totals.total_size as number)}</small></div>
          <div className="card marketing-metric"><span>视频 / 图片</span><strong>{metric(totals.video_count)} / {metric(totals.image_count)}</strong><small>近 7 天新增 {metric(totals.week_count)}</small></div>
          <div className="card marketing-metric"><span>24h 播放/曝光</span><strong>{metric(platformTotals.views)}</strong><small>{platformTotals.posts} 条内容</small></div>
          <div className="card marketing-metric"><span>互动 / 消耗</span><strong>{metric(platformTotals.likes)} / {metric(platformTotals.spend)}</strong><small>跨平台汇总</small></div>
        </div>

        <div className="marketing-grid">
          <section className="card marketing-panel">
            <div className="marketing-panel-head">
              <div><span className="eyebrow">Asset Library</span><h2>素材库</h2></div>
              <button className="btn-primary" onClick={() => void sendDashboardToChat()}>让小亨分析看板</button>
            </div>
            <div className="marketing-filters">
              <select className="input" value={folder} onChange={(event) => setFolder(event.target.value)}>
                <option value="">全部文件夹</option>
                {folders.map((item) => <option key={item} value={item}>{item}</option>)}
              </select>
              <input className="input" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索文件名、路径" />
              <button className="btn-secondary" onClick={() => void loadAll()}>搜索</button>
            </div>
            <div className="marketing-assets">
              {assets.map((asset) => (
                <article className="marketing-asset" key={asset.id}>
                  <div>
                    <strong>{asset.fileName}</strong>
                    <small>{asset.rootFolder} / {asset.assetType} / {formatBytes(asset.fileSize)}</small>
                  </div>
                  <button className="btn-secondary" onClick={() => void sendAssetToChat(asset)}>发到会话</button>
                </article>
              ))}
              {!assets.length && <div className="empty-state">没有匹配素材。</div>}
            </div>
            <footer className="marketing-panel-foot">当前筛选 {assets.length} / {assetTotal} 条</footer>
          </section>

          <section className="card marketing-panel">
            <div className="marketing-panel-head">
              <div><span className="eyebrow">Dashboard</span><h2>数据看板</h2></div>
            </div>
            <div className="marketing-platforms">
              {(campaign?.platforms ?? []).map((item) => (
                <article className="marketing-platform" key={String(item.platform)}>
                  <strong>{String(item.label ?? item.platform)}</strong>
                  <div><span>内容</span><b>{metric(item.postCount ?? item.post_count)}</b></div>
                  <div><span>播放</span><b>{metric(item.views)}</b></div>
                  <div><span>互动</span><b>{metric(item.likes)}</b></div>
                  <div><span>粉丝</span><b>{metric(item.followers)}</b></div>
                </article>
              ))}
              {!(campaign?.platforms ?? []).length && <div className="empty-state">暂无投放数据。</div>}
            </div>
            <div className="marketing-posts">
              <h3>内容表现 Top</h3>
              {(campaign?.posts ?? []).slice(0, 8).map((post) => (
                <div className="marketing-post" key={String(post.id)}>
                  <span>{String(post.title || "未命名内容")}</span>
                  <b>{metric(post.views)} 播放</b>
                </div>
              ))}
            </div>
          </section>
        </div>
      </div>
    </section>
  );
}
