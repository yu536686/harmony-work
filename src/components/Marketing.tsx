import { useEffect, useMemo, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useDispatch, useSelector } from "react-redux";
import { RootState, chatActions, sessionsActions } from "../store";
import {
  chatApi,
  marketingApi,
  marketingUserContext,
  openPath,
  readRememberedMarketingUser,
  sessionsApi,
  type MarketingAsset,
  type MarketingAssetSummary,
  type MarketingCampaignSummary,
  type MarketingPreparedAsset,
  type MarketingUser,
} from "../services/tauri";
import "./Marketing.css";
import { shouldShowAsset } from "./Marketing/assetFiltering";

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

function formatDuration(seconds: number | string | null | undefined) {
  const value = Number(seconds ?? 0);
  if (!Number.isFinite(value) || value <= 0) return "0:00";
  const mins = Math.floor(value / 60);
  const secs = Math.floor(value % 60);
  return `${mins}:${String(secs).padStart(2, "0")}`;
}

function assetDimensions(asset: MarketingAsset) {
  return asset.width && asset.height ? `${asset.width}x${asset.height}` : "";
}

function field<T = unknown>(source: Record<string, unknown> | undefined, camel: string, snake: string): T | undefined {
  return (source?.[camel] ?? source?.[snake]) as T | undefined;
}

