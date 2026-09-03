import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import JSZip from "jszip";
import { expect, test } from "vitest";
import { verifyArtifact } from "./verify";

test("rejects an OOXML package missing required document entries", async () => {
  const directory = await mkdtemp(join(tmpdir(), "workbuddy-invalid-ooxml-"));
  const path = join(directory, "invalid.docx");
  const zip = new JSZip();
  zip.file("[Content_Types].xml", "<Types />");
  zip.file("padding.bin", Buffer.alloc(2_048, 7));
  await writeFile(path, await zip.generateAsync({ type: "nodebuffer", compression: "STORE" }));

  await expect(verifyArtifact("document", path)).rejects.toThrow("word/document.xml");
});
