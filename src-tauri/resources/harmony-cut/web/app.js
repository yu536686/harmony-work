let state = {
  project: null,
  selectedAssetId: null,
  selectedClipIndex: 0,
  browserPath: "",
};

const $ = (id) => document.getElementById(id);

const ROLE_LABELS = {
  opening: "开箱",
  opening_detail: "开箱特写",
  hero: "英雄旋转",
  hero_detail: "英雄特写",
  handheld: "手拿展示",
  handheld_detail: "手拿特写",
  alternate_handheld: "另一种手拿",
  alternate_detail: "另一种特写",
  wrist_full: "上手全景",
  wrist_detail: "上手特写",
  wrist_full_2: "上手全景 2",
  wrist_detail_2: "上手特写 2",
  broll: "补充镜头",
  reject: "废片",
};

function setStatus(text) {
  $("status").textContent = text;
}

function markerText(project) {
  const files = project?.marker_files || {};
  if (!files.json && !files.xml) return "";
  return ` 标记文件：${files.json || ""}${files.xml ? `｜${files.xml}` : ""}`;
}

function showError(error) {
  const message = error?.message || String(error);
  setStatus(`操作失败：${message}`);
  window.alert(`操作失败：\n${message}`);
}

async function withBusyButton(buttonId, busyText, action) {
  const button = $(buttonId);
  const originalText = button?.textContent;
  if (button) {
    button.disabled = true;
    button.textContent = busyText;
  }
  try {
    return await action();
  } catch (error) {
    showError(error);
    return null;
  } finally {
    if (button) {
      button.disabled = false;
      button.textContent = originalText;
    }
  }
}

async function api(path, options = {}) {
  const res = await fetch(path, {
    headers: { "Content-Type": "application/json" },
    ...options,
  });
  let data = null;
  try {
    data = await res.json();
  } catch (error) {
    throw new Error(`接口返回不是 JSON：HTTP ${res.status}`);
  }
  if (!res.ok || !data.ok) throw new Error(data.error || `接口错误：HTTP ${res.status}`);
  return data;
}

function fileUrl(path) {
  return `/api/file?path=${encodeURIComponent(path)}`;
}

function basename(path) {
  return String(path || "").split(/[\\/]/).pop();
}

function formatTime(value) {
  return Number(value || 0).toFixed(3);
}

function currentAsset() {
  return state.project?.assets?.find((a) => a.id === state.selectedAssetId) || null;
}

function currentClip() {
  return state.project?.clips?.[state.selectedClipIndex] || null;
}

function bestAssetWindow(asset) {
  const windows = Array.isArray(asset?.best_windows) ? asset.best_windows : [];
  return windows[0] || null;
}

function roleLabel(role) {
  return ROLE_LABELS[role] || role || "未分类";
}

function renderAssets() {
  const list = $("assetList");
  const assets = state.project?.assets || [];
  $("assetCount").textContent = String(assets.length);
  list.innerHTML = "";
  assets.forEach((asset) => {
    const el = document.createElement("div");
    el.className = `asset-card ${asset.id === state.selectedAssetId ? "active" : ""}`;
    el.onclick = () => selectAsset(asset.id);
    const thumb = asset.thumbnail ? `<img src="${fileUrl(asset.thumbnail)}" alt="" />` : `<div class="empty-thumb"></div>`;
    const best = bestAssetWindow(asset);
    const tags = Array.isArray(asset.tags) && asset.tags.length ? asset.tags.join(", ") : "未标记";
    el.innerHTML = `
      ${thumb}
      <div>
        <div class="asset-name">${asset.name}</div>
        <div class="asset-meta">
          ${asset.width || "-"}x${asset.height || "-"} / ${Number(asset.duration || 0).toFixed(2)} 秒<br/>
          ${asset.rotation ? `旋转 ${asset.rotation} 度 / ` : ""}${tags}<br/>
          ${best ? `AI ${best.source_in}-${best.source_out} / 评分 ${best.score}` : ""}
        </div>
      </div>`;
    list.appendChild(el);
  });
}

