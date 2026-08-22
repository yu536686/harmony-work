import { mkdir, stat } from "node:fs/promises";
import { dirname } from "node:path";
import PptxGenJS from "pptxgenjs";
import type {
  GenerationResult,
  PresentationColumn,
  PresentationSlideSpec,
  PresentationSpec,
  PresentationThemeName,
} from "./types";

interface Theme {
  background: string;
  surface: string;
  text: string;
  muted: string;
  accent: string;
  accent2: string;
}

const THEMES: Record<PresentationThemeName, Theme> = {
  tech_dark: { background: "0B1220", surface: "172033", text: "F8FAFC", muted: "AAB5C4", accent: "33C3F0", accent2: "8B5CF6" },
  clean_light: { background: "F7F9FC", surface: "FFFFFF", text: "172033", muted: "667085", accent: "2563EB", accent2: "14B8A6" },
  business_blue: { background: "EEF4FA", surface: "FFFFFF", text: "102A43", muted: "52667A", accent: "1261A0", accent2: "2F9E8F" },
  editorial: { background: "FAFAF8", surface: "FFFFFF", text: "202124", muted: "6B6B65", accent: "C9362B", accent2: "2D6A4F" },
};

const FONT = "Microsoft YaHei";

function addPageChrome(slide: PptxGenJS.Slide, pptx: PptxGenJS, theme: Theme, index: number): void {
  slide.background = { color: theme.background };
  slide.addShape(pptx.ShapeType.line, { x: 0.55, y: 7.08, w: 12.25, h: 0, line: { color: theme.muted, transparency: 72, width: 1 } });
  slide.addText("小亨 WorkBuddy", { x: 0.6, y: 7.13, w: 2.4, h: 0.16, fontFace: FONT, fontSize: 8, color: theme.muted, margin: 0 });
  slide.addText(String(index).padStart(2, "0"), { x: 12.1, y: 7.1, w: 0.6, h: 0.2, fontFace: FONT, fontSize: 8, color: theme.muted, align: "right", margin: 0 });
}

function addTitle(slide: PptxGenJS.Slide, pptx: PptxGenJS, title: string, subtitle: string | undefined, theme: Theme, accent: string): void {
  slide.addShape(pptx.ShapeType.rect, { x: 0.65, y: 0.58, w: 0.08, h: 0.72, fill: { color: accent }, line: { color: accent } });
  slide.addText(title, { x: 0.92, y: 0.52, w: 11.6, h: 0.52, fontFace: FONT, fontSize: 25, bold: true, color: theme.text, margin: 0, breakLine: false });
  if (subtitle) {
    slide.addText(subtitle, { x: 0.94, y: 1.1, w: 11.2, h: 0.3, fontFace: FONT, fontSize: 11, color: theme.muted, margin: 0 });
  }
}

function addBulletList(slide: PptxGenJS.Slide, bullets: string[], theme: Theme, x: number, y: number, w: number, h: number): void {
  const runs = bullets.flatMap((bullet, index) => [
    { text: bullet, options: { bullet: { indent: 16 }, breakLine: index < bullets.length - 1 } },
  ]);
  slide.addText(runs, { x, y, w, h, fontFace: FONT, fontSize: 18, color: theme.text, breakLine: false, margin: 0.12, paraSpaceAfter: 13, valign: "middle", fit: "shrink" });
}

function addColumn(slide: PptxGenJS.Slide, pptx: PptxGenJS, column: PresentationColumn, theme: Theme, accent: string, x: number): void {
  slide.addShape(pptx.ShapeType.roundRect, { x, y: 1.7, w: 5.75, h: 4.65, rectRadius: 0.08, fill: { color: theme.surface }, line: { color: accent, transparency: 72, width: 1 } });
  if (column.title) slide.addText(column.title, { x: x + 0.38, y: 2.05, w: 4.95, h: 0.42, fontFace: FONT, fontSize: 19, bold: true, color: accent, margin: 0 });
  if (column.body) slide.addText(column.body, { x: x + 0.38, y: 2.65, w: 4.95, h: 2.9, fontFace: FONT, fontSize: 15, color: theme.text, margin: 0, fit: "shrink", breakLine: false, valign: "top" });
  if (column.bullets?.length) addBulletList(slide, column.bullets, theme, x + 0.25, 2.55, 5.1, 3.25);
}

