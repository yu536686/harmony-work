export interface ArtifactAnnotationTarget {
  type: "artifact";
  id: string;
  title: string;
  uri?: string | null;
}

export interface BrowserAnnotationTarget {
  type: "browser";
  id: string;
  title: string;
  uri: string;
}

export type AnnotationTarget = ArtifactAnnotationTarget | BrowserAnnotationTarget;

export type AnnotationSelection =
  | {
      type: "image_region";
      page?: number;
      x: number;
      y: number;
      width: number;
      height: number;
    }
  | {
      type: "point";
      page?: number;
      x: number;
      y: number;
    }
  | {
      type: "text_range";
      start: number;
      end: number;
      startLine: number;
      endLine: number;
      excerpt: string;
    }
  | {
      type: "page";
      url: string;
    };

export interface WorkbenchAnnotation {
  id: string;
  sessionId: string;
  target: AnnotationTarget;
  selection: AnnotationSelection;
  comment: string;
  status: "draft" | "staged" | "failed";
}

export interface WorkbenchCommentContext {
  id: string;
  target: AnnotationTarget;
  selection: AnnotationSelection;
  comment: string;
}

export interface AnnotationDraftState {
  items: WorkbenchAnnotation[];
  sendState: "idle" | "sending" | "failed";
}
