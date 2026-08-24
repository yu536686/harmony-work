import { readFile } from "node:fs/promises";
import JSZip from "jszip";

export async function packageEntries(path: string): Promise<string[]> {
  const buffer = await readFile(path);
  const archive = await JSZip.loadAsync(buffer);
  return Object.keys(archive.files);
}

