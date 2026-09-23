import { convertFileSrc } from "@tauri-apps/api/core";

const SAFE_DATA_IMAGE = /^data:image\/(?:png|jpeg|webp|gif|avif);base64,[A-Za-z0-9+/=]+$/i;
const WINDOWS_PATH = /^(?:[A-Za-z]:[\\/]|\\\\)/;
const UNIX_FILE_PATH = /^\/(?:Users|home|tmp|var|mnt|Volumes|opt)\//;

function localImagePath(source: string): string | null {
  if (WINDOWS_PATH.test(source)) return source.replace(/\//g, "\\");
  if (UNIX_FILE_PATH.test(source)) return source;
  if (!/^file:\/\//i.test(source)) return null;
  try {
    const url = new URL(source);
    if (url.protocol !== "file:") return null;
    const path = decodeURIComponent(url.pathname);
    if (url.hostname) return `\\\\${url.hostname}${path.replace(/\//g, "\\")}`;
    return /^\/[A-Za-z]:\//.test(path) ? path.slice(1).replace(/\//g, "\\") : path;
  } catch {
    return null;
  }
}

export function safeMessageUrl(url: string): string {
  if (/^(?:https?:\/\/|mailto:|#|\/)/i.test(url) || !url.includes(":")) return url;
  if (WINDOWS_PATH.test(url) || /^file:\/\//i.test(url) || SAFE_DATA_IMAGE.test(url)) return url;
  return "";
}

export function messageImageSrc(source: string | undefined): string {
  if (!source) return "";
  const path = localImagePath(source);
  if (path) return convertFileSrc(path);
  if (/^https?:\/\//i.test(source) || SAFE_DATA_IMAGE.test(source)) return source;
  return "";
}
