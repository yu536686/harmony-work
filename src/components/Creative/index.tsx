import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import {
  creativeApi,
  marketingApi,
  normalizeReferenceSelection,
  resolveCanvasRequest,
  type ChatAttachment,
  type CreativeArtifact,
  type CreativeJob,
  type MarketingUser
} from "../../services/tauri";
import "./Creative.css";

type CreativePageProps = {
  initialKind: "music" | "image";
  user: MarketingUser | null;
  onStageToChat: (attachments: ChatAttachment[], draft: string) => void;
};

type ReferenceAttachment = {
  id: string;
  fileName: string;
  previewUrl: string;
};

const businessLines = ["腕表如诗", "岁月如歌", "柜台似画", "百花齐放"] as const;

// Curated from the brands' current official collection navigations. Collections are
// deliberately concise so the selector remains useful for a campaign brief.
const watchCatalog: Record<string, string[]> = {
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

function jobStatus(status: CreativeJob["status"]) {
  return ({ queued: "排队中", processing: "制作中", succeeded: "已完成", failed: "制作失败", expired: "已过期" })[status];
}

function artifactCaption(artifact: CreativeArtifact) {
  const labels = { audio: "音频", image: "图片", cover: "封面" };
  return `${labels[artifact.kind]} · ${artifact.fileName}`;
}

function readPreview(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error(`无法读取 ${file.name}`));
    reader.readAsDataURL(file);
  });
}

function extensionOf(fileName: string) {
  const extension = fileName.split(".").pop()?.toLowerCase();
  return extension && extension !== fileName.toLowerCase() ? extension : "bin";
}

