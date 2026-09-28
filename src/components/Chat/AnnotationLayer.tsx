import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { MessageSquarePlus, X } from "lucide-react";
import type { SessionArtifact } from "../../services/tauri";
import { pointerSelection, textRangeSelection } from "./annotation/selection";
import type { AnnotationSelection, WorkbenchAnnotation } from "./annotation/types";
import { useMacNativeAnnotation } from "./annotation/useMacNativeAnnotation";

interface AnnotationLayerProps {
  enabled: boolean;
  sessionId: string;
  artifact: SessionArtifact;
  previewKind: "image" | "text" | "code" | "html" | "pdf" | "external";
  textContent?: string | null;
  annotations: WorkbenchAnnotation[];
  focusedAnnotationId?: string | null;
  children: ReactNode;
  onStage: (annotation: WorkbenchAnnotation) => void;
  onCancelDraft?: () => void;
}

interface DraftSelection {
  selection: AnnotationSelection;
  left: number;
  top: number;
  annotationId?: string;
}

const annotationId = () => globalThis.crypto?.randomUUID?.() ?? `annotation-${Date.now()}-${Math.random().toString(16).slice(2)}`;

function anchorForSelection(selection: AnnotationSelection) {
  if (selection.type === "text_range" || selection.type === "page") return { left: 20, top: 20 };
  return {
    left: Math.min(82, Math.max(4, (selection.x + (selection.type === "image_region" ? selection.width : 0)) * 100)),
    top: Math.min(78, Math.max(4, (selection.y + (selection.type === "image_region" ? selection.height : 0)) * 100)),
  };
}