function renderClips() {
  const list = $("clipList");
  const clips = state.project?.clips || [];
  $("clipCount").textContent = String(clips.length);
  list.innerHTML = "";
  clips.forEach((clip, idx) => {
    const el = document.createElement("div");
    el.className = `clip-card ${idx === state.selectedClipIndex ? "active" : ""} ${clip.role === "reject" ? "rejected" : ""}`;
    el.onclick = () => selectClip(idx);
    el.innerHTML = `
      <div class="shot-num">#${clip.shot || idx + 1}</div>
      <div>
        <div class="asset-name">${roleLabel(clip.role)}</div>
        <div class="clip-meta">
          ${formatTime(clip.source_in)}-${formatTime(clip.source_out)}s<br/>
          ${basename(clip.path)}
        </div>
      </div>`;
    list.appendChild(el);
  });
}

function renderBestWindows(asset) {
  const box = $("bestWindows");
  box.innerHTML = "";
  const windows = Array.isArray(asset?.best_windows) ? asset.best_windows : [];
  windows.forEach((win, idx) => {
    const btn = document.createElement("button");
    btn.textContent = `#${idx + 1} ${win.source_in}-${win.source_out} / ${win.score}`;
    btn.onclick = () => {
      $("inInput").value = win.source_in;
      $("outInput").value = win.source_out;
      syncFieldsToClip();
      seekToInPoint();
    };
    box.appendChild(btn);
  });
  if (!box.children.length) {
    box.textContent = "暂无 AI 推荐片段。";
  }
}

function renderCaptions() {
  const list = $("captionList");
  const captions = state.project?.captions || [];
  list.innerHTML = "";
  captions.forEach((caption) => {
    const item = document.createElement("div");
    item.className = "caption-item";
    item.textContent = `${formatTime(caption.start)}-${formatTime(caption.end)} ${caption.text}`;
    list.appendChild(item);
  });
}

function setVideoSource(path, time = 0) {
  if (!path) return;
  $("video").src = fileUrl(path);
  $("video").onloadedmetadata = () => {
    $("video").currentTime = Math.max(0, Number(time || 0));
  };
}

function loadClipToEditor() {
  const clip = currentClip();
  if (!clip) return;
  const asset = state.project.assets.find((a) => a.id === clip.asset_id) || null;
  state.selectedAssetId = clip.asset_id || asset?.id || state.selectedAssetId;
  $("inInput").value = clip.source_in ?? 0;
  $("outInput").value = clip.source_out ?? 0;
  $("speedInput").value = clip.speed ?? 1;
  $("roleInput").value = clip.role || "broll";
  $("rotationInput").value = asset?.rotation ?? 0;
  const kfs = Array.isArray(clip.scale_keyframes) ? clip.scale_keyframes : [];
  $("scaleStartInput").value = kfs[0]?.scale ?? 1;
  $("scaleEndInput").value = kfs[kfs.length - 1]?.scale ?? 1;
  const videoPath = clip.path || asset?.proxy_path || asset?.path;
  setVideoSource(videoPath, clip.source_in || 0);
  renderBestWindows(asset);
  renderAssets();
  renderClips();
  renderCaptions();
}

function selectAsset(assetId) {
  syncFieldsToClip();
  state.selectedAssetId = assetId;
  const asset = currentAsset();
  if (!asset) return;
  const best = bestAssetWindow(asset);
  setVideoSource(asset.proxy_path || asset.path, best?.source_in || 0);
  $("inInput").value = best?.source_in ?? 0;
  $("outInput").value = best?.source_out ?? Math.min(2.4, asset.duration || 2.4);
  $("speedInput").value = 1;
  $("rotationInput").value = asset.rotation || 0;
  renderBestWindows(asset);
  renderAssets();
  setStatus(`已选择素材：${asset.name}`);
}

