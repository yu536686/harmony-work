import { useEffect, useMemo, useState } from "react";
import {
  creativeApi,
  marketingApi,
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

const seriesPrompt: Record<string, string> = {
  "腕表如诗": "elegant neo-classical luxury instrumental, refined piano motif, delicate strings, sparkling but restrained, cinematic product reveal",
  "岁月如歌": "warm heritage instrumental, lyrical piano, slow unfolding strings, graceful time and lasting memory",
  "柜台似画": "modern luxury lounge instrumental, Rhodes electric piano, subtle bass, refined boutique ambience",
  "百花齐放": "bright celebratory chamber pop instrumental, light marimba, airy strings, polished festive atmosphere"
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

function buildAssistantTags(series: string, watchStyle: string, brand: string, seconds: number) {
  return `${seriesPrompt[series]}, ${brand} ${watchStyle} watch, ${seconds}-second brand BGM, instrumental only, completely no vocals, no voice samples, no speech, no humming, no chanting, clean balanced mixing`;
}

function artifactCaption(artifact: CreativeArtifact) {
  const labels = { audio: "音频", image: "图片", cover: "封面" };
  return `${labels[artifact.kind]} · ${artifact.fileName}`;
}

export default function CreativePage({ initialKind, user, onStageToChat }: CreativePageProps) {
  const [jobs, setJobs] = useState<CreativeJob[]>([]);
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ artifact: CreativeArtifact; dataUrl: string } | null>(null);

  const [musicMode, setMusicMode] = useState<"free" | "assistant">("free");
  const [musicTitle, setMusicTitle] = useState("Happy Sport 品牌 BGM");
  const [musicTags, setMusicTags] = useState("Luxury fashion instrumental, 102 BPM, elegant high-end, refined piano motif, subtle string pads, clean wide mix, instrumental only, completely no vocals");
  const [musicDuration, setMusicDuration] = useState(45);
  const [series, setSeries] = useState("腕表如诗");
  const [brand, setBrand] = useState("Happy Sport");
  const [watchStyle, setWatchStyle] = useState("多钻");

  const [imageMode, setImageMode] = useState<"text" | "image">("text");
  const [imagePrompt, setImagePrompt] = useState("一枚 Happy Sport 女士腕表置于柔和的香槟金丝绸上，光影精致，广告摄影，留白适合放置品牌文案。");
  const [canvasPreset, setCanvasPreset] = useState<"television" | "vertical" | "store-poster" | "phone-fullscreen" | "custom">("vertical");
  const [canvasWidth, setCanvasWidth] = useState(1024);
  const [canvasHeight, setCanvasHeight] = useState(1024);
  const [referenceIds, setReferenceIds] = useState<string[]>([]);

  const assistantTags = useMemo(
    () => buildAssistantTags(series, watchStyle, brand, musicDuration),
    [series, watchStyle, brand, musicDuration]
  );
  const selectedJob = jobs.find((job) => job.id === selectedJobId) ?? jobs[0] ?? null;
  const activeJobs = jobs.filter((job) => job.status === "queued" || job.status === "processing");

  const refreshJobs = async () => {
    const response = await creativeApi.listJobs();
    setJobs(response.jobs);
    setSelectedJobId((current) => current ?? response.jobs[0]?.id ?? null);
  };

  useEffect(() => {
    if (!user) return;
    void refreshJobs().catch((error) => setNotice(String(error)));
  }, [user]);

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

  const createMusic = async () => {
    setBusy(true);
    setNotice("");
    try {
      const tags = musicMode === "assistant" ? assistantTags : musicTags;
      const response = await creativeApi.createMusic({ mode: musicMode, tags, title: musicTitle, duration: musicDuration, prompt: "" });
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
    setBusy(true);
    setNotice("");
    try {
      const canvas = resolveCanvasRequest(canvasPreset, canvasWidth, canvasHeight);
      const response = await creativeApi.createImage({
        mode: imageMode,
        prompt: imagePrompt,
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

  const uploadReference = async (file: File | undefined) => {
    if (!file) return;
    setBusy(true);
    setNotice("");
    try {
      const uploaded = await creativeApi.uploadReference(file);
      setReferenceIds((previous) => [...previous, uploaded.id].slice(-4));
      setImageMode("image");
      setNotice(`已加入参考图：${file.name}`);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(false);
    }
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

  const stageForChat = async (job: CreativeJob) => {
    setBusy(true);
    try {
      const staged = await creativeApi.stageForChat(job.id);
      const attachments = await Promise.all(staged.assetIds.map(async (assetId) => {
        const artifact = job.artifacts.find((item) => item.assetId === assetId);
        const prepared = await marketingApi.prepareAssetForChat(
          assetId,
          artifact?.fileName ?? "Harvis 创作成品",
        );
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
        <div className="creative-meta"><span>{activeJobs.length ? `${activeJobs.length} 个任务进行中` : "创作通道已就绪"}</span><button className="btn btn-secondary" onClick={() => void refreshJobs()} disabled={busy}>刷新任务</button></div>
      </header>
      {notice && <div className="creative-notice">{notice}</div>}
      <div className="creative-layout">
        <div className="creative-main">
          {isMusic ? (
            <section className="creative-panel">
              <div className="creative-panel-head"><div className="creative-tabs"><button className={musicMode === "free" ? "active" : ""} onClick={() => setMusicMode("free")}>自由模式</button><button className={musicMode === "assistant" ? "active" : ""} onClick={() => setMusicMode("assistant")}>助手模式</button></div></div>
              <div className="creative-panel-body">
                {musicMode === "assistant" && <div className="assistant-grid"><label>业务系列<select value={series} onChange={(event) => setSeries(event.target.value)}>{Object.keys(seriesPrompt).map((value) => <option key={value}>{value}</option>)}</select></label><label>品牌<select value={brand} onChange={(event) => setBrand(event.target.value)}><option>Happy Sport</option><option>Alpine Eagle</option><option>Classic Racing</option></select></label><label>腕表风格<select value={watchStyle} onChange={(event) => setWatchStyle(event.target.value)}>{["多钻", "经典", "时尚", "大气", "复杂结构"].map((value) => <option key={value}>{value}</option>)}</select></label><label>时长<select value={musicDuration} onChange={(event) => setMusicDuration(Number(event.target.value))}>{[30, 45, 60, 90].map((value) => <option key={value} value={value}>{value} 秒</option>)}</select></label></div>}
                <label>作品名称<input value={musicTitle} onChange={(event) => setMusicTitle(event.target.value)} /></label>
                <label>{musicMode === "assistant" ? "品牌提示词" : "音乐提示词"}<textarea value={musicMode === "assistant" ? assistantTags : musicTags} onChange={(event) => musicMode === "free" && setMusicTags(event.target.value)} readOnly={musicMode === "assistant"} /></label>
                {musicMode === "assistant" && <div className="creative-actions"><button className="btn btn-secondary" onClick={() => { setMusicTags(assistantTags); setMusicMode("free"); }}>写入自由模式</button></div>}
                {musicMode === "free" && <label>目标时长<input type="number" min="10" max="360" value={musicDuration} onChange={(event) => setMusicDuration(Number(event.target.value))} /></label>}
                <div className="creative-actions"><button className="btn btn-primary" onClick={() => void createMusic()} disabled={busy}>{busy ? "提交中" : "开始制作"}</button></div>
              </div>
            </section>
          ) : (
            <section className="creative-panel">
              <div className="creative-panel-head"><div className="creative-tabs"><button className={imageMode === "text" ? "active" : ""} onClick={() => setImageMode("text")}>文生图</button><button className={imageMode === "image" ? "active" : ""} onClick={() => setImageMode("image")}>图生图</button></div></div>
              <div className="creative-panel-body">
                <label>图像提示词<textarea value={imagePrompt} onChange={(event) => setImagePrompt(event.target.value)} /></label>
                <div className="creative-template-grid">{imageTemplates.map(([title, prompt]) => <button key={title} className="creative-template" onClick={() => setImagePrompt(prompt)}><strong>{title}</strong><small>写入提示词</small></button>)}</div>
                <label>参考图<input type="file" accept="image/*" onChange={(event) => void uploadReference(event.target.files?.[0])} /></label>
                {referenceIds.length > 0 && <div className="creative-reference-count">已选 {referenceIds.length} 张参考图</div>}
                <label>画面比例</label><div className="canvas-grid">{canvasOptions.map((option) => <button key={option.id} className={canvasPreset === option.id ? "canvas-option active" : "canvas-option"} onClick={() => setCanvasPreset(option.id)}><strong>{option.title}</strong><small>{option.detail}</small></button>)}</div>
                {canvasPreset === "custom" && <div className="canvas-custom"><label>宽度<input type="number" min="64" max="8000" value={canvasWidth} onChange={(event) => setCanvasWidth(Number(event.target.value))} /></label><span>x</span><label>高度<input type="number" min="64" max="8000" value={canvasHeight} onChange={(event) => setCanvasHeight(Number(event.target.value))} /></label><small>px</small></div>}
                <div className="creative-actions"><button className="btn btn-primary" onClick={() => void createImage()} disabled={busy}>{busy ? "提交中" : "生成图片"}</button></div>
              </div>
            </section>
          )}
        </div>
        <aside className="creative-side">
          <section className="creative-panel"><div className="creative-panel-head"><strong>作品任务</strong><span>{jobs.length}</span></div><div className="creative-job-list">{jobs.map((job) => <button key={job.id} className={selectedJob?.id === job.id ? "creative-job active" : "creative-job"} onClick={() => setSelectedJobId(job.id)}><span>{job.kind === "music" ? "音乐" : "图片"}</span><strong>{job.promptSnapshot || "未命名创作"}</strong><small>{jobStatus(job.status)} {job.progress > 0 ? `${job.progress}%` : ""}</small></button>)}{!jobs.length && <div className="creative-empty">暂无创作任务</div>}</div></section>
          <section className="creative-panel creative-detail"><div className="creative-panel-head"><strong>任务详情</strong>{selectedJob && <span>{jobStatus(selectedJob.status)}</span>}</div>{selectedJob ? <div className="creative-panel-body"><p className="creative-detail-prompt">{selectedJob.promptSnapshot}</p>{selectedJob.errorMessage && <div className="creative-error">{selectedJob.errorMessage}</div>}<div className="creative-artifacts">{selectedJob.artifacts.map((artifact) => <button key={artifact.id} onClick={() => void showPreview(artifact)}>{artifactCaption(artifact)}</button>)}</div>{selectedJob.artifacts.length > 0 && <div className="creative-actions"><button className="btn btn-secondary" onClick={() => void stageForChat(selectedJob)} disabled={busy}>放入会话输入框</button></div>}</div> : <div className="creative-empty">选择一个任务查看产物</div>}</section>
        </aside>
      </div>
      {preview && <div className="creative-preview-mask" onClick={() => setPreview(null)}><div className="creative-preview" onClick={(event) => event.stopPropagation()}><div className="creative-panel-head"><strong>{preview.artifact.fileName}</strong><button className="btn btn-secondary" onClick={() => setPreview(null)}>关闭</button></div>{preview.artifact.kind === "audio" ? <audio controls autoPlay src={preview.dataUrl} /> : <img src={preview.dataUrl} alt={preview.artifact.fileName} />}</div></div>}
    </section>
  );
}
