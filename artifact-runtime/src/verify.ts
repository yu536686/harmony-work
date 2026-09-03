import { readFile, stat } from "node:fs/promises";
import JSZip from "jszip";

export type ArtifactKind = "document" | "presentation" | "spreadsheet";

const REQUIRED_ENTRIES: Record<ArtifactKind, string[]> = {
  document: ["[Content_Types].xml", "word/document.xml"],
  presentation: ["[Content_Types].xml", "ppt/presentation.xml", "ppt/slides/slide1.xml"],
  spreadsheet: ["[Content_Types].xml", "xl/workbook.xml", "xl/worksheets/sheet1.xml"],
};

export interface VerificationResult {
  packageEntries: number;
  bytes: number;
}

export async function verifyArtifact(kind: ArtifactKind, path: string): Promise<VerificationResult> {
  const metadata = await stat(path);
  if (!metadata.isFile() || metadata.size < 512) {
    throw new Error(`artifact output is missing or too small: ${path}`);
  }
  const archive = await JSZip.loadAsync(await readFile(path));
  const entries = Object.keys(archive.files);
  for (const required of REQUIRED_ENTRIES[kind]) {
    if (!archive.file(required)) throw new Error(`artifact package is missing required entry: ${required}`);
  }
  return { packageEntries: entries.length, bytes: metadata.size };
}