function selectClip(idx) {
  syncFieldsToClip();
  state.selectedClipIndex = idx;
  loadClipToEditor();
}

function syncFieldsToClip() {
  const clip = currentClip();
  if (!clip) return;
  const asset = currentAsset();
  const sourceIn = Number($("inInput").value || 0);
  const sourceOut = Number($("outInput").value || 0);
  clip.source_in = Math.max(0, sourceIn);
  clip.source_out = Math.max(clip.source_in + 0.05, sourceOut);
  clip.speed = Number($("speedInput").value || 1);
  clip.role = $("roleInput").value;
  const duration = Math.max(0.1, Number(clip.timeline_end || 0) - Number(clip.timeline_start || 0));
  clip.scale_keyframes = [
    { time: 0, scale: Number($("scaleStartInput").value || 1), x: 0, y: 0 },
    { time: duration, scale: Number($("scaleEndInput").value || 1), x: 0, y: 0 },
  ];
  if (asset) {
    asset.rotation = Number($("rotationInput").value || 0);
  }
  renderClips();
}

function assignAssetToSlot() {
  const asset = currentAsset();
  const clip = currentClip();
  if (!asset || !clip) return;
  const sourceIn = Number($("inInput").value || bestAssetWindow(asset)?.source_in || 0);
  const sourceOut = Number($("outInput").value || bestAssetWindow(asset)?.source_out || Math.min(2.4, asset.duration || 2.4));
  clip.asset_id = asset.id;
  clip.path = asset.proxy_path || asset.path;
  clip.original_path = asset.path;
  clip.source_in = sourceIn;
  clip.source_out = Math.max(sourceIn + 0.05, sourceOut);
  clip.speed = Number($("speedInput").value || 1);
  clip.role = $("roleInput").value === "reject" ? clip.role : $("roleInput").value;
  loadClipToEditor();
  setStatus(`已将 ${asset.name} 放入第 ${clip.shot || state.selectedClipIndex + 1} 镜头`);
}

async function newProject() {
  setStatus("正在创建项目...");
  const data = await api("/api/projects", {
    method: "POST",
    body: JSON.stringify({
      title: $("titleInput").value,
      media_dir: $("mediaDirInput").value,
      requirement: $("requirementInput").value,
    }),
  });
  state.project = data.project;
  state.selectedAssetId = null;
  state.selectedClipIndex = 0;
  setStatus(`项目已创建：${state.project.id}`);
  renderAssets();
  renderClips();
}

async function loadProject(projectId) {
  const data = await api(`/api/projects/${projectId}`);
  state.project = data.project;
  $("projectIdInput").value = state.project.id || "";
  $("titleInput").value = state.project.title || "";
  $("mediaDirInput").value = state.project.media_dir || $("mediaDirInput").value;
  $("requirementInput").value = state.project.requirement || "";
  state.selectedClipIndex = 0;
  state.selectedAssetId = state.project.clips?.[0]?.asset_id || state.project.assets?.[0]?.id || null;
  if (state.project.clips?.length) {
    loadClipToEditor();
  } else {
    renderAssets();
    renderClips();
    renderCaptions();
  }
  setStatus(`已载入项目：${state.project.id}${markerText(state.project)}`);
}

async function loadLatestProject() {
  const params = new URLSearchParams(window.location.search);
  const projectId = params.get("project");
  if (projectId) {
    await loadProject(projectId);
    return;
  }
  const data = await api("/api/projects");
  const latest = data.projects?.[0];
  if (!latest) {
    setStatus("还没有项目。");
    return;
  }
  await loadProject(latest.id);
}

