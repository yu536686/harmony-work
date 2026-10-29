import { useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
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
import { watchLibraryApi } from "../services/tauri/watchLibrary";

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
import { REGISTER_URL } from "../services/serviceUrls";

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

function sortAssets(a: MarketingAsset, b: MarketingAsset, sort: string) {
  switch (sort) {
    case "modified_asc":
      return String(a.modifiedAt ?? "").localeCompare(String(b.modifiedAt ?? ""));
    case "name_asc":
      return a.fileName.localeCompare(b.fileName);
    case "size_desc":
      return Number(b.fileSize ?? 0) - Number(a.fileSize ?? 0);
    case "modified_desc":
    default:
      return String(b.modifiedAt ?? "").localeCompare(String(a.modifiedAt ?? ""));
  }
}

function assetKindIcon(asset: MarketingAsset) {
  if (asset.assetType === "image") return "🖼";
  if (asset.assetType === "video") return "▶";
  if (asset.assetType === "audio") return "♪";
  if (asset.assetType === "document") return "DOC";
  return (asset.extension || "FILE").slice(0, 4).toUpperCase();
}

function isPdf(asset: MarketingAsset) {
  return (asset.extension || "").trim().toLowerCase().replace(/^\.+/, "") === "pdf";
}

function canInlinePreview(asset: MarketingAsset) {
  return (
    asset.assetType === "image" ||
    asset.assetType === "video" ||
    asset.assetType === "audio" ||
    (asset.assetType === "document" && isPdf(asset))
  );
}

function assetPreviewUrl(asset: MarketingAsset) {
  const raw = asset.previewUrl;
  if (!raw) return "";
  try {
    const url = new URL(raw);
    return url.pathname.startsWith("/api/assets/direct/") && url.searchParams.has("ticket")
      ? url.toString()
      : "";
  } catch {
    return "";
  }
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
  const [sort, setSort] = useState("modified_desc");
  const [pageOffset, setPageOffset] = useState(48);
  const [hasMore, setHasMore] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const requestSeq = useRef(0);
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
  const [uploading, setUploading] = useState(false);

  const active = user?.approvalStatus === "active" && user.accountType === "marketing";

  const loadAll = async () => {
    if (!active) return;
    const requestId = ++requestSeq.current;
    setLoading(true);
    setMessage("");
    try {
      const selectedGroups = formatGroups.length ? formatGroups : [];
      const assetRequests = selectedGroups.length
        ? selectedGroups.map((group) => marketingApi.assets({ folder, q: query, formatGroup: group, includeAuxiliary: false, limit: 48, offset: 0, sort }))
        : [Promise.resolve({ assets: [], total: 0 })];
      const [nextSummary, nextFolders, nextAssetGroups, nextCampaign] = await Promise.all([
        marketingApi.summary(),
        marketingApi.folders(),
        Promise.all(assetRequests),
        marketingApi.campaignSummary(1440),
      ]);
      if (requestId !== requestSeq.current) return;
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
      const nextAssets = filteredAssets.sort((a, b) => sortAssets(a, b, sort));
      setSummary(nextSummary);
      setFolders(nextFolders.folders ?? []);
      setAssets(nextAssets);
      setThumbnailFailures({});
      setAssetTotal(total);
      setPageOffset(48);
      setHasMore(nextAssetGroups.some((group) => group.total > 48));
      setCampaign(nextCampaign);
      if (selectedAsset && !nextAssets.some((asset) => asset.id === selectedAsset.id)) {
        setSelectedAsset(nextAssets[0] ?? null);
        setPreview(null);
        setPreviewDataUrl("");
      } else if (!selectedAsset && nextAssets[0]) {
        setSelectedAsset(nextAssets[0]);
      }
    } catch (error) {
      if (requestId === requestSeq.current) setMessage(String(error));
    } finally {
      if (requestId === requestSeq.current) setLoading(false);
    }
  };

  const loadMore = async () => {
    if (!active || !hasMore || loadingMore) return;
    const requestId = requestSeq.current;
    const selectedGroups = formatGroups;
    setLoadingMore(true);
    try {
      const groups = await Promise.all(selectedGroups.map((group) => marketingApi.assets({
        folder, q: query, formatGroup: group, includeAuxiliary: false, limit: 48, offset: pageOffset, sort,
      })));
      if (requestId !== requestSeq.current) return;
      setAssets((previous) => {
        const merged = new Map(previous.map((asset) => [asset.id, asset]));
        for (const group of groups) {
          for (const asset of group.assets) {
            if (shouldShowAsset(asset, false) && assetMatchesFormat(asset, selectedGroups)) merged.set(asset.id, asset);
          }
        }
        return Array.from(merged.values()).sort((a, b) => sortAssets(a, b, sort));
      });
      setHasMore(groups.some((group) => group.total > pageOffset + 48));
      setPageOffset(pageOffset + 48);
    } catch (error) {
      if (requestId === requestSeq.current) setMessage(String(error));
    } finally {
      setLoadingMore(false);
    }
  };

  useEffect(() => {
    void loadAll();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, folder, view, formatGroups, sort]);

  useEffect(() => {
    if (!active || view !== "assets") return;
    const timer = window.setInterval(() => void loadAll(), 8 * 60 * 1000);
    return () => window.clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, view, folder, formatGroups, sort]);

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
    if (asset.assetType === "video" && assetPreviewUrl(asset)) return;
    if (canInlinePreview(asset)) {
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

  const uploadAssets = async () => {
    const selected = await openFileDialog({ multiple: true, directory: false });
    const paths = Array.isArray(selected) ? selected : selected ? [selected] : [];
    if (!paths.length) return;
    setUploading(true);
    setMessage(`正在创建 ${paths.length} 个上传任务…`);
    try {
      const tasks = await watchLibraryApi.marketingUploadStart(paths, folder);
      window.dispatchEvent(new Event("harmony:watch-uploads-open"));
      setMessage(`已创建 ${tasks.length} 个上传任务，可在上传窗口查看进度。`);
    } catch (error) {
      setMessage(`上传失败：${String(error)}`);
    } finally {
      setUploading(false);
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

  const saveSelectedAssetToWorkspace = async (asset: MarketingAsset) => {
    setMessage(`正在保存素材到工作区：${asset.fileName}`);
    try {
      const prepared = preview?.assetId === asset.id ? preview : await prepareAsset(asset);
      setMessage(`素材已保存到工作区：${prepared.localPath}`);
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
            <h2>登录营销用户账号以访问素材库和数据看板</h2>
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
            <h2>{user.name}，当前账号无法访问线上营销工作台</h2>
            <p>{user.approvalStatus !== "active" ? "请等待管理员启用账号。" : "请联系管理员将账号设为营销用户。"}</p>
          </div>
        </div>
      </section>
    );
  }

  const renderAssetPreview = () => {
    if (!selectedAsset) return <div className="empty-state">选择一个素材查看详情和预览。</div>;
    const src = previewDataUrl || (preview?.localPath ? convertFileSrc(preview.localPath) : "")
      || (selectedAsset.assetType === "video" ? assetPreviewUrl(selectedAsset) : "");
    const inline = canInlinePreview(selectedAsset);
    return (
      <aside className="card marketing-preview">
        <div className="marketing-preview-media">
          {previewLoading && <div className="marketing-preview-empty">加载预览...</div>}
          {!previewLoading && src && selectedAsset.assetType === "image" && (
            <img src={src} alt={selectedAsset.fileName} onError={() => setPreviewError("图片预览加载失败，可点击打开文件用系统应用查看。")} />
          )}
          {!previewLoading && src && selectedAsset.assetType === "video" && (
            <video
              src={src}
              controls
              preload="metadata"
              poster={thumbnailDataUrls[selectedAsset.id] || assetPreviewUrl(selectedAsset) || undefined}
              onError={() => setPreviewError("视频预览加载失败，可点击打开文件用系统播放器查看。")}
            />
          )}
          {!previewLoading && src && selectedAsset.assetType === "audio" && (
            <audio src={src} controls preload="metadata" onError={() => setPreviewError("音频预览加载失败，可点击打开文件用系统播放器查看。")} />
          )}
          {!previewLoading && src && selectedAsset.assetType === "document" && isPdf(selectedAsset) && (
            <iframe
              src={src}
              title={selectedAsset.fileName}
              className="marketing-preview-pdf"
              onError={() => setPreviewError("PDF 预览加载失败，可点击打开文件用系统应用查看。")}
            />
          )}
          {!previewLoading && !src && <div className="marketing-preview-empty">{assetKindIcon(selectedAsset)}</div>}
        </div>
        <div className="marketing-preview-info">
          <h3>{selectedAsset.fileName}</h3>
          {inline && !preview && !previewLoading && !previewError && <p className="marketing-preview-hint">素材通过公司直连通道预览。</p>}
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
            <button className="btn btn-secondary" onClick={() => void saveSelectedAssetToWorkspace(selectedAsset)}>保存到工作区</button>
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
                <div className="marketing-actions">
                  <span className="marketing-panel-foot">当前筛选 {assets.length} / {assetTotal} 条</span>
                  <button className="btn btn-secondary" onClick={() => void uploadAssets()} disabled={uploading}>
                    {uploading ? "准备中…" : "上传素材"}
                  </button>
                  <button className="btn btn-secondary" onClick={() => window.dispatchEvent(new Event("harmony:watch-uploads-open"))}>上传任务</button>
                </div>
              </div>
              <div className="marketing-filters">
                <select className="input" value={folder} onChange={(event) => setFolder(event.target.value)}>
                  <option value="">全部文件夹</option>
                  {folders.map((item) => <option key={item} value={item}>{item}</option>)}
                </select>
                <select className="input" value={sort} onChange={(event) => setSort(event.target.value)} aria-label="排序">
                  <option value="modified_desc">最近更新</option>
                  <option value="name_asc">名称 A-Z</option>
                  <option value="size_desc">大小降序</option>
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
              {hasMore && (
                <div className="marketing-load-more">
                  <button className="btn btn-secondary" onClick={() => void loadMore()} disabled={loadingMore}>
                    {loadingMore ? "正在加载…" : `加载更多（已显示 ${assets.length} / ${assetTotal}）`}
                  </button>
                </div>
              )}
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
