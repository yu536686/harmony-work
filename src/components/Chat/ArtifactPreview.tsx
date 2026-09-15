import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { convertFileSrc } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { AlertCircle, ExternalLink, FileText, FolderOpen, LoaderCircle, Save } from "lucide-react";
import type { SessionArtifact } from "../../services/tauri";
import { chatApi, openPath, type ArtifactPreviewPayload } from "../../services/tauri";
import AnnotationLayer from "./AnnotationLayer";
import type { WorkbenchAnnotation } from "./annotation/types";

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

  useEffect(() => {
    if (!artifact.uri) {
      setPreview(null);
      setError(null);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(null);
    chatApi.getArtifactPreview(sessionId, artifact.uri)
      .then((value) => {
        if (!cancelled) setPreview(value);
      })
      .catch((reason) => {
        if (!cancelled) {
          setPreview(null);
          setError(String(reason));
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [artifact.uri, sessionId]);

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

  if (loading) {
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
      <iframe
        className="artifact-preview-frame"
        src={convertFileSrc(preview.uri)}
        title={artifact.name}
        sandbox="allow-scripts allow-same-origin"
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
