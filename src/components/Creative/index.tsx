import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { migratePublicServiceUrl } from "../../services/serviceUrls";
import { extractHarvisResultUrl } from "./harvisRefresh";
import { useHarvisTasks } from "./useHarvisTasks";
import { isCurrentMarketingSession } from "../../services/tauri/marketing";
import {
  creativeApi,
  normalizeReferenceSelection,
  resolveCanvasRequest,
  type MusicAssistantConfig,
  type MarketingUser,
  type HarvisTask,
  addHarvisTask,
  updateHarvisFeedback
} from "../../services/tauri";
import "./Creative.css";

export type CreativePageProps = {
  initialKind: "music" | "image";
  user: MarketingUser | null;
  focusedTaskId?: string;
  focusResult?: boolean;
};

type ReferenceAttachment = {
  id: string;
  referenceUrl?: string;
  fileName: string;
  previewUrl: string;
};

type AssistantPromptResult = {
  analysisZh: string;
  confidence: number | null;
  promptZh: string;
  warnings: string[];
  degraded: boolean;
  evidenceCases: Array<{ id: string; title: string; genre?: string }>;
};

const fallbackWatchCatalog: Record<string, string[]> = {
  "宝玑": ["Classique", "Marine", "Type XX", "Reine de Naples", "Tradition", "Héritage"],
  "宝珀": ["Fifty Fathoms", "Villeret", "Ladybird", "Air Command", "Métiers d'Art"],
  "萧邦": ["Happy Sport", "Alpine Eagle", "L.U.C", "Mille Miglia", "Imperiale", "Happy Diamonds"],
  "欧米茄": ["Speedmaster", "Seamaster", "Constellation", "De Ville"],
  "格拉苏蒂原创": ["Senator", "Pano", "Spezialist", "Vintage", "Serenade"],
  "雅典表": ["Freak", "Blast", "Diver", "Marine"],
  "劳力士": ["Datejust", "Submariner", "Cosmograph Daytona", "GMT-Master II", "Day-Date", "Oyster Perpetual", "Explorer", "Yacht-Master"],
  "帝舵": ["Black Bay", "Pelagos", "Ranger", "TUDOR Royal", "1926", "Clair de Rose"],
  "浪琴": ["Conquest", "HydroConquest", "Spirit", "Master Collection", "Heritage", "DolceVita", "PrimaLuna"],
  "天梭": ["PRX", "Seastar", "Le Locle", "T-Touch Connect Solar", "Gentleman", "PR 100", "T-Race"],
  "沛纳海": ["Luminor", "Luminor Due", "Radiomir", "Submersible"]
};

const fallbackMusicConfig: MusicAssistantConfig = {
  businessLines: [
    { id: "watch-poetry", title: "腕表如诗", description: "以单只腕表、系列和产品细节为核心的品牌视频配乐。", fields: [{ key: "brand", label: "品牌", type: "select", required: true, options: [] }, { key: "collection", label: "腕表系列", type: "select", required: true, options: [] }, { key: "model", label: "具体型号或表款", type: "text", required: true, options: [] }, { key: "theme", label: "视频主题", type: "text", required: true, options: [] }, { key: "watchStyle", label: "腕表风格", type: "select", required: false, options: ["多钻", "经典", "时尚", "大气", "复杂结构"] }, { key: "desiredStyle", label: "期望风格", type: "text", required: false, options: [] }, { key: "bpm", label: "目标 BPM", type: "number", required: false, options: [] }] },
    { id: "flowers-in-bloom", title: "百花齐放", description: "面向城市、活动、节气和人群氛围的品牌内容配乐。", fields: [{ key: "style", label: "风格", type: "text", required: true, options: [] }, { key: "province", label: "省份", type: "text", required: false, options: [] }, { key: "city", label: "城市", type: "text", required: false, options: [] }, { key: "eventTheme", label: "活动主题", type: "text", required: true, options: [] }] },
    { id: "counters-like-painting", title: "柜台似画", description: "面向门店陈列、柜台空间和多表展示的宣传配乐。", fields: [{ key: "brand", label: "品牌", type: "select", required: false, options: [] }, { key: "city", label: "城市", type: "text", required: true, options: [] }, { key: "displayTheme", label: "陈列主题", type: "text", required: true, options: [] }] },
    { id: "time-sings", title: "岁月如歌", description: "面向品牌历史、当下故事和未来收束的叙事型宣传配乐。", fields: [{ key: "narrativeStage", label: "叙事阶段", type: "select", required: true, options: ["往昔溯源", "当下绽放", "未来永续"] }, { key: "theme", label: "主题", type: "text", required: true, options: [] }, { key: "brand", label: "品牌", type: "select", required: false, options: [] }] }
  ],
  brands: Object.entries(fallbackWatchCatalog).map(([name, collections], index) => ({ id: `fallback-${index}`, name, collections }))
};

