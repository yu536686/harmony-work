import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { MessageSquarePlus, X } from "lucide-react";
import { pointerSelection } from "./annotation/selection";
import type { AnnotationSelection, BrowserAnnotationTarget, WorkbenchAnnotation } from "./annotation/types";

interface BrowserAnnotationCanvasProps {
  enabled: boolean;
  sessionId: string;
  target: BrowserAnnotationTarget;
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

export default function BrowserAnnotationCanvas({
  enabled,
  sessionId,
  target,
  annotations,
  focusedAnnotationId,
  children,
  onStage,
  onCancelDraft,
}: BrowserAnnotationCanvasProps) {
  const { t } = useTranslation();
  const rootRef = useRef<HTMLDivElement>(null);
  const pointerStartRef = useRef<{ x: number; y: number } | null>(null);
  const [dragSelection, setDragSelection] = useState<AnnotationSelection | null>(null);
  const [draft, setDraft] = useState<DraftSelection | null>(null);
  const [comment, setComment] = useState("");

  const visibleAnnotations = useMemo(
    () => annotations.filter((item) => item.target.type === "browser" && item.target.id === target.id),
    [annotations, target.id],
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
    if (!enabled) return;
    pointerStartRef.current = { x: event.clientX, y: event.clientY };
    event.currentTarget.setPointerCapture?.(event.pointerId);
  };

  const pointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!pointerStartRef.current || !rootRef.current) return;
    setDragSelection(pointerSelection(pointerStartRef.current, { x: event.clientX, y: event.clientY }, rootRef.current.getBoundingClientRect()));
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

  const stage = () => {
    if (!draft || !comment.trim()) return;
    onStage({
      id: draft.annotationId ?? annotationId(),
      sessionId,
      target,
      selection: draft.selection,
      comment: comment.trim(),
      status: "staged",
    });
    setDraft(null);
    setDragSelection(null);
    setComment("");
  };

  const closeDraft = () => {
    setDraft(null);
    setDragSelection(null);
    setComment("");
    onCancelDraft?.();
  };

  const markerItems = [
    ...visibleAnnotations,
    ...(dragSelection ? [{ id: "drag-preview", selection: dragSelection } as Pick<WorkbenchAnnotation, "id" | "selection">] : []),
  ];

  return (
    <div ref={rootRef} className={`browser-annotation-canvas${enabled ? " is-enabled" : ""}`}>
      {children}
      <div
        className={`annotation-capture${enabled ? " is-active" : ""}`}
        aria-label="浏览器批注画布"
        onPointerDown={pointerDown}
        onPointerMove={pointerMove}
        onPointerUp={pointerUp}
      />
      {markerItems.map((item, index) => (
        item.selection.type === "text_range" || item.selection.type === "page" ? null : (
          <button
            type="button"
            key={item.id}
            className={`annotation-marker is-${item.selection.type}${item.id === "drag-preview" ? " is-draft" : ""}`}
            style={item.selection.type === "image_region" ? {
              left: `${item.selection.x * 100}%`, top: `${item.selection.y * 100}%`,
              width: `${item.selection.width * 100}%`, height: `${item.selection.height * 100}%`,
            } : { left: `${item.selection.x * 100}%`, top: `${item.selection.y * 100}%` }}
            onClick={() => {
              if (item.id === "drag-preview") return;
              const existing = visibleAnnotations.find((annotation) => annotation.id === item.id);
              if (!existing) return;
              setDraft({ selection: existing.selection, ...anchorForSelection(existing.selection), annotationId: existing.id });
              setComment(existing.comment);
            }}
            aria-label={item.id === "drag-preview" ? "当前选区" : `批注 ${index + 1}`}
          >
            {item.id !== "drag-preview" && <span>{index + 1}</span>}
          </button>
        )
      ))}
      {draft && (
        <div className="annotation-popover" style={{ left: draft.left, top: draft.top }}>
          <div className="annotation-popover-head">
            <span><MessageSquarePlus size={14} /> {t("chat.annotationRequirement")}</span>
            <button type="button" aria-label={t("chat.annotationCancel")} onClick={closeDraft}><X size={14} /></button>
          </div>
          <textarea autoFocus value={comment} onChange={(event) => setComment(event.target.value)} placeholder={t("chat.annotationPlaceholder")} maxLength={2000} disabled={!enabled} />
          <button type="button" className="annotation-stage-button" disabled={!enabled || !comment.trim()} onClick={stage}>{t("chat.annotationStage")}</button>
        </div>
      )}
    </div>
  );
}