async function scanProject() {
  if (!state.project) await newProject();
  setStatus("正在扫描素材并生成初剪，素材较多时会稍慢...");
  const data = await api(`/api/projects/${state.project.id}/scan`, {
    method: "POST",
    body: JSON.stringify({
      media_dir: $("mediaDirInput").value,
      requirement: $("requirementInput").value,
      auto_rotate_landscape: $("autoRotateInput").checked,
    }),
  });
  state.project = data.project;
  state.selectedClipIndex = 0;
  state.selectedAssetId = state.project.clips?.[0]?.asset_id || state.project.assets?.[0]?.id || null;
  loadClipToEditor();
  setStatus("扫描完成，初剪 EDL 已生成。");
}

async function saveProject() {
  if (!state.project) {
    setStatus("请先新建或载入项目。");
    return null;
  }
  syncFieldsToClip();
  setStatus("正在保存 EDL...");
  const data = await api(`/api/projects/${state.project.id}/save`, {
    method: "POST",
    body: JSON.stringify({
      title: $("titleInput").value,
      requirement: $("requirementInput").value,
      settings: state.project.settings,
      assets: state.project.assets,
      clips: state.project.clips,
      captions: state.project.captions,
      audio: state.project.audio,
      workflow_status: "user_review_saved",
    }),
  });
  state.project = data.project;
  const savedAt = state.project.updated_at || new Date().toLocaleString();
  setStatus(`已保存，回到 AI 对话发送“继续”。保存时间：${savedAt}${markerText(state.project)}`);
  window.alert(`已保存标记文件。\n\n请回到 AI 对话发送“继续”，我会继续生成剪映草稿并渲染成片。\n\n${markerText(state.project)}`);
  return state.project;
}

async function createDraft() {
  if (!state.project) {
    setStatus("请先新建或载入项目。");
    return null;
  }
  const saved = await saveProject();
  if (!saved) return null;
  setStatus("正在生成剪映草稿...");
  const data = await api(`/api/projects/${state.project.id}/draft`, { method: "POST", body: "{}" });
  state.project = data.project;
  const draftPath = data.result.draft_path ? `｜${data.result.draft_path}` : "";
  setStatus(`剪映草稿已生成：${data.result.draft_name}${draftPath}`);
  window.alert(`剪映草稿已生成：\n${data.result.draft_name}${draftPath ? `\n${data.result.draft_path}` : ""}`);
  return data.result;
}

async function renderPreview() {
  if (!state.project) {
    setStatus("请先新建或载入项目。");
    return null;
  }
  syncFieldsToClip();
  setStatus("正在渲染预览 MP4...");
  const data = await api(`/api/projects/${state.project.id}/preview`, {
    method: "POST",
    body: JSON.stringify({
      settings: state.project.settings,
      assets: state.project.assets,
      clips: state.project.clips,
      captions: state.project.captions,
      audio: state.project.audio,
    }),
  });
  state.project = data.project;
  const path = data.result.path;
  $("previewPanel").classList.remove("hidden");
  $("previewVideo").src = fileUrl(path);
  $("previewLink").href = fileUrl(path);
  setStatus(`预览已渲染：${basename(path)}（${data.result.clips} 个镜头）`);
  return data.result;
}

async function matchCaptions() {
  if (!state.project) {
    setStatus("请先新建或载入项目。");
    return null;
  }
  setStatus("正在匹配字幕到镜头...");
  const data = await api(`/api/projects/${state.project.id}/captions`, {
    method: "POST",
    body: JSON.stringify({ text: $("captionText").value }),
  });
  state.project = data.project;
  renderCaptions();
  setStatus(`已匹配 ${data.captions.length} 条字幕。`);
  return data.captions;
}

function markIn() {
  $("inInput").value = $("video").currentTime.toFixed(3);
  syncFieldsToClip();
}

function markOut() {
  $("outInput").value = $("video").currentTime.toFixed(3);
  syncFieldsToClip();
}

function seekToInPoint() {
  $("video").currentTime = Number($("inInput").value || 0);
}

