export type LocalMediaKind = "video" | "tts";

const videoMimes = new Map(Object.entries({ "video/mp4": "mp4", "video/webm": "webm", "video/ogg": "ogv" }));
const audioMimes = new Map(Object.entries({ "audio/mpeg": "mp3", "audio/mp3": "mp3", "audio/mp4": "m4a", "audio/x-m4a": "m4a", "audio/wav": "wav", "audio/x-wav": "wav", "audio/ogg": "ogg", "audio/webm": "webm", "audio/aac": "aac", "audio/flac": "flac" }));

export function resolveMediaSource(raw: string | undefined, kind: LocalMediaKind): { url: string; extension: string } | null {
  if (!raw || /[\u0000-\u001f\u007f]/.test(raw)) return null;
  const source = raw.trim();
  if (source.startsWith("data:")) {
    if (source.length > 64 * 1024 * 1024) return null;
    const match = /^data:([a-z0-9/+.-]+);base64,([A-Za-z0-9+/]+={0,2})$/i.exec(source);
    if (!match || match[2].length % 4 !== 0) return null;
    const extension = (kind === "video" ? videoMimes : audioMimes).get(match[1].toLowerCase());
    return extension ? { url: source, extension } : null;
  }
  try {
    const url = new URL(source);
    if (url.protocol !== "https:" || !url.hostname || url.username || url.password) return null;
    const candidate = url.pathname.split(".").pop()?.toLowerCase() ?? "";
    const extensions = kind === "video" ? ["mp4", "webm", "ogv", "mov", "m4v"] : ["mp3", "m4a", "wav", "ogg", "webm", "aac", "flac"];
    return { url: source, extension: extensions.includes(candidate) ? candidate : kind === "video" ? "mp4" : "mp3" };
  } catch {
    return null;
  }
}
