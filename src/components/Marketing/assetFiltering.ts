export type AssetContentClass = "business" | "source" | "auxiliary";

const BUSINESS_EXTENSIONS = new Set([
  "jpg",
  "jpeg",
  "png",
  "webp",
  "gif",
  "tiff",
  "svg",
  "mp4",
  "mov",
  "m4v",
  "mkv",
  "webm",
  "avi",
  "mp3",
  "wav",
  "m4a",
  "aac",
  "flac",
  "ogg",
  "pdf",
  "docx",
  "xlsx",
  "pptx",
  "txt",
  "md",
  "csv",
]);

const SOURCE_EXTENSIONS = new Set(["psd", "ai", "aep", "prproj", "cdr", "fig"]);

export function isFallbackBusinessExtension(extension: string): boolean {
  const ext = extension.trim().toLowerCase().replace(/^\.+/, "");
  return BUSINESS_EXTENSIONS.has(ext) || SOURCE_EXTENSIONS.has(ext);
}

export function shouldShowAsset(
  asset: { isAuxiliary?: boolean; contentClass?: string; extension: string },
  includeAuxiliary: boolean
): boolean {
  if (includeAuxiliary) return true;
  if (asset.isAuxiliary === true || asset.contentClass === "auxiliary") {
    return false;
  }
  // 旧门户未提供分类字段时，回退到安全白名单，避免 nfo/字幕等辅助文件混入。
  if (asset.contentClass === undefined && asset.isAuxiliary === undefined) {
    return isFallbackBusinessExtension(asset.extension);
  }
  return true;
}

export function resolveVisibleSelection<T extends { id: string }>(
  selectedId: string | null,
  visible: readonly T[]
): string | null {
  return selectedId && visible.some((item) => item.id === selectedId) ? selectedId : null;
}
