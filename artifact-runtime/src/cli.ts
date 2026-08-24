import { readFile } from "node:fs/promises";
import { generateDocument } from "./document";
import { generatePresentation } from "./presentation";
import { generateSpreadsheet } from "./spreadsheet";
import type { DocumentSpec, GenerationResult, PresentationSpec, SpreadsheetSpec } from "./types";
import { verifyArtifact, type ArtifactKind } from "./verify";

export async function executeArtifact(kind: ArtifactKind, spec: unknown): Promise<GenerationResult> {
  let result: GenerationResult;
  if (kind === "document") result = await generateDocument(spec as DocumentSpec);
  else if (kind === "presentation") result = await generatePresentation(spec as PresentationSpec);
  else result = await generateSpreadsheet(spec as SpreadsheetSpec);
  const verification = await verifyArtifact(kind, result.path);
  return {
    ...result,
    bytes: verification.bytes,
    details: { ...result.details, packageEntries: verification.packageEntries },
  };
}

async function main(): Promise<void> {
  const kind = process.argv[2] as ArtifactKind | undefined;
  const specPath = process.argv[3];
  if (!kind || !["document", "presentation", "spreadsheet"].includes(kind)) {
    throw new Error("first argument must be document, presentation, or spreadsheet");
  }
  if (!specPath) throw new Error("second argument must be a JSON specification path");
  const spec = JSON.parse(await readFile(specPath, "utf8")) as unknown;
  const result = await executeArtifact(kind, spec);
  process.stdout.write(`${JSON.stringify({ ok: true, kind, ...result })}\n`);
}

if (require.main === module) {
  main().catch((error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`${message}\n`);
    process.exitCode = 1;
  });
}