function addContentSlide(pptx: PptxGenJS, spec: PresentationSlideSpec, theme: Theme, index: number): void {
  const slide = pptx.addSlide();
  const accent = (spec.accent ?? theme.accent).replace(/^#/, "");
  addPageChrome(slide, pptx, theme, index);
  addTitle(slide, pptx, spec.title, spec.subtitle, theme, accent);
  const layout = spec.layout ?? (spec.image_path ? "image" : "bullets");

  if (layout === "section") {
    slide.addText(spec.body ?? spec.subtitle ?? "", { x: 1.25, y: 2.6, w: 10.85, h: 1.25, fontFace: FONT, fontSize: 28, bold: true, color: theme.text, align: "center", valign: "middle", fit: "shrink", margin: 0 });
    slide.addShape(pptx.ShapeType.line, { x: 5.35, y: 4.2, w: 2.65, h: 0, line: { color: accent, width: 4, beginArrowType: "none", endArrowType: "none" } });
  } else if (layout === "two_column") {
    addColumn(slide, pptx, spec.left ?? { title: "要点", bullets: spec.bullets }, theme, accent, 0.72);
    addColumn(slide, pptx, spec.right ?? { title: "说明", body: spec.body }, theme, theme.accent2, 6.85);
  } else if (layout === "quote") {
    slide.addText(`“${spec.body ?? spec.bullets?.[0] ?? ""}”`, { x: 1.25, y: 2.15, w: 10.8, h: 2.7, fontFace: FONT, fontSize: 27, italic: true, color: theme.text, align: "center", valign: "middle", fit: "shrink", margin: 0.2 });
  } else if (layout === "image" && spec.image_path) {
    slide.addImage({ path: spec.image_path, x: 0.82, y: 1.55, w: 7.2, h: 4.95, transparency: 0 });
    if (spec.body) slide.addText(spec.body, { x: 8.38, y: 1.9, w: 3.95, h: 3.9, fontFace: FONT, fontSize: 17, color: theme.text, fit: "shrink", breakLine: false, valign: "middle", margin: 0 });
    if (spec.bullets?.length) addBulletList(slide, spec.bullets, theme, 8.2, 1.85, 4.15, 4.2);
  } else if (layout === "summary") {
    const bullets = spec.bullets ?? (spec.body ? [spec.body] : []);
    bullets.slice(0, 4).forEach((bullet, itemIndex) => {
      const x = 0.8 + (itemIndex % 2) * 6.2;
      const y = 1.65 + Math.floor(itemIndex / 2) * 2.45;
      slide.addShape(pptx.ShapeType.roundRect, { x, y, w: 5.72, h: 1.95, rectRadius: 0.06, fill: { color: theme.surface }, line: { color: itemIndex % 2 === 0 ? accent : theme.accent2, transparency: 65 } });
      slide.addText(String(itemIndex + 1).padStart(2, "0"), { x: x + 0.28, y: y + 0.3, w: 0.7, h: 0.45, fontFace: FONT, fontSize: 16, bold: true, color: itemIndex % 2 === 0 ? accent : theme.accent2, margin: 0 });
      slide.addText(bullet, { x: x + 1.05, y: y + 0.25, w: 4.15, h: 1.4, fontFace: FONT, fontSize: 17, color: theme.text, valign: "middle", fit: "shrink", breakLine: false, margin: 0 });
    });
  } else {
    if (spec.body) slide.addText(spec.body, { x: 0.95, y: 1.65, w: 11.25, h: 1.05, fontFace: FONT, fontSize: 16, color: theme.muted, margin: 0, fit: "shrink", breakLine: false });
    addBulletList(slide, spec.bullets ?? [], theme, 1.02, spec.body ? 2.65 : 1.75, 11.1, spec.body ? 3.35 : 4.4);
  }
}

export async function generatePresentation(spec: PresentationSpec): Promise<GenerationResult> {
  if (!spec.path.toLowerCase().endsWith(".pptx")) throw new Error("presentation path must end with .pptx");
  if (!spec.title.trim()) throw new Error("presentation title is required");
  if (!spec.slides.length) throw new Error("presentation slides must not be empty");
  if (spec.slides.length > 40) throw new Error("presentation supports at most 40 content slides");

  const pptx = new PptxGenJS();
  const theme = THEMES[spec.theme ?? "clean_light"];
  pptx.layout = "LAYOUT_WIDE";
  pptx.author = spec.author ?? "小亨 WorkBuddy";
  pptx.subject = spec.subtitle ?? spec.title;
  pptx.title = spec.title;
  pptx.company = "Harmony";
  pptx.theme = {
    headFontFace: FONT,
    bodyFontFace: FONT,
  };

  const cover = pptx.addSlide();
  cover.background = { color: theme.background };
  cover.addShape(pptx.ShapeType.rect, { x: 0, y: 0, w: 0.18, h: 7.5, fill: { color: theme.accent }, line: { color: theme.accent } });
  cover.addText(spec.title, { x: 0.95, y: 2.35, w: 10.9, h: 1.2, fontFace: FONT, fontSize: 34, bold: true, color: theme.text, margin: 0, fit: "shrink", breakLine: false });
  if (spec.subtitle) cover.addText(spec.subtitle, { x: 1.0, y: 3.72, w: 10.5, h: 0.55, fontFace: FONT, fontSize: 17, color: theme.muted, margin: 0 });
  cover.addShape(pptx.ShapeType.line, { x: 1.0, y: 4.58, w: 2.25, h: 0, line: { color: theme.accent2, width: 4 } });
  cover.addText("HARMONY · XIAOHENG WORKBUDDY", { x: 1.0, y: 6.65, w: 4.4, h: 0.25, fontFace: "Arial", fontSize: 9, color: theme.muted, charSpacing: 1.2, margin: 0 });

  spec.slides.forEach((slide, index) => addContentSlide(pptx, slide, theme, index + 2));
  await mkdir(dirname(spec.path), { recursive: true });
  await pptx.writeFile({ fileName: spec.path });
  const metadata = await stat(spec.path);
  return { path: spec.path, bytes: metadata.size, details: { slides: spec.slides.length + 1, theme: spec.theme ?? "clean_light" } };
}
