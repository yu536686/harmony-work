import { useEffect, useId, useRef, useState, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isMacNativeUI } from "../../ui/useMacNativeMenu";

interface NativeAnnotationDraft {
  left: number;
  top: number;
}

interface NativeAnnotationOptions {
  draft: NativeAnnotationDraft | null;
  rootRef: RefObject<HTMLElement>;
  initialComment: string;
  title: string;
  stageLabel: string;
  cancelLabel: string;
  onStage: (comment: string) => void;
  onClose: () => void;
}

export function useMacNativeAnnotation({
  draft, rootRef, initialComment, title, stageLabel, cancelLabel, onStage, onClose,
}: NativeAnnotationOptions) {
  const baseId = useId();
  const sequence = useRef(0);
  const activeId = useRef("");
  const stageRef = useRef(onStage);
  const closeRef = useRef(onClose);
  stageRef.current = onStage;
  closeRef.current = onClose;
  const [nativeFailed, setNativeFailed] = useState(false);
  const nativeMode = isMacNativeUI() && !nativeFailed;

  useEffect(() => {
    if (!nativeMode || !draft || !rootRef.current) return;
    const annotationId = `${baseId}-${++sequence.current}`;
    activeId.current = annotationId;
    const rect = rootRef.current.getBoundingClientRect();
    const onNativeStage = (event: Event) => {
      const detail = (event as CustomEvent<{ annotationId: string; comment: string }>).detail;
      if (detail?.annotationId === activeId.current) stageRef.current(detail.comment);
    };
    const onNativeClose = (event: Event) => {
      if ((event as CustomEvent<{ annotationId: string }>).detail?.annotationId === activeId.current) closeRef.current();
    };
    window.addEventListener("harmony:native-annotation-stage", onNativeStage);
    window.addEventListener("harmony:native-annotation-close", onNativeClose);
    void invoke("show_macos_annotation", {
      annotationId, title, comment: initialComment, stageLabel, cancelLabel,
      x: rect.left + draft.left, y: rect.top + draft.top,
    }).catch(() => setNativeFailed(true));
    return () => {
      if (activeId.current === annotationId) activeId.current = "";
      void invoke("close_macos_annotation", { annotationId });
      window.removeEventListener("harmony:native-annotation-stage", onNativeStage);
      window.removeEventListener("harmony:native-annotation-close", onNativeClose);
    };
  }, [nativeMode, draft, rootRef, baseId, title, stageLabel, cancelLabel]);

  return nativeMode;
}
