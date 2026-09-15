import type { AnnotationSelection } from "./types";

interface Point {
  x: number;
  y: number;
}

interface Bounds {
  left: number;
  top: number;
  width: number;
  height: number;
}

const clamp = (value: number) => Math.min(1, Math.max(0, value));

export function pointerSelection(
  start: Point,
  end: Point,
  bounds: Bounds,
  page = 1,
  minimumDragPixels = 4,
): AnnotationSelection {
  const widthPixels = Math.abs(end.x - start.x);
  const heightPixels = Math.abs(end.y - start.y);
  const x1 = clamp((Math.min(start.x, end.x) - bounds.left) / Math.max(bounds.width, 1));
  const y1 = clamp((Math.min(start.y, end.y) - bounds.top) / Math.max(bounds.height, 1));
  const x2 = clamp((Math.max(start.x, end.x) - bounds.left) / Math.max(bounds.width, 1));
  const y2 = clamp((Math.max(start.y, end.y) - bounds.top) / Math.max(bounds.height, 1));

  if (widthPixels < minimumDragPixels && heightPixels < minimumDragPixels) {
    return {
      type: "point",
      page,
      x: clamp((end.x - bounds.left) / Math.max(bounds.width, 1)),
      y: clamp((end.y - bounds.top) / Math.max(bounds.height, 1)),
    };
  }

  return {
    type: "image_region",
    page,
    x: x1,
    y: y1,
    width: Math.max(0, x2 - x1),
    height: Math.max(0, y2 - y1),
  };
}

export function textRangeSelection(
  content: string,
  startOffset: number,
  endOffset: number,
  maxExcerptLength = 500,
): AnnotationSelection {
  const start = Math.max(0, Math.min(content.length, Math.min(startOffset, endOffset)));
  const end = Math.max(start, Math.min(content.length, Math.max(startOffset, endOffset)));
  const lineAt = (offset: number) => content.slice(0, offset).split("\n").length;
  return {
    type: "text_range",
    start,
    end,
    startLine: lineAt(start),
    endLine: lineAt(end),
    excerpt: content.slice(start, end).slice(0, maxExcerptLength),
  };
}