const canvasOptions = [
  { id: "television", title: "电视屏幕", detail: "16:9" },
  { id: "vertical", title: "竖屏", detail: "9:16" },
  { id: "store-poster", title: "门店海报", detail: "420 x 592" },
  { id: "phone-fullscreen", title: "手机全屏", detail: "2868 x 1320" },
  { id: "custom", title: "自定义", detail: "填写输出像素" }
] as const;

const imageTemplates = [
  ["新品主视觉海报", "高级腕表主视觉，干净棚拍，香槟金与象牙白背景，预留品牌文案区域，精细金属与钻石质感"],
  ["活动邀请函", "高端腕表活动邀请函背景，高级纸张与压纹，优雅留白，局部金属光泽，不出现文字"],
  ["腕表细节海报", "腕表微距产品摄影，表盘和表耳细节，真实金属反射，奢华但克制，商业广告质感"],
  ["节日社媒图", "节日氛围腕表社交媒体视觉，柔和灯光与节庆元素，品牌级构图，保留安全文案区域"]
] as const;

function readPreview(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error(`无法读取 ${file.name}`));
    reader.readAsDataURL(file);
  });
}

export default function CreativePage({ initialKind, user, focusedTaskId, focusResult = false }: CreativePageProps) {
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [downloadingTaskId, setDownloadingTaskId] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ fileName: string; kind: "audio" | "image"; dataUrl: string; task: HarvisTask } | null>(null);
  const [previewLoading, setPreviewLoading] = useState<string | null>(null);
  const previewRequest = useRef(0);
  const focusedTarget = useRef("");
  const taskListRef = useRef<HTMLDivElement | null>(null);
  const { tasks: harvisTasks, refresh: refreshHarvisTasks, session } = useHarvisTasks(initialKind, Boolean(user));
  const [showMusicConfig, setShowMusicConfig] = useState(false);
  const [adminBrandName, setAdminBrandName] = useState("");
  const [adminCollections, setAdminCollections] = useState("");

  const [musicMode, setMusicMode] = useState<"free" | "assistant">("free");
  const [musicTitle, setMusicTitle] = useState("萧邦 Happy Sport 品牌 BGM");
  const [musicTags, setMusicTags] = useState("Luxury fashion instrumental, 102 BPM, elegant high-end, refined piano motif, subtle string pads, clean wide mix, instrumental only, completely no vocals");
  const [musicDuration, setMusicDuration] = useState(45);
  const [musicConfig, setMusicConfig] = useState<MusicAssistantConfig>(fallbackMusicConfig);
  const [businessLine, setBusinessLine] = useState("腕表如诗");
  const [brand, setBrand] = useState("萧邦");
  const [watchCollection, setWatchCollection] = useState("Happy Sport");
  const [watchStyle, setWatchStyle] = useState("");
  const [desiredStyle, setDesiredStyle] = useState("");
  const [bpm, setBpm] = useState("");
  const [assistantDetails, setAssistantDetails] = useState<Record<string, string>>({});
  const [assistantPromptResult, setAssistantPromptResult] = useState<AssistantPromptResult | null>(null);

  const [imageMode, setImageMode] = useState<"text" | "image">("text");
  const [imagePrompt, setImagePrompt] = useState("一枚 Happy Sport 女士腕表置于柔和的香槟金丝绸上，光影精致，广告摄影，留白适合放置品牌文案。");
  const [canvasPreset, setCanvasPreset] = useState<"television" | "vertical" | "store-poster" | "phone-fullscreen" | "custom">("vertical");
  const [canvasWidth, setCanvasWidth] = useState(1024);
  const [canvasHeight, setCanvasHeight] = useState(1024);
  const [referenceFiles, setReferenceFiles] = useState<ReferenceAttachment[]>([]);
  const [referenceDragActive, setReferenceDragActive] = useState(false);
  const fileInputRef = useRef<HTMLInputElement | null>(null);

  const selectedBusinessLine = musicConfig.businessLines.find((line) => line.title === businessLine) ?? musicConfig.businessLines[0] ?? fallbackMusicConfig.businessLines[0];
  const selectedBrand = musicConfig.brands.find((item) => item.name === brand) ?? musicConfig.brands[0];
  const collections = selectedBrand?.collections ?? [];
  const hasAssistantField = (key: string) => selectedBusinessLine.fields.some((field) => field.key === key);
  const additionalAssistantFields = selectedBusinessLine.fields.filter((field) => !["brand", "collection", "model", "theme", "watchStyle", "desiredStyle", "bpm", "referenceAssets"].includes(field.key));
  const referenceIds = useMemo(() => normalizeReferenceSelection(referenceFiles.map((item) => item.id)), [referenceFiles]);

  useEffect(() => {
    if (!user) return;
    void creativeApi.musicConfig()
      .then((config) => {
        if (!config.businessLines.length || !config.brands.length) return;
        setMusicConfig(config);
        const line = config.businessLines.find((item) => item.title === businessLine) ?? config.businessLines[0];
        const nextBrand = config.brands.find((item) => item.name === brand) ?? config.brands[0];
        setBusinessLine(line.title);
        setBrand(nextBrand.name);
        setWatchCollection(nextBrand.collections.includes(watchCollection) ? watchCollection : (nextBrand.collections[0] ?? ""));
      })
      .catch(() => setNotice("音乐助手配置暂不可用，已使用本地回退配置"));
  }, [user?.id]);

  const activeHarvisTasks = harvisTasks.filter((task) => task.kind === initialKind && (task.status === "pending" || task.status === "processing"));
  const filteredHarvisTasks = harvisTasks.filter((task) => task.kind === initialKind);
  const updateLocalTask = (taskId: string, changes: Pick<Partial<HarvisTask>, "adopted" | "rating" | "feedbackTags">) => updateHarvisFeedback(taskId, initialKind, changes, session);

  const updateBrand = (nextBrand: string) => {
    const nextCollection = musicConfig.brands.find((item) => item.name === nextBrand)?.collections[0] ?? "";
    setBrand(nextBrand);
    setWatchCollection(nextCollection);
    setMusicTitle(`${nextBrand} ${nextCollection} 品牌 BGM`);
  };

  const updateBusinessLine = (nextBusinessLine: string) => {
    setBusinessLine(nextBusinessLine);
    setAssistantDetails({});
    setAssistantPromptResult(null);
  };

  const assistantFieldValue = (key: string) => ({
    brand,
    collection: watchCollection,
    watchStyle,
    desiredStyle,
    bpm,
    ...assistantDetails
  })[key]?.trim?.() ?? "";

  const generateAssistantPrompt = async () => {
    const missing = selectedBusinessLine.fields.find((field) => field.required && !assistantFieldValue(field.key));
    if (missing) {
      setNotice(`请填写${missing.label}`);
      return;
    }
    setBusy(true);
    setNotice("");
    try {
      const response = await creativeApi.composeMusicPrompt({
        businessLine,
        brand,
        watchCollection,
        model: assistantDetails.model || undefined,
        theme: assistantDetails.theme || undefined,
        watchStyle: watchStyle || undefined,
        desiredStyle: desiredStyle || undefined,
        bpm: bpm ? Number(bpm) : undefined,
        duration: musicDuration,
        details: assistantDetails
      });
      setMusicTags(response.prompt);
      setAssistantPromptResult({
        analysisZh: response.analysisZh?.trim() || "已根据业务信息生成可编辑的 Suno 纯音乐提示词。",
        confidence: typeof response.confidence === "number" ? Math.max(0, Math.min(1, response.confidence)) : null,
        promptZh: response.promptZh?.trim() || "当前模型未返回中文说明，可直接以英文提示词提交制作。",
        warnings: response.warnings ?? [],
        degraded: Boolean(response.degraded),
        evidenceCases: response.evidenceCases ?? []
      });
      if (response.degraded) setNotice("已生成可编辑的提示词，请检查内容后使用");
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const createMusic = async () => {
    const owner = session;
    if (!owner.userId || owner.userId !== user?.id || !isCurrentMarketingSession(owner)) return;
    if (!musicTags.trim()) {
      setNotice("请先填写提示词，或在助手模式中点击“生成提示词”");
      return;
    }
    setBusy(true);
    setNotice("");
    try {
      const result = await creativeApi.harvisGenerate({
        kind: "music",
        prompt: musicTags.trim(),
        title: musicTitle.trim() || `${brand} ${watchCollection} 品牌 BGM`,
        duration: musicDuration,
        mode: musicMode
      });
      if (!isCurrentMarketingSession(owner)) return;
      const taskId = (result as any)?.task_id ?? (result as any)?.id ?? "";
      const resultUrl = extractHarvisResultUrl(result);
      if (taskId || resultUrl) {
        const task: HarvisTask = {
          taskId: taskId || `music-${Date.now()}`,
          kind: "music",
          prompt: musicTags.trim(),
          status: resultUrl ? "succeeded" : "pending",
          progress: resultUrl ? 100 : 0,
          resultUrl,
          createdAt: Date.now(),
        };
        addHarvisTask(task, owner);
      } else {
        throw new Error("未收到创作任务或成品，请稍后重试");
      }
    } catch (error) {
      if (isCurrentMarketingSession(owner)) setNotice(String(error));
    } finally {
      if (isCurrentMarketingSession(owner)) setBusy(false);
    }
  };

  const createImage = async () => {
    const owner = session;
    if (!owner.userId || owner.userId !== user?.id || !isCurrentMarketingSession(owner)) return;
    if (!imagePrompt.trim()) {
      setNotice("请填写图像提示词");
      return;
    }
    if (imageMode === "image" && !referenceIds.length) {
      setNotice("图生图需要至少添加一张参考图");
      return;
    }
    setBusy(true);
    setNotice("");
    try {
      const canvas = resolveCanvasRequest(canvasPreset, canvasWidth, canvasHeight);
      const result = await creativeApi.harvisGenerate({
        kind: "image",
        mode: imageMode,
        prompt: imagePrompt.trim(),
        canvasPreset: canvas.canvasPreset,
        outputWidth: canvas.outputWidth,
        outputHeight: canvas.outputHeight,
        referenceIds,
        referenceUrls: imageMode === "image" ? referenceFiles.map((file) => file.referenceUrl).filter(Boolean) : []
      });
      if (!isCurrentMarketingSession(owner)) return;
      const taskId = (result as any)?.task_id ?? (result as any)?.id ?? "";
      const resultUrl = extractHarvisResultUrl(result) ?? "";
      if (resultUrl) {
        const task: HarvisTask = {
          taskId: taskId || `image-${Date.now()}`,
          kind: "image",
          prompt: imagePrompt.trim(),
          status: "succeeded",
          progress: 100,
          resultUrl,
          createdAt: Date.now(),
        };
        addHarvisTask(task, owner);
      } else if (taskId) {
        const task: HarvisTask = {
          taskId,
          kind: "image",
          prompt: imagePrompt.trim(),
          status: "pending",
          progress: 0,
          createdAt: Date.now(),
        };
        addHarvisTask(task, owner);
      } else {
        throw new Error("未收到创作任务或成品，请稍后重试");
      }
    } catch (error) {
      if (isCurrentMarketingSession(owner)) setNotice(String(error));
    } finally {
      if (isCurrentMarketingSession(owner)) setBusy(false);
    }
  };

  const uploadReferences = async (files: FileList | File[]) => {
    const candidates = Array.from(files).filter((file) => file.type.startsWith("image/"));
    if (!candidates.length) {
      setNotice("请添加 PNG、JPG、WEBP 等图片文件");
      return;
    }
    const slots = Math.max(0, 4 - referenceFiles.length);
    if (!slots) {
      setNotice("最多可添加 4 张参考图，请先移除已有图片");
      return;
    }
    const accepted = candidates.slice(0, slots);
    setBusy(true);
    setNotice("");
    try {
      const uploaded = await Promise.all(accepted.map(async (file) => {
        if (file.size > 20 * 1024 * 1024) throw new Error(`${file.name} 超过 20MB 上传限制`);
        const [result, previewUrl] = await Promise.all([creativeApi.uploadReference(file), readPreview(file)]);
        if (!result.referenceUrl) throw new Error("参考图服务未返回可访问地址，请联系管理员更新服务");
        return { id: result.id, referenceUrl: migratePublicServiceUrl(result.referenceUrl), fileName: file.name, previewUrl };
      }));
      setReferenceFiles((previous) => [...previous, ...uploaded]);
      setImageMode("image");
      setNotice(`已添加 ${uploaded.length} 张参考图${candidates.length > slots ? "；超过上限的文件未添加" : ""}`);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const removeReference = (id: string) => {
    setReferenceFiles((previous) => {
      const remaining = previous.filter((item) => item.id !== id);
      if (!remaining.length) setImageMode("text");
      return remaining;
    });
  };

  const showHarvisPreview = useCallback(async (task: HarvisTask) => {
    if (!task.resultUrl || task.ownerId !== session.userId || !isCurrentMarketingSession(session) || (task.kind !== "image" && task.kind !== "music")) return;
    const request = ++previewRequest.current;
    setPreviewLoading(task.taskId);
    try {
      const result = await creativeApi.harvisPreviewResult(task.resultUrl, task.kind);
      if (request !== previewRequest.current || !isCurrentMarketingSession(session)) return;
      setPreview({ task, fileName: `Harvis ${task.kind === "music" ? "音乐" : "图片"}成品`, kind: task.kind === "music" ? "audio" : "image", dataUrl: result.dataUrl });
    } catch (error) {
      if (request === previewRequest.current) setNotice(`预览失败：${String(error)}。仍可下载成品查看。`);
    } finally {
      if (request === previewRequest.current) setPreviewLoading(null);
    }
  }, [session.userId, session.generation]);

  useEffect(() => { setBusy(false); setNotice(""); }, [session.userId, session.generation]);

  useEffect(() => {
    setPreview(null);
    setPreviewLoading(null);
    return () => { previewRequest.current += 1; focusedTarget.current = ""; };
  }, [initialKind, focusedTaskId, focusResult, user?.id, session.generation]);

  useEffect(() => {
    if (!focusedTaskId || !user) { focusedTarget.current = ""; return; }
    const task = harvisTasks.find((item) => item.taskId === focusedTaskId && item.kind === initialKind);
    if (!task) return;
    const key = JSON.stringify([initialKind, focusedTaskId, focusResult]);
    if (focusedTarget.current === key) return;
    const node = Array.from(taskListRef.current?.querySelectorAll<HTMLElement>("[data-task-id]") ?? []).find((item) => item.dataset.taskId === focusedTaskId);
    node?.scrollIntoView?.({ block: "nearest" });
    node?.focus({ preventScroll: true });
    if (focusResult && !task.resultUrl) return;
    focusedTarget.current = key;
    if (focusResult) void showHarvisPreview(task);
  }, [focusedTaskId, focusResult, initialKind, harvisTasks, user, showHarvisPreview]);

  const downloadHarvisTask = async (task: HarvisTask) => {
    if (!task.resultUrl || downloadingTaskId || task.ownerId !== session.userId || !isCurrentMarketingSession(session)) return;
    setDownloadingTaskId(task.taskId);
    try {
      let extension = task.kind === "music" ? "m4a" : task.kind === "video" ? "mp4" : "png";
      try {
        const candidate = new URL(task.resultUrl).pathname.split(".").pop()?.toLowerCase();
        if (candidate && ["png", "jpg", "jpeg", "webp", "mp3", "m4a", "wav", "mp4"].includes(candidate)) extension = candidate;
      } catch { /* Data URLs use the default media extension. */ }
      const path = await saveFileDialog({
        title: "保存 Harvis 创作成品",
        defaultPath: `Harvis-${task.taskId.replace(/[^a-zA-Z0-9_-]/g, "")}.${extension}`,
        filters: [{ name: "创作成品", extensions: [extension] }]
      });
      if (!path || Array.isArray(path) || !isCurrentMarketingSession(session)) return;
      await creativeApi.harvisSaveResult(task.resultUrl, path);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setDownloadingTaskId(null);
    }
  };

  const saveMusicBrand = async () => {
    if (!adminBrandName.trim()) {
      setNotice("请填写品牌名称");
      return;
    }
    setBusy(true);
    try {
      await creativeApi.saveMusicBrand(adminBrandName.trim(), adminCollections.split(/[，,\n]/).map((value) => value.trim()).filter(Boolean));
      const config = await creativeApi.musicConfig();
      setMusicConfig(config);
      setAdminBrandName("");
      setAdminCollections("");
      setNotice("品牌与系列已保存，并已同步到创作助手");
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  if (!user) return <div className="creative-locked">登录线上营销工作台账号后可使用 Harvis AI 创作。</div>;

  const isMusic = initialKind === "music";
  return (
    <section className="creative-page">
      <header className="creative-header">
        <div><span className="eyebrow">Harvis Creative</span><h1>{isMusic ? "Harvis 音乐创作" : "Harvis 图片创作"}</h1></div>
        <div className="creative-meta"><span>{activeHarvisTasks.length ? `${activeHarvisTasks.length} 个任务进行中` : "创作通道已就绪"}</span><button className="btn btn-secondary" onClick={() => void refreshHarvisTasks()} disabled={busy}>刷新成果</button></div>
      </header>
      {notice && <div className="creative-notice">{notice}</div>}
      <div className="creative-layout">
        <div className="creative-main">
          {isMusic ? (
            <section className="creative-panel">
              <div className="creative-panel-head"><div className="creative-tabs"><button className={musicMode === "free" ? "active" : ""} onClick={() => setMusicMode("free")}>自由模式</button><button className={musicMode === "assistant" ? "active" : ""} onClick={() => setMusicMode("assistant")}>助手模式</button></div></div>
              <div className="creative-panel-body">
                {musicMode === "assistant" && <>
                  <p className="creative-assistant-tip">选择业务信息后，助手会检索已批准的音乐案例，并由默认模型生成可编辑的 Suno 纯音乐提示词。成品可在创作成果中预览、下载和评分。</p>
                  <div className="assistant-grid">
                    <label>业务线<select value={businessLine} onChange={(event) => updateBusinessLine(event.target.value)}>{musicConfig.businessLines.map((line) => <option key={line.id} value={line.title}>{line.title}</option>)}</select><small>{selectedBusinessLine.description}</small></label>
                    {hasAssistantField("brand") && <label>品牌<select value={brand} onChange={(event) => updateBrand(event.target.value)}>{musicConfig.brands.map((item) => <option key={item.id} value={item.name}>{item.name}</option>)}</select></label>}
                    {hasAssistantField("collection") && <label>腕表系列<select value={watchCollection} onChange={(event) => setWatchCollection(event.target.value)}>{collections.map((value) => <option key={value}>{value}</option>)}</select></label>}
                    {hasAssistantField("model") && <label>具体型号或表款 <small>必填</small><input value={assistantDetails.model ?? ""} placeholder="如：Happy Sport 30mm" onChange={(event) => setAssistantDetails((previous) => ({ ...previous, model: event.target.value }))} /></label>}
                    {hasAssistantField("theme") && <label>视频主题 <small>必填</small><input value={assistantDetails.theme ?? ""} placeholder="如：节日礼赠、女表穿搭、品牌故事" onChange={(event) => setAssistantDetails((previous) => ({ ...previous, theme: event.target.value }))} /></label>}
                    <label>时长<select value={musicDuration} onChange={(event) => setMusicDuration(Number(event.target.value))}>{[30, 45, 60, 90].map((value) => <option key={value} value={value}>{value} 秒</option>)}</select></label>
                    {hasAssistantField("desiredStyle") && <label>期望风格 <small>可选</small><input value={desiredStyle} placeholder="如：现代爵士、电影感" onChange={(event) => setDesiredStyle(event.target.value)} /></label>}
                    {hasAssistantField("bpm") && <label>BPM <small>可选</small><input type="number" min="40" max="220" value={bpm} placeholder="如：102" onChange={(event) => setBpm(event.target.value)} /></label>}
                    {hasAssistantField("watchStyle") && <label>腕表风格 <small>可选</small><select value={watchStyle} onChange={(event) => setWatchStyle(event.target.value)}><option value="">由模型判断</option>{(selectedBusinessLine.fields.find((field) => field.key === "watchStyle")?.options ?? ["多钻", "经典", "时尚", "大气", "复杂结构"]).map((value) => <option key={value}>{value}</option>)}</select></label>}
                    {additionalAssistantFields.map((field) => <label key={field.key}>{field.label}{field.required && <small>必填</small>}{field.type === "select" ? <select value={assistantDetails[field.key] ?? ""} onChange={(event) => setAssistantDetails((previous) => ({ ...previous, [field.key]: event.target.value }))}><option value="">请选择</option>{field.options.map((option) => <option key={option}>{option}</option>)}</select> : <input type={field.type === "number" ? "number" : "text"} value={assistantDetails[field.key] ?? ""} onChange={(event) => setAssistantDetails((previous) => ({ ...previous, [field.key]: event.target.value }))} />}</label>)}
                  </div>
                  <div className="creative-actions"><button className="btn btn-secondary" onClick={() => void generateAssistantPrompt()} disabled={busy}>{busy ? "生成中" : "生成提示词"}</button></div>
                  {assistantPromptResult && <section className="creative-assistant-result" aria-label="提示词依据">
                    <div className="creative-assistant-result-head"><strong>匹配依据</strong>{assistantPromptResult.confidence !== null && <span>置信度 {Math.round(assistantPromptResult.confidence * 100)}%</span>}</div>
                    <p>{assistantPromptResult.analysisZh}</p>
                    {assistantPromptResult.evidenceCases.length > 0 && <div className="creative-evidence-list">{assistantPromptResult.evidenceCases.map((item) => <span key={item.id}>{item.title}{item.genre ? ` · ${item.genre}` : ""}</span>)}</div>}
                    <details className="creative-prompt-translation"><summary>查看中文提示词说明</summary><p>{assistantPromptResult.promptZh}</p></details>
                    {assistantPromptResult.warnings.length > 0 && <div className="creative-prompt-warnings">{assistantPromptResult.warnings.map((warning) => <span key={warning}>{warning}</span>)}</div>}
                  </section>}
                  {user.userRole === "admin" && <section className="creative-admin-config"><button type="button" className="creative-admin-config-toggle" onClick={() => setShowMusicConfig((visible) => !visible)} aria-expanded={showMusicConfig}>管理品牌与系列</button>{showMusicConfig && <div className="creative-admin-config-form"><p>新增内容会立即进入助手下拉选项。业务线与字段结构可通过管理员配置接口维护。</p><label>品牌名称<input value={adminBrandName} placeholder="如：积家" onChange={(event) => setAdminBrandName(event.target.value)} /></label><label>腕表系列 <small>以逗号或换行分隔</small><textarea value={adminCollections} placeholder="如：Reverso, Master Ultra Thin" onChange={(event) => setAdminCollections(event.target.value)} /></label><div className="creative-actions"><button type="button" className="btn btn-secondary" onClick={() => void saveMusicBrand()} disabled={busy}>保存品牌与系列</button></div></div>}</section>}
                </>}
                <label>作品名称<input value={musicTitle} onChange={(event) => setMusicTitle(event.target.value)} /></label>
                <label>{musicMode === "assistant" ? "Suno 提示词" : "音乐提示词"}<textarea value={musicTags} onChange={(event) => setMusicTags(event.target.value)} placeholder="先生成或直接填写英文音乐提示词" /></label>
                {musicMode === "free" && <label>目标时长<input type="number" min="10" max="360" value={musicDuration} onChange={(event) => setMusicDuration(Number(event.target.value))} /></label>}
                <div className="creative-actions"><button className="btn btn-primary" onClick={() => void createMusic()} disabled={busy}>{busy ? "提交中" : "开始制作"}</button></div>
              </div>
            </section>
          ) : (
            <section className="creative-panel">
              <div className="creative-panel-head"><div className="creative-tabs"><button className={imageMode === "text" ? "active" : ""} onClick={() => setImageMode("text")}>文生图</button><button className={imageMode === "image" ? "active" : ""} onClick={() => setImageMode("image")}>图生图</button></div></div>
              <div className="creative-panel-body">
                <label>图像提示词<textarea value={imagePrompt} onChange={(event) => setImagePrompt(event.target.value)} /></label>
                <div className={`creative-reference-drop ${referenceDragActive ? "active" : ""}`} onDragEnter={(event) => { event.preventDefault(); setReferenceDragActive(true); }} onDragOver={(event) => event.preventDefault()} onDragLeave={(event) => { event.preventDefault(); setReferenceDragActive(false); }} onDrop={(event) => { event.preventDefault(); setReferenceDragActive(false); void uploadReferences(event.dataTransfer.files); }}>
                  <input ref={fileInputRef} type="file" accept="image/*" multiple onChange={(event) => { if (event.target.files) void uploadReferences(event.target.files); event.currentTarget.value = ""; }} />
                  <div><strong>添加参考图</strong><small>可一次选择或拖放多张图片，最多 4 张，每张不超过 20MB</small></div>
                  <button className="btn btn-secondary" type="button" onClick={() => fileInputRef.current?.click()} disabled={busy}>选择图片</button>
                </div>
                {referenceFiles.length > 0 && <div className="creative-reference-strip">{referenceFiles.map((item) => <figure key={item.id} className="creative-reference-card"><img src={item.previewUrl} alt={item.fileName} /><figcaption title={item.fileName}>{item.fileName}</figcaption><button type="button" onClick={() => removeReference(item.id)} aria-label={`移除 ${item.fileName}`}>x</button></figure>)}</div>}
                <div className="creative-template-grid">{imageTemplates.map(([title, prompt]) => <button key={title} className="creative-template" onClick={() => setImagePrompt(prompt)}><strong>{title}</strong><small>写入提示词</small></button>)}</div>
                <label>画面比例</label><div className="canvas-grid">{canvasOptions.map((option) => <button key={option.id} className={canvasPreset === option.id ? "canvas-option active" : "canvas-option"} onClick={() => setCanvasPreset(option.id)}><strong>{option.title}</strong><small>{option.detail}</small></button>)}</div>
                {canvasPreset === "custom" && <div className="canvas-custom"><label>宽度<input type="number" min="64" max="8000" value={canvasWidth} onChange={(event) => setCanvasWidth(Number(event.target.value))} /></label><span>x</span><label>高度<input type="number" min="64" max="8000" value={canvasHeight} onChange={(event) => setCanvasHeight(Number(event.target.value))} /></label><small>px</small></div>}
                <div className="creative-actions"><button className="btn btn-primary" onClick={() => void createImage()} disabled={busy}>{busy ? "提交中" : "生成图片"}</button></div>
              </div>
            </section>
          )}
        </div>
        <aside className="creative-side">
          <section className="creative-panel">
            <div className="creative-panel-head"><strong>创作成果</strong><span>{filteredHarvisTasks.length}</span></div>
            <div className="creative-job-list" ref={taskListRef}>
              {filteredHarvisTasks.map((task) => (
                <div key={task.taskId} className="creative-local-job" data-task-id={task.taskId} data-focused={task.taskId === focusedTaskId} tabIndex={-1}>
                  <div className="creative-local-job-top"><strong>{task.prompt.slice(0, 48) || "未命名创作"}</strong><small>{task.status === "succeeded" ? "已完成" : task.status === "failed" ? "失败" : task.status === "pending" ? "排队中" : "生成中…"}</small></div>
                  {task.errorMessage && <small className="creative-error">{task.errorMessage}</small>}
                  {task.queryError && <small className="creative-error">查询暂不可用：{task.queryError}</small>}
                  {(task.status === "pending" || task.status === "processing") && (
                    <div className="creative-job-progress">
                      <progress value={task.progress > 0 ? task.progress : undefined} max="100" aria-label="生成进度" />
                      <small>{task.progress > 0 ? `${Math.round(task.progress)}%` : "进度待更新"}</small>
                    </div>
                  )}
                  {task.resultUrl && (
                    <>
                    <div className="creative-local-job-actions">
                      <button type="button" className="btn btn-secondary" onClick={() => void showHarvisPreview(task)} disabled={previewLoading === task.taskId}>
                        {previewLoading === task.taskId ? "正在加载…" : "预览成品"}
                      </button>
                      <button type="button" className="btn btn-secondary" onClick={() => void downloadHarvisTask(task)} disabled={downloadingTaskId !== null}>
                        {downloadingTaskId === task.taskId ? "正在保存…" : "下载成品"}
                      </button>
                    </div>
                    {isMusic && <div className="creative-local-feedback" aria-label="作品采用与评分">
                      <button type="button" className="btn btn-secondary" onClick={() => updateLocalTask(task.taskId, { adopted: !task.adopted })}>{task.adopted ? "已采用" : "采用作品"}</button>
                      <div className="creative-rating" aria-label="评分">{[1, 2, 3, 4, 5].map((value) => <button key={value} type="button" disabled={!task.adopted} className={value <= (task.rating ?? 0) ? "active" : ""} onClick={() => updateLocalTask(task.taskId, { rating: value })} aria-label={`${value} 星`}>★</button>)}</div>
                      <small>采用与评分仅保存在本机</small>
                    </div>}
                    </>
                  )}
                </div>
              ))}
              {!filteredHarvisTasks.length && <div className="creative-empty">暂无创作成果</div>}
            </div>
          </section>
        </aside>
      </div>
      {preview && <div className="creative-preview-mask" onClick={() => setPreview(null)}><div className="creative-preview" role="dialog" aria-modal="true" aria-label={preview.fileName} onClick={(event) => event.stopPropagation()}><div className="creative-panel-head"><strong>{preview.fileName}</strong><div className="creative-preview-actions"><button className="btn btn-secondary" onClick={() => void downloadHarvisTask(preview.task)}>下载</button><button className="btn btn-secondary" onClick={() => setPreview(null)}>关闭</button></div></div>{preview.kind === "audio" ? <audio controls autoPlay src={preview.dataUrl} /> : <img src={preview.dataUrl} alt={preview.fileName} />}</div></div>}
    </section>
  );
}