export default function AnnotationLayer({
  enabled,
  sessionId,
  artifact,
  previewKind,
  textContent,
  annotations,
  focusedAnnotationId,
  children,
  onStage,
  onCancelDraft,
}: AnnotationLayerProps) {
  const { t } = useTranslation();
  const rootRef = useRef<HTMLDivElement>(null);
  const pointerStartRef = useRef<{ x: number; y: number } | null>(null);
  const [dragSelection, setDragSelection] = useState<AnnotationSelection | null>(null);
  const [draft, setDraft] = useState<DraftSelection | null>(null);
  const [comment, setComment] = useState("");

  const visibleAnnotations = useMemo(
    () => annotations.filter((item) => item.target.id === artifact.id),
    [annotations, artifact.id],
  );

  useEffect(() => {
    if (!focusedAnnotationId) return;
    const annotation = visibleAnnotations.find((item) => item.id === focusedAnnotationId);
    if (!annotation) return;
    const anchor = anchorForSelection(annotation.selection);
    setDraft({ selection: annotation.selection, ...anchor, annotationId: annotation.id });
    setComment(annotation.comment);
  }, [focusedAnnotationId, visibleAnnotations]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || !draft) return;
      setDraft(null);
      setDragSelection(null);
      setComment("");
      onCancelDraft?.();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [draft, onCancelDraft]);

  const pointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!enabled || (previewKind !== "image" && previewKind !== "html" && previewKind !== "pdf")) return;
    pointerStartRef.current = { x: event.clientX, y: event.clientY };
    event.currentTarget.setPointerCapture?.(event.pointerId);
  };

  const pointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!pointerStartRef.current || !rootRef.current) return;
    const bounds = rootRef.current.getBoundingClientRect();
    setDragSelection(pointerSelection(
      pointerStartRef.current,
      { x: event.clientX, y: event.clientY },
      bounds,
    ));
  };

  const pointerUp = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!pointerStartRef.current || !rootRef.current) return;
    const bounds = rootRef.current.getBoundingClientRect();
    const selection = pointerSelection(pointerStartRef.current, { x: event.clientX, y: event.clientY }, bounds);
    pointerStartRef.current = null;
    setDragSelection(null);
    setDraft({
      selection,
      left: Math.min(bounds.width - 248, Math.max(8, event.clientX - bounds.left + 10)),
      top: Math.min(bounds.height - 150, Math.max(8, event.clientY - bounds.top + 10)),
    });
    setComment("");
  };

  const captureTextSelection = () => {
    if (!enabled || (previewKind !== "text" && previewKind !== "code") || !rootRef.current || !textContent) return;
    const selection = window.getSelection();
    if (!selection || selection.isCollapsed || selection.rangeCount === 0) return;
    const range = selection.getRangeAt(0);
    if (!rootRef.current.contains(range.commonAncestorContainer)) return;
    const excerpt = selection.toString();
    if (!excerpt.trim()) return;
    const textRoot = rootRef.current.querySelector(".artifact-preview-text");
    if (!textRoot || !textRoot.contains(range.commonAncestorContainer)) return;
    const before = document.createRange();
    before.selectNodeContents(textRoot);
    before.setEnd(range.startContainer, range.startOffset);
    const start = before.toString().length;
    const rect = range.getBoundingClientRect();
    const bounds = rootRef.current.getBoundingClientRect();
    setDraft({
      selection: textRangeSelection(textContent, start, start + excerpt.length),
      left: Math.min(bounds.width - 248, Math.max(8, rect.left - bounds.left)),
      top: Math.min(bounds.height - 150, Math.max(8, rect.bottom - bounds.top + 8)),
    });
    setComment("");
  };

  const stageComment = (nextComment: string) => {
    if (!draft || !nextComment.trim()) return;
    onStage({
      id: draft.annotationId ?? annotationId(),
      sessionId,
      target: {
        type: "artifact",
        id: artifact.id,
        title: artifact.name,
        uri: artifact.uri,
      },
      selection: draft.selection,
      comment: nextComment.trim(),
      status: "staged",
    });
    setDraft(null);
    setDragSelection(null);
    setComment("");
    window.getSelection()?.removeAllRanges();
  };

  const closeDraft = () => {
    setDraft(null);
    setDragSelection(null);
    setComment("");
    onCancelDraft?.();
  };

  const nativeAnnotation = useMacNativeAnnotation({
    draft: enabled ? draft : null, rootRef, initialComment: comment,
    title: t("chat.annotationRequirement"),
    stageLabel: t("chat.annotationStage"),
    cancelLabel: t("chat.annotationCancel"),
    onStage: stageComment,
    onClose: closeDraft,
  });

  return (
    <div ref={rootRef} className={`annotation-root${enabled ? " is-enabled" : ""}`} onMouseUp={captureTextSelection}>
      {children}
      <div
        className={`annotation-capture${enabled && (previewKind === "image" || previewKind === "html" || previewKind === "pdf") ? " is-active" : ""}`}
        aria-label="批注画布"
        onPointerDown={pointerDown}
        onPointerMove={pointerMove}
        onPointerUp={pointerUp}
      />

      {[...visibleAnnotations, ...(dragSelection ? [{ id: "drag-preview", selection: dragSelection } as Pick<WorkbenchAnnotation, "id" | "selection">] : [])].map((item, index) => (
        item.selection.type === "text_range" || item.selection.type === "page" ? null : (
          <button
            type="button"
            key={item.id}
            className={`annotation-marker is-${item.selection.type}${item.id === "drag-preview" ? " is-draft" : ""}`}
            style={item.selection.type === "image_region" ? {
              left: `${item.selection.x * 100}%`,
              top: `${item.selection.y * 100}%`,
              width: `${item.selection.width * 100}%`,
              height: `${item.selection.height * 100}%`,
            } : {
              left: `${item.selection.x * 100}%`,
              top: `${item.selection.y * 100}%`,
            }}
            onClick={() => {
              if (item.id === "drag-preview") return;
              const existing = visibleAnnotations.find((annotation) => annotation.id === item.id);
              if (!existing) return;
              const anchor = anchorForSelection(existing.selection);
              setDraft({ selection: existing.selection, ...anchor, annotationId: existing.id });
              setComment(existing.comment);
            }}
            aria-label={item.id === "drag-preview" ? "当前选区" : `批注 ${index + 1}`}
          >
            {item.id !== "drag-preview" && <span>{index + 1}</span>}
          </button>
        )
      ))}

      {draft && !nativeAnnotation && (
        <div className="annotation-popover" style={{ left: draft.left, top: draft.top }}>
          <div className="annotation-popover-head">
            <span><MessageSquarePlus size={14} /> {t("chat.annotationRequirement")}</span>
            <button type="button" aria-label={t("chat.annotationCancel")} onClick={closeDraft}>
              <X size={14} />
            </button>
          </div>
          <textarea
            autoFocus
            value={comment}
            onChange={(event) => setComment(event.target.value)}
            placeholder={t("chat.annotationPlaceholder")}
            maxLength={2000}
            disabled={!enabled}
          />
          <button type="button" className="annotation-stage-button" disabled={!enabled || !comment.trim()} onClick={() => stageComment(comment)}>
            {t("chat.annotationStage")}
          </button>
        </div>
      )}
    </div>
  );
}
