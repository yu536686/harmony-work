import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { convertFileSrc } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { AlertCircle, ExternalLink, FileText, FolderOpen, LoaderCircle, Save } from "lucide-react";
import type { SessionArtifact } from "../../services/tauri";
import { chatApi, openPath, type ArtifactPreviewPayload } from "../../services/tauri";
import AnnotationLayer from "./AnnotationLayer";
import type { WorkbenchAnnotation } from "./annotation/types";
import { sanitizePreviewHtml } from "./htmlSanitizer";

interface ArtifactPreviewProps {
  sessionId: string;
  artifact: SessionArtifact;
  annotationMode?: boolean;
  annotations?: WorkbenchAnnotation[];
  focusedAnnotationId?: string | null;
  onStageAnnotation?: (annotation: WorkbenchAnnotation) => void;
  onCancelAnnotation?: () => void;
}

export default function ArtifactPreview({
  sessionId,
  artifact,
  annotationMode = false,
  annotations = [],
  focusedAnnotationId,
  onStageAnnotation = () => {},
  onCancelAnnotation,
}: ArtifactPreviewProps) {
  const { t } = useTranslation();
  const [preview, setPreview] = useState<ArtifactPreviewPayload | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const identity = JSON.stringify([sessionId, artifact.id, artifact.uri]);
  const [interactiveConsent, setInteractiveConsent] = useState<string | null>(null);
  const [directoryConsent, setDirectoryConsent] = useState<string | null>(null);
  const [confirmation, setConfirmation] = useState<{ identity: string; kind: "interactive" | "directory" } | null>(null);
  const allowNetwork = interactiveConsent === identity;
  const allowDirectory = directoryConsent === identity;
  const requestKey = JSON.stringify([identity, allowNetwork, allowDirectory]);
  const [loadedRequestKey, setLoadedRequestKey] = useState<string | null>(null);
  const staticHtml = useMemo(() => {
    if (allowNetwork || preview?.kind !== "html" || preview.content == null || !preview.preview_base_url) return null;
    try { return sanitizePreviewHtml(preview.content, preview.preview_base_url); }
    catch { return null; }
  }, [allowNetwork, preview]);

  useEffect(() => {
    setInteractiveConsent(null);
    setDirectoryConsent(null);
    setConfirmation(null);
  }, [identity]);

  useEffect(() => {
    if (!artifact.uri) {
      setPreview(null);
      setError(null);
      setLoading(false);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(null);
    chatApi.getArtifactPreview(sessionId, artifact.uri, allowNetwork, allowDirectory)
      .then((value) => {
        if (!cancelled) {
          setPreview(value);
          setLoadedRequestKey(requestKey);
        }
      })
      .catch((reason) => {
        if (!cancelled) {
          setPreview(null);
          setError(String(reason));
          setLoadedRequestKey(requestKey);
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [artifact.uri, sessionId, allowNetwork, allowDirectory, requestKey]);

  const openArtifact = () => {
    const target = preview?.uri || artifact.uri;
    if (target) void openPath(target).catch(console.error);
  };

  const revealArtifact = () => {
    const target = preview?.uri || artifact.uri;
    if (!target) return;
    const normalized = target.replace(/\\/g, "/");
    const parent = normalized.slice(0, normalized.lastIndexOf("/"));
    if (parent) void openPath(parent).catch(console.error);
  };

  const saveArtifactAs = async () => {
    const target = preview?.uri || artifact.uri;
    if (!target) return;
    const destination = await save({ defaultPath: artifact.name });
    if (!destination) return;
    await chatApi.saveArtifactCopy(sessionId, target, destination);
  };

  if (loading || (artifact.uri && loadedRequestKey !== requestKey)) {
    return (
      <div className="artifact-preview-state" role="status">
        <LoaderCircle size={18} className="workbench-spin" />
        <span>{t("chat.workbenchPreviewLoading")}</span>
      </div>
    );
  }

  if (error) {
    return (
      <div className="artifact-preview-state is-error" role="alert">
        <AlertCircle size={18} />
        <div>
          <strong>{t("chat.workbenchPreviewFailed")}</strong>
          <p>{error}</p>
          {artifact.uri && (
            <button type="button" className="workbench-text-command" onClick={openArtifact}>
              <ExternalLink size={14} /> {t("chat.workbenchOpenWithSystem")}
            </button>
          )}
        </div>
      </div>
    );
  }

  if (!preview) {
    return (
      <div className="artifact-preview-state">
        <FileText size={18} />
        <span>{artifact.content_summary || t("chat.workbenchNoPreview")}</span>
      </div>
    );
  }

  let previewContent;
  if (preview.kind === "image") {
    previewContent = (
      <div className="artifact-preview-canvas">
        <img src={convertFileSrc(preview.uri)} alt={artifact.name} />
      </div>
    );
  } else if (preview.kind === "html") {
    previewContent = (
      <div className="artifact-preview-html">
        <div className="artifact-preview-network">
          <span>{allowNetwork ? "交互预览：网页可联网和跳转，仅运行可信内容。" : "受限静态预览"}</span>
          <button type="button" className="workbench-text-command"
            onClick={() => allowNetwork ? setInteractiveConsent(null) : setConfirmation({ identity, kind: "interactive" })}>
            {allowNetwork ? "返回静态预览" : "启用交互并允许联网"}
          </button>
        </div>
        {preview.is_outside_workspace && <div className="artifact-preview-network">
          <span>{allowDirectory ? "已允许读取同目录及其子目录中的所有文件" : "工作区外：仅授权当前 HTML 文件"}</span>
          <button type="button" className="workbench-text-command"
            onClick={() => allowDirectory ? setDirectoryConsent(null) : setConfirmation({ identity, kind: "directory" })}>
            {allowDirectory ? "改为仅当前 HTML" : "允许读取同目录资源"}
          </button>
        </div>}
        {confirmation?.identity === identity && <div className="artifact-preview-consent" role="alertdialog"
          aria-label={confirmation.kind === "interactive" ? "交互联网授权" : "同目录资源授权"}>
          <p>{confirmation.kind === "interactive"
            ? "启用后网页可联网和跳转，并运行原始脚本；网页可读取已授权范围内的文件。请仅对可信内容启用。"
            : "允许后，同目录及其子目录中的其他文件均可被网页读取，不仅是图片和样式。交互模式还可将这些内容发送到网络。请确认目录内没有不希望网页读取的文件。"}</p>
          {confirmation.kind === "directory" && <code>{preview.uri.replace(/\\/g, "/").replace(/\/[^/]*$/, "/")}</code>}
          <div className="artifact-preview-consent-actions">
            <button type="button" className="workbench-text-command" onClick={() => setConfirmation(null)}>取消</button>
            <button type="button" className="workbench-text-command" onClick={() => {
              if (confirmation.kind === "interactive") setInteractiveConsent(identity);
              else setDirectoryConsent(identity);
              setConfirmation(null);
            }}>{confirmation.kind === "interactive" ? "确认启用交互并允许联网" : "确认允许读取同目录资源"}</button>
          </div>
        </div>}
        {preview.truncated && !allowNetwork && <div className="artifact-preview-truncated">{t("chat.workbenchPreviewTruncated")}</div>}
        {allowNetwork && preview.preview_url ? <iframe
          key={`${requestKey}:interactive`}
          className="artifact-preview-frame"
          src={preview.preview_url}
          title={artifact.name}
          sandbox="allow-scripts allow-downloads"
          referrerPolicy="no-referrer"
        /> : !allowNetwork && staticHtml !== null ? <iframe
          key={`${requestKey}:static`}
          className="artifact-preview-frame"
          srcDoc={staticHtml}
          title={artifact.name}
          sandbox=""
          referrerPolicy="no-referrer"
        /> : <div role="alert" className="artifact-preview-state">无法建立安全预览，请重新打开成果。</div>}
      </div>
    );
  } else if (preview.kind === "pdf") {
    previewContent = (
      <iframe
        className="artifact-preview-frame"
        src={convertFileSrc(preview.uri)}
        title={artifact.name}
      />
    );
  } else if (preview.kind === "text" || preview.kind === "code") {
    previewContent = (
      <div className="artifact-preview-text-wrap">
        <pre className={`artifact-preview-text${preview.kind === "code" ? " is-code" : ""}`}>
          {preview.content || ""}
        </pre>
        {preview.truncated && (
          <div className="artifact-preview-truncated">{t("chat.workbenchPreviewTruncated")}</div>
        )}
      </div>
    );
  } else {
    previewContent = (
      <div className="artifact-preview-external">
        <FileText size={28} />
        <strong>{artifact.name}</strong>
        <p>{artifact.content_summary || t("chat.workbenchExternalHint")}</p>
        <span>{preview.size_bytes.toLocaleString()} bytes</span>
        <div className="artifact-preview-actions">
          <button type="button" className="workbench-primary-command" onClick={openArtifact}>
            <ExternalLink size={14} /> {t("chat.workbenchOpenArtifact")}
          </button>
          <button type="button" className="workbench-secondary-command" onClick={() => void saveArtifactAs()}>
            <Save size={14} /> {t("chat.workbenchSaveAs")}
          </button>
          <button type="button" className="workbench-secondary-command" onClick={revealArtifact}>
            <FolderOpen size={14} /> {t("chat.workbenchReveal")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <AnnotationLayer
      enabled={annotationMode && preview.kind !== "external"}
      sessionId={sessionId}
      artifact={artifact}
      previewKind={preview.kind}
      textContent={preview.content}
      annotations={annotations}
      focusedAnnotationId={focusedAnnotationId}
      onStage={onStageAnnotation}
      onCancelDraft={onCancelAnnotation}
    >
      {previewContent}
    </AnnotationLayer>
  );
}