function useBest() {
  const asset = currentAsset();
  const best = bestAssetWindow(asset);
  if (!best) return;
  $("inInput").value = best.source_in;
  $("outInput").value = best.source_out;
  syncFieldsToClip();
  seekToInPoint();
}

function rejectCurrent() {
  $("roleInput").value = "reject";
  syncFieldsToClip();
  nextClip();
}

function nextClip() {
  if (!state.project?.clips?.length) return;
  syncFieldsToClip();
  state.selectedClipIndex = Math.min(state.project.clips.length - 1, state.selectedClipIndex + 1);
  loadClipToEditor();
}

function prevClip() {
  if (!state.project?.clips?.length) return;
  syncFieldsToClip();
  state.selectedClipIndex = Math.max(0, state.selectedClipIndex - 1);
  loadClipToEditor();
}

async function browse(path = "") {
  const data = await api(`/api/browse?path=${encodeURIComponent(path)}`);
  state.browserPath = data.path || "";
  $("browserPath").textContent = data.path || "磁盘";
  $("browserPanel").classList.remove("hidden");
  $("browserUpBtn").disabled = !data.parent;
  $("browserUpBtn").onclick = () => browse(data.parent);
  const list = $("browserList");
  list.innerHTML = "";
  data.items.forEach((item) => {
    const row = document.createElement("button");
    row.className = "browser-item";
    row.textContent = item.name;
    row.onclick = () => browse(item.path);
    list.appendChild(row);
  });
}

function useBrowsedFolder() {
  if (state.browserPath) {
    $("mediaDirInput").value = state.browserPath;
    $("browserPanel").classList.add("hidden");
    setStatus(`素材文件夹已设置：${state.browserPath}`);
  }
}

function bind() {
  $("newProjectBtn").onclick = () => withBusyButton("newProjectBtn", "创建中...", newProject);
  $("loadLatestBtn").onclick = () => withBusyButton("loadLatestBtn", "载入中...", loadLatestProject);
  $("scanBtn").onclick = () => withBusyButton("scanBtn", "扫描中...", scanProject);
  $("saveBtn").onclick = () => withBusyButton("saveBtn", "保存中...", saveProject);
  $("previewBtn").onclick = () => withBusyButton("previewBtn", "渲染中...", renderPreview);
  $("draftBtn").onclick = () => withBusyButton("draftBtn", "生成中...", createDraft);
  $("captionBtn").onclick = () => withBusyButton("captionBtn", "匹配中...", matchCaptions);
  $("browseBtn").onclick = () => browse($("mediaDirInput").value).catch(() => browse("").catch(showError));
  $("browserUseBtn").onclick = useBrowsedFolder;
  $("browserCloseBtn").onclick = () => $("browserPanel").classList.add("hidden");
  $("markInBtn").onclick = markIn;
  $("markOutBtn").onclick = markOut;
  $("useBestBtn").onclick = useBest;
  $("assignBtn").onclick = assignAssetToSlot;
  $("rejectBtn").onclick = rejectCurrent;
  $("nextBtn").onclick = nextClip;
  $("prevBtn").onclick = prevClip;
  ["inInput", "outInput", "speedInput", "roleInput", "rotationInput", "scaleStartInput", "scaleEndInput"].forEach((id) => {
    $(id).onchange = syncFieldsToClip;
  });
  document.addEventListener("keydown", (e) => {
    if (["INPUT", "SELECT", "TEXTAREA"].includes(e.target.tagName)) return;
    if (e.key === "i" || e.key === "I") markIn();
    if (e.key === "o" || e.key === "O") markOut();
    if (e.key === "ArrowRight") nextClip();
    if (e.key === "ArrowLeft") prevClip();
    if (e.code === "Space") {
      e.preventDefault();
      const v = $("video");
      if (v.paused) v.play(); else v.pause();
    }
  });
}

bind();
loadLatestProject().catch(() => setStatus("准备就绪"));