export default function CreativePage({ initialKind, user, onStageToChat }: CreativePageProps) {
  const [jobs, setJobs] = useState<CreativeJob[]>([]);
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
  const [jobDetailLoading, setJobDetailLoading] = useState(false);
  const [preview, setPreview] = useState<{ artifact: CreativeArtifact; dataUrl: string } | null>(null);

  const [musicMode, setMusicMode] = useState<"free" | "assistant">("free");
  const [musicTitle, setMusicTitle] = useState("萧邦 Happy Sport 品牌 BGM");
  const [musicTags, setMusicTags] = useState("Luxury fashion instrumental, 102 BPM, elegant high-end, refined piano motif, subtle string pads, clean wide mix, instrumental only, completely no vocals");
  const [musicDuration, setMusicDuration] = useState(45);
  const [businessLine, setBusinessLine] = useState<(typeof businessLines)[number]>("腕表如诗");
  const [brand, setBrand] = useState("萧邦");
  const [watchCollection, setWatchCollection] = useState("Happy Sport");
  const [watchStyle, setWatchStyle] = useState("");
  const [desiredStyle, setDesiredStyle] = useState("");
  const [bpm, setBpm] = useState("");

  const [imageMode, setImageMode] = useState<"text" | "image">("text");
  const [imagePrompt, setImagePrompt] = useState("一枚 Happy Sport 女士腕表置于柔和的香槟金丝绸上，光影精致，广告摄影，留白适合放置品牌文案。");
  const [canvasPreset, setCanvasPreset] = useState<"television" | "vertical" | "store-poster" | "phone-fullscreen" | "custom">("vertical");
  const [canvasWidth, setCanvasWidth] = useState(1024);
  const [canvasHeight, setCanvasHeight] = useState(1024);
  const [referenceFiles, setReferenceFiles] = useState<ReferenceAttachment[]>([]);
  const [referenceDragActive, setReferenceDragActive] = useState(false);
  const fileInputRef = useRef<HTMLInputElement | null>(null);

  const collections = watchCatalog[brand] ?? [];
  const referenceIds = useMemo(() => normalizeReferenceSelection(referenceFiles.map((item) => item.id)), [referenceFiles]);
  const selectedJob = jobs.find((job) => job.id === selectedJobId) ?? jobs[0] ?? null;
  const activeJobs = jobs.filter((job) => job.status === "queued" || job.status === "processing");

  const refreshJobDetail = useCallback(async (jobId: string) => {
    setJobDetailLoading(true);
    try {
      const response = await creativeApi.getJob(jobId);
      setJobs((previous) => previous.map((job) => job.id === response.job.id ? response.job : job));
    } catch (error) {
      setNotice(`任务详情加载失败：${String(error)}`);
    } finally {
      setJobDetailLoading(false);
    }
  }, []);

  const refreshJobs = async () => {
    const response = await creativeApi.listJobs();
    setJobs((previous) => response.jobs.map((job) => {
      const hydrated = previous.find((item) => item.id === job.id);
      return hydrated && hydrated.artifacts.length > job.artifacts.length
        ? { ...job, artifacts: hydrated.artifacts }
        : job;
    }));
    setSelectedJobId((current) => current ?? response.jobs[0]?.id ?? null);
  };

  useEffect(() => {
    if (!user) return;
    void refreshJobs().catch((error) => setNotice(String(error)));
  }, [user]);

  useEffect(() => {
    if (!user || !selectedJobId) return;
    void refreshJobDetail(selectedJobId);
  }, [refreshJobDetail, selectedJobId, user?.id]);

  useEffect(() => {
    if (!activeJobs.length) return;
    const timer = window.setInterval(() => {
      void Promise.all(activeJobs.map((job) => creativeApi.getJob(job.id)))
        .then((responses) => {
          setJobs((previous) => previous.map((job) => responses.find((response) => response.job.id === job.id)?.job ?? job));
        })
        .catch(() => undefined);
    }, 12_000);
    return () => window.clearInterval(timer);
  }, [activeJobs.map((job) => job.id).join(",")]);

  const updateBrand = (nextBrand: string) => {
    const nextCollection = watchCatalog[nextBrand]?.[0] ?? "";
    setBrand(nextBrand);
    setWatchCollection(nextCollection);
    setMusicTitle(`${nextBrand} ${nextCollection} 品牌 BGM`);
  };

  const generateAssistantPrompt = async () => {
    setBusy(true);
    setNotice("");
    try {
      const response = await creativeApi.composeMusicPrompt({
        businessLine,
        brand,
        watchCollection,
        watchStyle: watchStyle || undefined,
        desiredStyle: desiredStyle || undefined,
        bpm: bpm ? Number(bpm) : undefined,
        duration: musicDuration
      });
      setMusicTags(response.prompt);
      setNotice(`已使用默认模型生成提示词${response.model ? `：${response.model}` : ""}`);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const createMusic = async () => {
    if (!musicTags.trim()) {
      setNotice("请先填写提示词，或在助手模式中点击“生成提示词”");
      return;
    }
    setBusy(true);
    setNotice("");
    try {
      const response = await creativeApi.createMusic({ mode: musicMode, tags: musicTags.trim(), title: musicTitle.trim() || `${brand} ${watchCollection} 品牌 BGM`, duration: musicDuration, prompt: musicTags.trim() });
      setJobs((previous) => [response.job, ...previous]);
      setSelectedJobId(response.job.id);
      setNotice("音乐任务已提交");
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
  };

  const createImage = async () => {
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
      const response = await creativeApi.createImage({
        mode: imageMode,
        prompt: imagePrompt.trim(),
        canvasPreset: canvas.canvasPreset,
        outputWidth: canvas.outputWidth,
        outputHeight: canvas.outputHeight,
        referenceIds
      });
      setJobs((previous) => [response.job, ...previous]);
      setSelectedJobId(response.job.id);
      setNotice("图片任务已提交");
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
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
        return { id: result.id, fileName: file.name, previewUrl };
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

  const showPreview = async (artifact: CreativeArtifact) => {
    if (!artifact.assetId) return;
    try {
      const data = await marketingApi.assetDataUrl(artifact.assetId, artifact.fileName, artifact.kind === "audio" ? 24 * 1024 * 1024 : 16 * 1024 * 1024);
      setPreview({ artifact, dataUrl: data.dataUrl });
    } catch (error) {
      setNotice(String(error));
    }
  };

  const downloadArtifact = async (artifact: CreativeArtifact) => {
    if (!artifact.assetId) {
      setNotice("该成品尚未归档，暂时无法下载");
      return;
    }
    try {
      const path = await saveFileDialog({
        title: "保存 Harvis 创作成品",
        defaultPath: artifact.fileName,
        filters: [{ name: artifact.kind === "audio" ? "音频文件" : "图片文件", extensions: [extensionOf(artifact.fileName)] }]
      });
      if (!path || Array.isArray(path)) return;
      const saved = await creativeApi.saveAsset(artifact.assetId, artifact.fileName, path);
      setNotice(`已保存到 ${saved.localPath}`);
    } catch (error) {
      setNotice(String(error));
    }
  };

  const stageForChat = async (job: CreativeJob) => {
    setBusy(true);
    try {
      const staged = await creativeApi.stageForChat(job.id);
      const attachments = await Promise.all(staged.assetIds.map(async (assetId) => {
        const artifact = job.artifacts.find((item) => item.assetId === assetId);
        const prepared = await marketingApi.prepareAssetForChat(assetId, artifact?.fileName ?? "Harvis 创作成品");
        return { path: prepared.localPath, filename: prepared.fileName, media_type: prepared.mediaType };
      }));
      onStageToChat(attachments, `请基于这些 Harvis 创作成品继续处理：${job.promptSnapshot}`);
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
        <div className="creative-meta"><span>{activeJobs.length ? `${activeJobs.length} 个任务进行中` : "创作通道已就绪"}</span><button className="btn btn-secondary" onClick={() => { void refreshJobs(); if (selectedJobId) void refreshJobDetail(selectedJobId); }} disabled={busy || jobDetailLoading}>刷新任务</button></div>
      </header>
      {notice && <div className="creative-notice">{notice}</div>}
      <div className="creative-layout">
        <div className="creative-main">
          {isMusic ? (
            <section className="creative-panel">
              <div className="creative-panel-head"><div className="creative-tabs"><button className={musicMode === "free" ? "active" : ""} onClick={() => setMusicMode("free")}>自由模式</button><button className={musicMode === "assistant" ? "active" : ""} onClick={() => setMusicMode("assistant")}>助手模式</button></div></div>
              <div className="creative-panel-body">
                {musicMode === "assistant" && <>
                  <p className="creative-assistant-tip">选择业务和腕表信息后，默认模型会从内置 Suno 曲风关键词库中选择合适的纯音乐方向，并写入下方可编辑的提示词。</p>
                  <div className="assistant-grid">
                    <label>业务线<select value={businessLine} onChange={(event) => setBusinessLine(event.target.value as (typeof businessLines)[number])}>{businessLines.map((value) => <option key={value}>{value}</option>)}</select></label>
                    <label>品牌<select value={brand} onChange={(event) => updateBrand(event.target.value)}>{Object.keys(watchCatalog).map((value) => <option key={value}>{value}</option>)}</select></label>
                    <label>腕表系列<select value={watchCollection} onChange={(event) => setWatchCollection(event.target.value)}>{collections.map((value) => <option key={value}>{value}</option>)}</select></label>
                    <label>时长<select value={musicDuration} onChange={(event) => setMusicDuration(Number(event.target.value))}>{[30, 45, 60, 90].map((value) => <option key={value} value={value}>{value} 秒</option>)}</select></label>
                    <label>期望风格 <small>可选</small><input value={desiredStyle} placeholder="如：现代爵士、电影感" onChange={(event) => setDesiredStyle(event.target.value)} /></label>
                    <label>BPM <small>可选</small><input type="number" min="40" max="220" value={bpm} placeholder="如：102" onChange={(event) => setBpm(event.target.value)} /></label>
                    <label>腕表风格 <small>可选</small><select value={watchStyle} onChange={(event) => setWatchStyle(event.target.value)}><option value="">由模型判断</option>{["多钻", "经典", "时尚", "大气", "复杂结构"].map((value) => <option key={value}>{value}</option>)}</select></label>
                  </div>
                  <div className="creative-actions"><button className="btn btn-secondary" onClick={() => void generateAssistantPrompt()} disabled={busy}>{busy ? "生成中" : "生成提示词"}</button></div>
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
          <section className="creative-panel"><div className="creative-panel-head"><strong>作品任务</strong><span>{jobs.length}</span></div><div className="creative-job-list">{jobs.map((job) => <button key={job.id} className={selectedJob?.id === job.id ? "creative-job active" : "creative-job"} onClick={() => setSelectedJobId(job.id)}><span>{job.kind === "music" ? "音乐" : "图片"}</span><strong>{job.promptSnapshot || "未命名创作"}</strong><small>{jobStatus(job.status)} {job.progress > 0 ? `${job.progress}%` : ""}</small></button>)}{!jobs.length && <div className="creative-empty">暂无创作任务</div>}</div></section>
          <section className="creative-panel creative-detail"><div className="creative-panel-head"><strong>任务详情</strong>{selectedJob && <span>{jobStatus(selectedJob.status)}</span>}</div>{selectedJob ? <div className="creative-panel-body"><p className="creative-detail-prompt">{selectedJob.promptSnapshot}</p>{selectedJob.errorMessage && <div className="creative-error">{selectedJob.errorMessage}</div>}{jobDetailLoading ? <div className="creative-empty creative-detail-state">正在同步成品信息…</div> : selectedJob.status === "succeeded" && selectedJob.artifacts.length === 0 ? <div className="creative-empty creative-detail-state">任务已完成，成品归档信息暂未返回。请点击“刷新任务”重试。</div> : null}<div className="creative-artifacts">{selectedJob.artifacts.map((artifact) => <div key={artifact.id} className="creative-artifact-row"><button className="creative-artifact-open" onClick={() => void showPreview(artifact)}>{artifactCaption(artifact)}</button><button className="creative-artifact-download" onClick={() => void downloadArtifact(artifact)} aria-label={`下载 ${artifact.fileName}`}>下载</button></div>)}</div>{selectedJob.artifacts.length > 0 && <div className="creative-actions"><button className="btn btn-secondary" onClick={() => void stageForChat(selectedJob)} disabled={busy}>放入会话输入框</button></div>}</div> : <div className="creative-empty">选择一个任务查看产物</div>}</section>
        </aside>
      </div>
      {preview && <div className="creative-preview-mask" onClick={() => setPreview(null)}><div className="creative-preview" onClick={(event) => event.stopPropagation()}><div className="creative-panel-head"><strong>{preview.artifact.fileName}</strong><div className="creative-preview-actions"><button className="btn btn-secondary" onClick={() => void downloadArtifact(preview.artifact)}>下载</button><button className="btn btn-secondary" onClick={() => setPreview(null)}>关闭</button></div></div>{preview.artifact.kind === "audio" ? <audio controls autoPlay src={preview.dataUrl} /> : <img src={preview.dataUrl} alt={preview.artifact.fileName} />}</div></div>}
    </section>
  );
}