const FORMAT_FILTERS = [
  { id: "image", label: "图片", extensions: ["jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "svg"] },
  { id: "video", label: "视频", extensions: ["mp4", "mov", "mkv", "avi", "webm", "m4v"] },
  { id: "audio", label: "音频", extensions: ["mp3", "wav", "m4a", "aac", "flac", "ogg"] },
  { id: "document", label: "文档", extensions: ["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "md", "csv"] },
  { id: "source", label: "工程源文件", extensions: ["psd", "ai", "aep", "prproj", "cdr", "fig"] },
  { id: "archive", label: "归档", extensions: ["zip", "rar", "7z", "tar", "gz"] },
] as const;

const DEFAULT_FORMAT_GROUPS = FORMAT_FILTERS.filter((item) => item.id !== "source").map((item) => item.id);
import { MARKETING_BASE as MARKETING_ASSET_BASE, REGISTER_URL, migratePublicServiceUrl } from "../services/serviceUrls";

function selectedExtensions(groups: string[]) {
  return new Set<string>(
    FORMAT_FILTERS
      .filter((item) => groups.includes(item.id))
      .flatMap((item) => item.extensions),
  );
}

function assetMatchesFormat(asset: MarketingAsset, groups: string[]) {
  if (!groups.length) return false;
  const extensions = selectedExtensions(groups);
  const ext = (asset.extension || asset.fileName.split(".").pop() || "").replace(/^\./, "").toLowerCase();
  if (extensions.has(ext)) return true;
  return groups.includes(asset.assetType);
}

function assetKindIcon(asset: MarketingAsset) {
  if (asset.assetType === "image") return "🖼";
  if (asset.assetType === "video") return "▶";
  if (asset.assetType === "audio") return "♪";
  if (asset.assetType === "document") return "DOC";
  return (asset.extension || "FILE").slice(0, 4).toUpperCase();
}

function assetPreviewUrl(asset: MarketingAsset) {
  const raw = asset.previewUrl;
  if (!raw) return "";
  return raw.startsWith("http://") || raw.startsWith("https://")
    ? migratePublicServiceUrl(raw)
    : `${MARKETING_ASSET_BASE}${raw.startsWith("/") ? raw : `/${raw}`}`;
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
  const [previewDataUrl, setPreviewDataUrl] = useState("");
  const [thumbnailDataUrls, setThumbnailDataUrls] = useState<Record<string, string>>({});
  const [thumbnailFailures, setThumbnailFailures] = useState<Record<string, string>>({});
  const [message, setMessage] = useState("");
  const [formatGroups, setFormatGroups] = useState<string[]>(DEFAULT_FORMAT_GROUPS);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewError, setPreviewError] = useState("");

  const active = user?.approvalStatus === "active";

  const loadAll = async () => {
    if (!active) return;
    setLoading(true);
    setMessage("");
    try {
      const selectedGroups = formatGroups.length ? formatGroups : [];
      const assetRequests = selectedGroups.length
        ? selectedGroups.map((group) => marketingApi.assets({ folder, q: query, formatGroup: group, includeAuxiliary: false, limit: 96, sort: "modified_desc" }))
        : [Promise.resolve({ assets: [], total: 0 })];
      const [nextSummary, nextFolders, nextAssetGroups, nextCampaign] = await Promise.all([
        marketingApi.summary(),
        marketingApi.folders(),
        Promise.all(assetRequests),
        marketingApi.campaignSummary(1440),
      ]);
      const merged = new Map<string, MarketingAsset>();
      let total = 0;
      for (const groupResult of nextAssetGroups) {
        total += Number(groupResult.total ?? 0);
        for (const asset of groupResult.assets ?? []) {
          merged.set(asset.id, asset);
        }
      }
      const filteredAssets = Array.from(merged.values())
        .filter((asset) => shouldShowAsset(asset, false))
        .filter((asset) => assetMatchesFormat(asset, selectedGroups));
      const nextAssets = filteredAssets
        .sort((a, b) => String(b.modifiedAt ?? "").localeCompare(String(a.modifiedAt ?? "")))
        .slice(0, 48);
      setSummary(nextSummary);
      setFolders(nextFolders.folders ?? []);
      setAssets(nextAssets);
      setAssetTotal(filteredAssets.length || total);
      setCampaign(nextCampaign);
      if (selectedAsset && !merged.has(selectedAsset.id)) {
        setSelectedAsset(nextAssets[0] ?? null);
        setPreview(null);
      } else if (!selectedAsset && nextAssets[0]) {
        setSelectedAsset(nextAssets[0]);
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
  }, [active, folder, view, formatGroups]);

  useEffect(() => {
    if (!assets.length) return;
    let cancelled = false;
    const missingImages = assets
      .filter((asset) => asset.assetType === "image" && !thumbnailDataUrls[asset.id] && !thumbnailFailures[asset.id])
      .slice(0, 36);
    if (!missingImages.length) return;
    void (async () => {
      const next: Record<string, string> = {};
      const failed: Record<string, string> = {};
      for (const asset of missingImages) {
        try {
          const data = await marketingApi.assetDataUrl(asset.id, asset.fileName, 12 * 1024 * 1024);
          if (data.mediaType.startsWith("image/")) {
            next[asset.id] = data.dataUrl;
          } else {
            failed[asset.id] = data.mediaType || "unsupported";
          }
        } catch (error) {
          failed[asset.id] = String(error);
        }
        if (cancelled) return;
      }
      if (!cancelled && Object.keys(next).length) {
        setThumbnailDataUrls((prev) => ({ ...prev, ...next }));
      }
      if (!cancelled && Object.keys(failed).length) {
        setThumbnailFailures((prev) => ({ ...prev, ...failed }));
      }
    })();
    return () => { cancelled = true; };
  }, [assets, thumbnailDataUrls, thumbnailFailures]);

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
      if (next.approvalStatus === "pending") {
        setLoginError("账号已注册，但尚未通过管理员审批。审批通过后即可访问素材库、市场和团队功能。");
      } else if (next.approvalStatus === "disabled") {
        setLoginError("账号当前已停用，请联系管理员处理。");
      }
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
    setPreviewLoading(true);
    setPreviewError("");
    try {
      const prepared = await marketingApi.prepareAssetForChat(asset.id, asset.fileName, settings?.workspace_root);
      setPreview(prepared);
      return prepared;
    } finally {
      setPreviewLoading(false);
    }
  };

  const prepareInlinePreview = async (asset: MarketingAsset) => {
    setPreviewLoading(true);
    setPreviewError("");
    setPreviewDataUrl("");
    try {
      const maxBytes = asset.assetType === "video" ? 32 * 1024 * 1024 : 16 * 1024 * 1024;
      const data = await marketingApi.assetDataUrl(asset.id, asset.fileName, maxBytes);
      setPreviewDataUrl(data.dataUrl);
      if (data.mediaType.startsWith("image/")) {
        setThumbnailDataUrls((prev) => ({ ...prev, [asset.id]: data.dataUrl }));
      }
      return data;
    } finally {
      setPreviewLoading(false);
    }
  };

  const toggleFormatGroup = (group: string) => {
    setFormatGroups((prev) => (
      prev.includes(group)
        ? prev.filter((item) => item !== group)
        : [...prev, group]
    ));
  };

  const selectAsset = async (asset: MarketingAsset) => {
    setSelectedAsset(asset);
    setPreview(null);
    setPreviewDataUrl("");
    setPreviewError("");
    if (asset.assetType === "image" || asset.assetType === "video") {
      try {
        await prepareInlinePreview(asset);
      } catch (error) {
        try {
          await prepareAsset(asset);
        } catch {
          setPreviewError(String(error));
          setMessage(String(error));
        }
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

  const openSelectedAsset = async (asset: MarketingAsset) => {
    setMessage(`正在准备打开：${asset.fileName}`);
    try {
      const prepared = preview?.assetId === asset.id ? preview : await prepareAsset(asset);
      await openPath(prepared.localPath);
      setMessage("");
    } catch (error) {
      setPreviewError(String(error));
      setMessage(String(error));
    }
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
      "请作为线上营销数据分析 Agent，基于以下 Harmony Work 原生数据看板摘要，输出风险、机会、下一步投放/内容动作，并给出优先级。",
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
            <button className="btn btn-primary" disabled={loading || !email.trim() || !password}>登录</button>
            <button className="btn btn-secondary" type="button" onClick={() => void openPath(REGISTER_URL)}>注册账号</button>
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
          <button className="btn btn-secondary" onClick={handleLogout}>退出登录</button>
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
    const src = previewDataUrl || (preview?.localPath ? convertFileSrc(preview.localPath) : "");
    const canInlinePreview = selectedAsset.assetType === "image" || selectedAsset.assetType === "video";
    return (
      <aside className="card marketing-preview">
        <div className="marketing-preview-media">
          {previewLoading && <div className="marketing-preview-empty">加载预览...</div>}
          {!previewLoading && src && selectedAsset.assetType === "image" && (
            <img src={src} alt={selectedAsset.fileName} onError={() => setPreviewError("图片预览加载失败，可点击打开文件用系统应用查看。")} />
          )}
          {!previewLoading && src && selectedAsset.assetType === "video" && (
            <video src={src} controls preload="metadata" onError={() => setPreviewError("视频预览加载失败，可点击打开文件用系统播放器查看。")} />
          )}
          {!previewLoading && !src && <div className="marketing-preview-empty">{assetKindIcon(selectedAsset)}</div>}
        </div>
        <div className="marketing-preview-info">
          <h3>{selectedAsset.fileName}</h3>
          {canInlinePreview && !preview && !previewLoading && !previewError && <p className="marketing-preview-hint">选择素材后会先缓存到本地，再使用本地文件进行安全预览。</p>}
          {previewError && <div className="marketing-preview-warning">{previewError}</div>}
          <dl>
            <dt>类型</dt><dd>{selectedAsset.assetType} / {selectedAsset.mimeType}</dd>
            <dt>大小</dt><dd>{formatBytes(selectedAsset.fileSize)}</dd>
            <dt>位置</dt><dd>{selectedAsset.rootFolder}/{selectedAsset.relativePath}</dd>
            <dt>修改时间</dt><dd>{selectedAsset.modifiedAt}</dd>
            {(selectedAsset.width || selectedAsset.height) && <><dt>尺寸</dt><dd>{selectedAsset.width ?? "-"} × {selectedAsset.height ?? "-"}</dd></>}
            {selectedAsset.durationSeconds && <><dt>时长</dt><dd>{selectedAsset.durationSeconds}s</dd></>}
          </dl>
          <div className="marketing-preview-actions">
            <button className="btn btn-primary" onClick={() => void stageAssetToChat(selectedAsset)}>放入会话输入框</button>
            <button
              className="btn btn-secondary"
              onClick={() => void openSelectedAsset(selectedAsset)}
            >
              系统打开
            </button>
          </div>
        </div>
      </aside>
    );
  };

  return (
    <section className="marketing-page">
      <header className="feature-topbar">
        <div>
          <h1 className="feature-topbar-title">线上营销工作台</h1>
          <p className="marketing-subtitle">{view === "assets" ? "素材库文件、信息和预览已接入 Harmony Work。" : "数据看板可直接发起小亨分析会话。"}</p>
        </div>
        <div className="marketing-actions">
          <span className="marketing-pill">{user.name} · {user.userRole}</span>
          <button className="btn btn-secondary" onClick={() => void loadAll()} disabled={loading}>刷新</button>
          <button className="btn btn-secondary" onClick={handleLogout}>退出</button>
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
                <button className="btn btn-secondary" onClick={() => void loadAll()}>搜索</button>
              </div>
              <div className="marketing-filter-caption">文件类型</div>
              <div className="marketing-format-filter" aria-label="格式筛选">
                {FORMAT_FILTERS.map((item) => (
                  <label key={item.id} title={item.extensions.join(", ")}>
                    <input
                      type="checkbox"
                      checked={formatGroups.includes(item.id)}
                      onChange={() => toggleFormatGroup(item.id)}
                    />
                    <span>{item.label}</span>
                    <small>{item.extensions.slice(0, 4).join("/")}</small>
                  </label>
                ))}
              </div>
              <div className="marketing-asset-cards">
                {assets.map((asset) => {
                  const previewUrl = thumbnailDataUrls[asset.id] || (thumbnailFailures[asset.id] === "preview-load-failed" ? "" : assetPreviewUrl(asset));
                  const dimensions = assetDimensions(asset);
                  return (
                  <button className={`marketing-asset-card asset-${asset.assetType} ${selectedAsset?.id === asset.id ? "active" : ""}`} key={asset.id} title={`${asset.fileName}\n${asset.rootFolder}/${asset.relativePath}`} onClick={() => void selectAsset(asset)}>
                    <span className="marketing-asset-select"><i />选择</span>
                    <span className="marketing-asset-media">
                      {previewUrl ? (
                        <img
                          className="marketing-asset-thumb"
                          src={previewUrl}
                          alt=""
                          loading="lazy"
                          draggable={false}
                          onError={() => setThumbnailFailures((prev) => ({ ...prev, [asset.id]: "preview-load-failed" }))}
                        />
                      ) : (
                        <span className="marketing-asset-placeholder">{assetKindIcon(asset)}</span>
                      )}
                      <span className="marketing-asset-type">{asset.assetType}</span>
                    </span>
                    <span className="marketing-asset-info">
                      <strong>{asset.fileName}</strong>
                      <small>{asset.rootFolder || "素材库"}</small>
                      <small>{[formatBytes(asset.fileSize), formatDuration(asset.durationSeconds), dimensions].filter(Boolean).join(" · ")}</small>
                    </span>
                  </button>
                  );
                })}
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
                <button className="btn btn-primary" onClick={() => void sendDashboardToChat()}>新建会话分析</button>
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
