export interface PersistedAttachment { path: string; name: string; mediaType: string }

function attachmentPath(value: string): string | null {
  if (/[\x00-\x1f]/.test(value)) return null;
  let path = value;
  if (/^\\\\\?\\UNC\\/i.test(path)) path = `\\\\${path.slice(8)}`;
  else if (/^\\\\\?\\[A-Za-z]:\\/.test(path)) path = path.slice(4);
  const drive = /^[A-Za-z]:[\\/]/.test(path);
  const unc = path.startsWith("\\\\");
  if (drive || unc) {
    const rest = drive ? path.slice(3) : path.slice(2);
    if (/[<>:"|?*]/.test(rest)) return null;
    const parts = rest.split(/[\\/]/);
    if (unc && parts.length < 3) return null;
    // Ordinary files only: no device namespace, DOS aliases, streams or traversal.
    if (parts.some(part => !part || part === "." || part === ".." || /[ .]$/.test(part)
      || /^(con|prn|aux|nul|clock\$|conin\$|conout\$|com[1-9\u00b9\u00b2\u00b3]|lpt[1-9\u00b9\u00b2\u00b3])(?:\.|$)/i.test(part.split(".")[0].trim()))) return null;
    return path;
  }
  return /^\/(?!\/)/.test(path) ? path : null;
}

export function parsePersistedAttachments(raw?: string | null): PersistedAttachment[] {
  if (!raw) return [];
  try {
    const value: unknown = JSON.parse(raw);
    if (!value || typeof value !== "object" || Array.isArray(value)) return [];
    const items = (value as Record<string, unknown>).attachments;
    if (!Array.isArray(items)) return [];
    return items.slice(0, 100).flatMap(item => {
      if (!item || typeof item !== "object" || typeof item.path !== "string") return [];
      const path = attachmentPath(item.path);
      if (!path) return [];
      const name = typeof item.name === "string" && item.name.trim() ? item.name : path.split(/[\\/]/).pop() || "附件";
      return [{ path, name, mediaType: typeof item.mediaType === "string" ? item.mediaType : "application/octet-stream" }];
    });
  } catch { return []; }
}
