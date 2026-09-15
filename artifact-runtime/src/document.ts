import { mkdir, readFile, stat, writeFile } from "node:fs/promises";
import { dirname, extname } from "node:path";
import {
  AlignmentType,
  BorderStyle,
  Document,
  Footer,
  Header,
  HeadingLevel,
  ImageRun,
  PageBreak,
  Packer,
  Paragraph,
  ShadingType,
  Table,
  TableCell,
  TableRow,
  TextRun,
  WidthType,
} from "docx";
import type { DocumentSectionSpec, DocumentSpec, GenerationResult } from "./types";

const BODY_FONT = "Microsoft YaHei";
const TABLE_BORDER = { style: BorderStyle.SINGLE, size: 4, color: "D9DEE7" };

function textParagraph(text: string): Paragraph {
  return new Paragraph({
    children: [new TextRun({ text, font: BODY_FONT, size: 22, color: "30343B" })],
    spacing: { after: 160, line: 330 },
  });
}

function tableCell(value: string | number | boolean | null, isHeader: boolean): TableCell {
  return new TableCell({
    width: { size: 50, type: WidthType.PERCENTAGE },
    shading: isHeader ? { fill: "EDF3FA", type: ShadingType.CLEAR } : undefined,
    margins: { top: 100, bottom: 100, left: 120, right: 120 },
    borders: {
      top: TABLE_BORDER,
      bottom: TABLE_BORDER,
      left: TABLE_BORDER,
      right: TABLE_BORDER,
    },
    children: [
      new Paragraph({
        children: [
          new TextRun({
            text: value == null ? "" : String(value),
            bold: isHeader,
            font: BODY_FONT,
            size: 20,
            color: "20242A",
          }),
        ],
      }),
    ],
  });
}

async function sectionChildren(section: DocumentSectionSpec): Promise<Array<Paragraph | Table>> {
  const children: Array<Paragraph | Table> = [];
  if (section.page_break) {
    children.push(new Paragraph({ children: [new PageBreak()] }));
  }
  if (section.heading) {
    const level = section.level === 1
      ? HeadingLevel.HEADING_1
      : section.level === 3
        ? HeadingLevel.HEADING_3
        : HeadingLevel.HEADING_2;
    children.push(new Paragraph({ text: section.heading, heading: level, spacing: { before: 220, after: 120 } }));
  }
  for (const paragraph of section.paragraphs ?? []) {
    children.push(textParagraph(paragraph));
  }
  for (const bullet of section.bullets ?? []) {
    children.push(new Paragraph({
      children: [new TextRun({ text: bullet, font: BODY_FONT, size: 22, color: "30343B" })],
      bullet: { level: 0 },
      spacing: { after: 100, line: 300 },
    }));
  }
  if (section.table) {
    const rows: TableRow[] = [];
    if (section.table.headers?.length) {
      rows.push(new TableRow({ children: section.table.headers.map((value) => tableCell(value, true)), tableHeader: true }));
    }
    for (const row of section.table.rows) {
      rows.push(new TableRow({ children: row.map((value) => tableCell(value, false)) }));
    }
    children.push(new Table({
      width: { size: 100, type: WidthType.PERCENTAGE },
      rows,
    }));
  }
  if (section.image_path) {
    const data = await readFile(section.image_path);
    const imageType = extname(section.image_path).toLowerCase().replace(".jpeg", ".jpg").slice(1);
    if (!(["jpg", "png", "gif", "bmp"] as const).includes(imageType as "jpg" | "png" | "gif" | "bmp")) {
      throw new Error(`document images must be JPG, PNG, GIF, or BMP: ${section.image_path}`);
    }
    children.push(new Paragraph({
      alignment: AlignmentType.CENTER,
      children: [new ImageRun({
        type: imageType as "jpg" | "png" | "gif" | "bmp",
        data,
        transformation: {
          width: section.image_width ?? 640,
          height: section.image_height ?? 360,
        },
      })],
      spacing: { before: 160, after: 160 },
    }));
  }
  return children;
}

export async function generateDocument(spec: DocumentSpec): Promise<GenerationResult> {
  if (!spec.path.toLowerCase().endsWith(".docx")) throw new Error("document path must end with .docx");
  if (!spec.title.trim()) throw new Error("document title is required");
  if (!Array.isArray(spec.sections)) throw new Error("document sections must be an array");

  const children: Array<Paragraph | Table> = [
    new Paragraph({
      alignment: AlignmentType.CENTER,
      children: [new TextRun({ text: spec.title, bold: true, font: BODY_FONT, size: 40, color: "172033" })],
      spacing: { before: 520, after: spec.subtitle ? 140 : 360 },
    }),
  ];
  if (spec.subtitle) {
    children.push(new Paragraph({
      alignment: AlignmentType.CENTER,
      children: [new TextRun({ text: spec.subtitle, font: BODY_FONT, size: 24, color: "667085" })],
      spacing: { after: 360 },
    }));
  }
  for (const section of spec.sections) {
    children.push(...await sectionChildren(section));
  }

  const document = new Document({
    creator: spec.author ?? "Harmony Work",
    title: spec.title,
    description: spec.subtitle,
    styles: {
      default: { document: { run: { font: BODY_FONT, size: 22 }, paragraph: { spacing: { line: 330 } } } },
      paragraphStyles: [
        { id: "Heading1", name: "Heading 1", basedOn: "Normal", next: "Normal", quickFormat: true, run: { size: 34, bold: true, color: "172033", font: BODY_FONT } },
        { id: "Heading2", name: "Heading 2", basedOn: "Normal", next: "Normal", quickFormat: true, run: { size: 28, bold: true, color: "1F4B7A", font: BODY_FONT } },
        { id: "Heading3", name: "Heading 3", basedOn: "Normal", next: "Normal", quickFormat: true, run: { size: 24, bold: true, color: "344054", font: BODY_FONT } },
      ],
    },
    sections: [{
      headers: spec.header_text
        ? { default: new Header({ children: [new Paragraph({ text: spec.header_text, alignment: AlignmentType.RIGHT })] }) }
        : undefined,
      footers: spec.footer_text
        ? { default: new Footer({ children: [new Paragraph({ text: spec.footer_text, alignment: AlignmentType.CENTER })] }) }
        : undefined,
      children,
    }],
  });

  await mkdir(dirname(spec.path), { recursive: true });
  await writeFile(spec.path, await Packer.toBuffer(document));
  const metadata = await stat(spec.path);
  return { path: spec.path, bytes: metadata.size, details: { sections: spec.sections.length } };
}
