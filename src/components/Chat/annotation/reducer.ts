import type {
  AnnotationDraftState,
  WorkbenchAnnotation,
  WorkbenchCommentContext,
} from "./types";

export const emptyAnnotationDraft: AnnotationDraftState = {
  items: [],
  sendState: "idle",
};

export function addAnnotation(
  state: AnnotationDraftState,
  annotation: WorkbenchAnnotation,
): AnnotationDraftState {
  const index = state.items.findIndex((item) => item.id === annotation.id);
  const items = [...state.items];
  if (index >= 0) items[index] = annotation;
  else items.push(annotation);
  return { ...state, items, sendState: "idle" };
}

export function removeAnnotation(
  state: AnnotationDraftState,
  annotationId: string,
): AnnotationDraftState {
  return {
    ...state,
    items: state.items.filter((item) => item.id !== annotationId),
    sendState: "idle",
  };
}

export function markAnnotationSendFailed(state: AnnotationDraftState): AnnotationDraftState {
  return {
    items: state.items.map((item) => ({ ...item, status: "failed" })),
    sendState: "failed",
  };
}

export function markAnnotationSending(state: AnnotationDraftState): AnnotationDraftState {
  return {
    items: state.items.map((item) => ({ ...item, status: "staged" })),
    sendState: "sending",
  };
}

export function toCommentContext(annotation: WorkbenchAnnotation): WorkbenchCommentContext {
  return {
    id: annotation.id,
    target: annotation.target,
    selection: annotation.selection,
    comment: annotation.comment,
  };
}

export function annotationAnchor(annotation: WorkbenchAnnotation, index: number): string {
  const selection = annotation.selection;
  let location = annotation.target.title;
  if (selection.type === "text_range") {
    location = selection.startLine === selection.endLine
      ? `第 ${selection.startLine} 行`
      : `第 ${selection.startLine}-${selection.endLine} 行`;
  } else if (selection.type === "image_region" || selection.type === "point") {
    location = selection.page ? `第 ${selection.page} 页` : annotation.target.title;
  } else if (selection.type === "page") {
    location = "网页";
  }
  return `批注 ${index + 1} · ${location}`;
}
