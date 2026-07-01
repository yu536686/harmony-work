import { useEffect, useMemo, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useDispatch, useSelector } from "react-redux";
import { RootState, chatActions, sessionsActions } from "../store";
import {
  chatApi,
  marketingApi,
  marketingUserContext,
  readRememberedMarketingUser,
  sessionsApi,
  type MarketingAsset,
  type MarketingAssetSummary,
  type MarketingCampaignSummary,
  type MarketingPreparedAsset,
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

function field<T = unknown>(source: Record<string, unknown> | undefined, camel: string, snake: string): T | undefined {
  return (source?.[camel] ?? source?.[snake]) as T | undefined;
}

export default function Marketing({ view, user, onUserChange, onNavigateToChat }: {
  view: "assets" | "dashboard";
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
  const [selectedAsset, setSelectedAsset] = useState<MarketingAsset | null>(null);
  const [preview, setPreview] = useState<MarketingPreparedAsset | null>(null);
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
        marketingApi.assets({ folder, q: query, limit: 48, sort: "modified_desc" }),
        marketingApi.campaignSummary(1440),
      ]);
      setSummary(nextSummary);
      setFolders(nextFolders.folders ?? []);
      setAssets(nextAssets.assets ?? []);
      setAssetTotal(nextAssets.total ?? 0);
      setCampaign(nextCampaign);
      if (!selectedAsset && nextAssets.assets?.[0]) {
        setSelectedAsset(nextAssets.assets[0]);
      }
    } catch (error) {
      setMessage(String(error));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadAll();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, folder, view]);

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
    setSelectedAsset(null);
    setPreview(null);
  };

  const prepareAsset = async (asset: MarketingAsset) => {
    const prepared = await marketingApi.prepareAssetForChat(asset.id, asset.fileName, settings?.workspace_root);
    setPreview(prepared);
    return prepared;
  };

  const selectAsset = async (asset: MarketingAsset) => {
    setSelectedAsset(asset);
    setPreview(null);
    if (asset.assetType === "image" || asset.assetType === "video") {
      try {
        await prepareAsset(asset);
      } catch (error) {
        setMessage(String(error));
      }
    }
  };

  const stageAssetToChat = async (asset: MarketingAsset) => {
    setMessage(`正在缓存素材：${asset.fileName}`);
    const prepared = await prepareAsset(asset);
    const session = activeSessionId
      ? null
      : await sessionsApi.create("线上营销素材分析", "chat");
    if (session) {
      dispatch(sessionsActions.addSession(session));
    }
    const targetSessionId = activeSessionId ?? session?.id ?? null;
    if (!targetSessionId) return;
    const prompt = [
      "请基于这个线上营销素材帮我分析：",
      "",
      `素材名称：${asset.fileName}`,
      `素材类型：${asset.assetType} / ${asset.mimeType}`,
      `素材库路径：${asset.rootFolder}/${asset.relativePath}`,
      `本地缓存路径：${prepared.localPath}`,
      "",
      "我的具体需求是：",
    ].join("\n");
    dispatch(sessionsActions.openMainChatView({
      filter: "chat",
      sessionId: targetSessionId,
      composerDraft: prompt,
      composerAttachments: [{ path: prepared.localPath, media_type: prepared.mediaType, filename: prepared.fileName }],
      autoSend: false,
    }));
    setMessage("素材已放入会话输入框附件区，请在聊天窗口补充需求后手动发送。");
    onNavigateToChat();
  };

  const sendDashboardToChat = async () => {
    const session = await sessionsApi.create("线上营销数据看板分析", "chat");
    dispatch(sessionsActions.addSession(session));
    dispatch(sessionsActions.setActiveSession(session.id));
    dispatch(chatActions.clearFrozenBubble(session.id));
    dispatch(chatActions.clearStreaming(session.id));
    dispatch(chatActions.clearToolSteps(session.id));
    dispatch(chatActions.setRunning({ sessionId: session.id, running: true }));
    onNavigateToChat();
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
    await chatApi.send(session.id, prompt, {
      userContext: marketingUserContext(user),
      clearPlan: true,
    });
  };

  if (!user) {
    const remembered = readRememberedMarketingUser();
    return (
      <section className="marketing-page">
        <header className="feature-topbar">
          <h1 className="feature-topbar-title">线上营销工作台</h1>
          {remembered && <span className="marketing-pill">上次登录：{remembered.name}</span>}
        </header>
        <div className="marketing-auth">
          <form className="marketing-login card" onSubmit={handleLogin}>
            <span className="eyebrow">Marketing Login</span>
            <h2>登录后解锁素材库、数据看板、技能市场和团队协作</h2>
            <p>登录使用线上营销工作台原有用户数据库。接口已切换到公网服务。</p>
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
          <h1 className="feature-topbar-title">线上营销工作台</h1>
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

  const renderAssetPreview = () => {
    if (!selectedAsset) return <div className="empty-state">选择一个素材查看详情和预览。</div>;
    const src = preview?.localPath ? convertFileSrc(preview.localPath) : "";
    return (
      <aside className="card marketing-preview">
        <div className="marketing-preview-media">
          {src && selectedAsset.assetType === "image" && <img src={src} alt={selectedAsset.fileName} />}
          {src && selectedAsset.assetType === "video" && <video src={src} controls />}
          {!src && <div className="marketing-preview-empty">{selectedAsset.extension || selectedAsset.assetType}</div>}
        </div>
        <div className="marketing-preview-info">
          <h3>{selectedAsset.fileName}</h3>
          <dl>
            <dt>类型</dt><dd>{selectedAsset.assetType} / {selectedAsset.mimeType}</dd>
            <dt>大小</dt><dd>{formatBytes(selectedAsset.fileSize)}</dd>
            <dt>位置</dt><dd>{selectedAsset.rootFolder}/{selectedAsset.relativePath}</dd>
            <dt>修改时间</dt><dd>{selectedAsset.modifiedAt}</dd>
            {(selectedAsset.width || selectedAsset.height) && <><dt>尺寸</dt><dd>{selectedAsset.width ?? "-"} × {selectedAsset.height ?? "-"}</dd></>}
            {selectedAsset.durationSeconds && <><dt>时长</dt><dd>{selectedAsset.durationSeconds}s</dd></>}
          </dl>
          <button className="btn-primary" onClick={() => void stageAssetToChat(selectedAsset)}>放入会话输入框</button>
        </div>
      </aside>
    );
  };

  return (
    <section className="marketing-page">
      <header className="feature-topbar">
        <div>
          <h1 className="feature-topbar-title">线上营销工作台</h1>
          <p className="marketing-subtitle">{view === "assets" ? "素材库文件、信息和预览已接入 WorkBuddy。" : "数据看板可直接发起小亨分析会话。"}</p>
        </div>
        <div className="marketing-actions">
          <span className="marketing-pill">{user.name} · {user.userRole}</span>
          <button className="btn-secondary" onClick={() => void loadAll()} disabled={loading}>刷新</button>
          <button className="btn-secondary" onClick={handleLogout}>退出</button>
        </div>
      </header>

      <div className="marketing-body">
        {message && <div className="marketing-notice">{message}</div>}
        {view === "assets" ? (
          <div className="marketing-assets-layout">
            <section className="card marketing-panel">
              <div className="marketing-panel-head">
                <div><span className="eyebrow">Asset Library</span><h2>素材库</h2></div>
                <span className="marketing-panel-foot">当前筛选 {assets.length} / {assetTotal} 条</span>
              </div>
              <div className="marketing-filters">
                <select className="input" value={folder} onChange={(event) => setFolder(event.target.value)}>
                  <option value="">全部文件夹</option>
                  {folders.map((item) => <option key={item} value={item}>{item}</option>)}
                </select>
                <input className="input" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索文件名、路径" />
                <button className="btn-secondary" onClick={() => void loadAll()}>搜索</button>
              </div>
              <div className="marketing-asset-cards">
                {assets.map((asset) => (
                  <button className={`marketing-asset-card ${selectedAsset?.id === asset.id ? "active" : ""}`} key={asset.id} onClick={() => void selectAsset(asset)}>
                    <span className="marketing-asset-type">{asset.assetType}</span>
                    <strong>{asset.fileName}</strong>
                    <small>{asset.rootFolder}</small>
                    <small>{formatBytes(asset.fileSize)} · {asset.extension || "file"}</small>
                  </button>
                ))}
                {!assets.length && <div className="empty-state">没有匹配素材。</div>}
              </div>
            </section>
            {renderAssetPreview()}
          </div>
        ) : (
          <>
            <div className="marketing-metrics">
              <div className="card marketing-metric"><span>素材总数</span><strong>{metric(totals.total_count)}</strong><small>{formatBytes(totals.total_size as number)}</small></div>
              <div className="card marketing-metric"><span>视频 / 图片</span><strong>{metric(totals.video_count)} / {metric(totals.image_count)}</strong><small>近 7 天新增 {metric(totals.week_count)}</small></div>
              <div className="card marketing-metric"><span>24h 播放/曝光</span><strong>{metric(platformTotals.views)}</strong><small>{platformTotals.posts} 条内容</small></div>
              <div className="card marketing-metric"><span>互动 / 消耗</span><strong>{metric(platformTotals.likes)} / {metric(platformTotals.spend)}</strong><small>跨平台汇总</small></div>
            </div>
            <section className="card marketing-panel">
              <div className="marketing-panel-head">
                <div><span className="eyebrow">Dashboard</span><h2>数据看板</h2></div>
                <button className="btn-primary" onClick={() => void sendDashboardToChat()}>新建会话分析</button>
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
                    <span>{String(field(post, "title", "title") || "未命名内容")}</span>
                    <b>{metric(field(post, "views", "views"))} 播放</b>
                  </div>
                ))}
              </div>
            </section>
          </>
        )}
      </div>
    </section>
  );
}
