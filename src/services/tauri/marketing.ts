import { invoke } from "@tauri-apps/api/core";

export interface MarketingUser {
  id: string;
  name: string;
  email: string;
  userRole: "admin" | "user";
  approvalStatus: "pending" | "active" | "disabled";
}

export interface AccountSettingsSyncPrepared {
  snapshotExists: boolean;
  revision: number | null;
  suggestedAction: "restore" | "backup";
}

export interface AccountSettingsSyncPrompt extends AccountSettingsSyncPrepared {
  userId: string;
}

export interface AccountSettingsSyncStatus {
  snapshot: { revision: number; schemaVersion: number; updatedAt: string; updatedDeviceName: string } | null;
  locallyConfigured: boolean;
}

export interface MarketingAsset {
  id: string;
  fileName: string;
  rootFolder: string;
  relativePath: string;
  extension: string;
  assetType: "video" | "image" | "audio" | "document" | "archive" | "other";
  mimeType: string;
  fileSize: number;
  modifiedAt: string;
  width?: number | null;
  height?: number | null;
  durationSeconds?: number | null;
  previewUrl?: string;
  thumbnailUrl?: string;
  coverUrl?: string;
  posterUrl?: string;
}

export interface MarketingAssetSummary {
  totals: Record<string, string | number>;
  folders: Array<{ rootFolder: string; count: number; totalSize: number; latestModifiedAt: string | null }>;
  recent: MarketingAsset[];
  scan: { running: boolean; scanned: number; finishedAt: string | null; errors?: string[] };
}

export interface MarketingAssetList {
  total: number;
  assets: MarketingAsset[];
}

export interface MarketingAssetQuery {
  folder?: string;
  assetType?: string;
  extension?: string;
  formatGroup?: string;
  q?: string;
  sort?: string;
  limit?: number;
  offset?: number;
}

export interface MarketingCampaignSummary {
  intervalMinutes: number;
  windowMinutes: number;
  platforms: Array<Record<string, string | number>>;
  accounts: Array<Record<string, string | number | null>>;
  posts: Array<Record<string, string | number | null>>;
  trend: Array<Record<string, string | number>>;
}

export interface MarketingPreparedAsset {
  assetId: string;
  fileName: string;
  mediaType: string;
  localPath: string;
  sourceUrl: string;
  byteSize: number;
}

export interface MarketingAssetDataUrl {
  assetId: string;
  fileName: string;
  mediaType: string;
  dataUrl: string;
  byteSize: number;
}

const USER_STORAGE_KEY = "workbuddy.marketingUser";

const IMAGE_EXTS = new Set(["jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "svg"]);
const VIDEO_EXTS = new Set(["mp4", "mov", "mkv", "avi", "webm", "m4v"]);
const AUDIO_EXTS = new Set(["mp3", "wav", "m4a", "aac", "flac", "ogg"]);
const DOC_EXTS = new Set(["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "md", "csv"]);
const ARCHIVE_EXTS = new Set(["zip", "rar", "7z", "tar", "gz"]);

function value<T = unknown>(source: Record<string, unknown>, camel: string, snake: string): T | undefined {
  return (source[camel] ?? source[snake]) as T | undefined;
}

function inferAssetType(extension: string, rawType?: string): MarketingAsset["assetType"] {
  const type = String(rawType || "").toLowerCase();
  if (type === "image" || type === "video" || type === "audio" || type === "document" || type === "archive" || type === "other") {
    if (type === "video" && !VIDEO_EXTS.has(extension)) {
      return inferAssetType(extension);
    }
    return type as MarketingAsset["assetType"];
  }
  if (IMAGE_EXTS.has(extension)) return "image";
  if (VIDEO_EXTS.has(extension)) return "video";
  if (AUDIO_EXTS.has(extension)) return "audio";
  if (DOC_EXTS.has(extension)) return "document";
  if (ARCHIVE_EXTS.has(extension)) return "archive";
  return "other";
}

function normalizeAsset(raw: MarketingAsset | Record<string, unknown>): MarketingAsset {
  const source = raw as Record<string, unknown>;
  const extension = String(value(source, "extension", "extension") ?? "").replace(/^\./, "").toLowerCase();
  const fileName = String(value(source, "fileName", "file_name") ?? value(source, "name", "name") ?? "");
  return {
    id: String(value(source, "id", "id") ?? ""),
    fileName,
    rootFolder: String(value(source, "rootFolder", "root_folder") ?? ""),
    relativePath: String(value(source, "relativePath", "relative_path") ?? ""),
    extension,
    assetType: inferAssetType(extension, String(value(source, "assetType", "asset_type") ?? "")),
    mimeType: String(value(source, "mimeType", "mime_type") ?? ""),
    fileSize: Number(value(source, "fileSize", "file_size") ?? 0),
    modifiedAt: String(value(source, "modifiedAt", "modified_at") ?? ""),
    width: value<number | null>(source, "width", "width") ?? null,
    height: value<number | null>(source, "height", "height") ?? null,
    durationSeconds: value<number | null>(source, "durationSeconds", "duration_seconds") ?? null,
    previewUrl: value<string | undefined>(source, "previewUrl", "preview_url")
      ?? value<string | undefined>(source, "thumbnailUrl", "thumbnail_url")
      ?? value<string | undefined>(source, "coverUrl", "cover_url")
      ?? value<string | undefined>(source, "posterUrl", "poster_url"),
    thumbnailUrl: value<string | undefined>(source, "thumbnailUrl", "thumbnail_url"),
    coverUrl: value<string | undefined>(source, "coverUrl", "cover_url"),
    posterUrl: value<string | undefined>(source, "posterUrl", "poster_url"),
  };
}

function normalizeAssetList(raw: MarketingAssetList | Record<string, unknown>): MarketingAssetList {
  const source = raw as Record<string, unknown>;
  const rawAssets = (source.assets ?? []) as Array<MarketingAsset | Record<string, unknown>>;
  return {
    total: Number(source.total ?? rawAssets.length ?? 0),
    assets: rawAssets.map(normalizeAsset),
  };
}

function normalizeSummary(raw: MarketingAssetSummary): MarketingAssetSummary {
  return {
    ...raw,
    recent: (raw.recent ?? []).map(normalizeAsset),
  };
}

function rememberUser(user: MarketingUser | null) {
  if (user) localStorage.setItem(USER_STORAGE_KEY, JSON.stringify(user));
  else localStorage.removeItem(USER_STORAGE_KEY);
}

export function forgetRememberedMarketingUser() {
  rememberUser(null);
}

export function readRememberedMarketingUser(): MarketingUser | null {
  try {
    const raw = localStorage.getItem(USER_STORAGE_KEY);
    return raw ? JSON.parse(raw) as MarketingUser : null;
  } catch {
    return null;
  }
}

export function marketingUserContext(user: MarketingUser | null = readRememberedMarketingUser()): string | undefined {
  if (!user) return undefined;
  return [
    `登录用户：${user.name}`,
    `邮箱：${user.email}`,
    `角色：${user.userRole}`,
    `账号状态：${user.approvalStatus}`,
  ].join("；");
}

export const marketingApi = {
  login: async (email: string, password: string) => {
    const result = await invoke<{ user: MarketingUser }>("marketing_login", { request: { email, password } });
    rememberUser(result.user);
    try {
      const prepared = await invoke<AccountSettingsSyncPrepared>("account_settings_sync_prepare", {
        request: {
          userId: result.user.id,
          password,
          deviceName: navigator.userAgent.includes("Windows") ? "Windows WorkBuddy" : "WorkBuddy desktop",
        },
      });
      const prompt: AccountSettingsSyncPrompt = { ...prepared, userId: result.user.id };
      window.dispatchEvent(new CustomEvent("workbuddy:account-settings-sync-prepared", { detail: prompt }));
    } catch (error) {
      // Settings sync is optional. Authentication remains available if a legacy
      // cloud vault cannot be unlocked on this device.
      console.warn("Unable to prepare account settings sync", error);
    }
    return result.user;
  },
  logout: async () => {
    await invoke("marketing_logout");
    rememberUser(null);
  },
  me: async () => {
    const result = await invoke<{ user: MarketingUser }>("marketing_me");
    rememberUser(result.user);
    return result.user;
  },
  summary: async () => normalizeSummary(await invoke<MarketingAssetSummary>("marketing_asset_summary")),
  folders: () => invoke<{ folders: string[] }>("marketing_asset_folders"),
  formats: () => invoke<{ existing: Array<{ extension: string; count: number }>; common: string[] }>("marketing_asset_formats"),
  assets: async (query: MarketingAssetQuery) => normalizeAssetList(await invoke<MarketingAssetList>("marketing_assets", { query })),
  campaignSummary: (minutes = 1440, platform = "") =>
    invoke<MarketingCampaignSummary>("marketing_campaign_summary", { minutes, platform: platform || null }),
  douyinAccounts: () => invoke<{ accounts: Array<Record<string, string | number | boolean | null>> }>("marketing_douyin_accounts"),
  prepareAssetForChat: (assetId: string, fileName: string, workspaceRoot?: string | null) =>
    invoke<MarketingPreparedAsset>("marketing_prepare_asset_for_chat", {
      request: { assetId, fileName, workspaceRoot: workspaceRoot || null },
    }),
  assetDataUrl: (assetId: string, fileName: string, maxBytes?: number) =>
    invoke<MarketingAssetDataUrl>("marketing_asset_data_url", {
      request: { assetId, fileName, maxBytes: maxBytes || null },
    }),
};

export const accountSettingsSyncApi = {
  status: () => invoke<AccountSettingsSyncStatus>("account_settings_sync_status"),
  backup: (userId: string) => invoke<AccountSettingsSyncStatus>("account_settings_sync_backup", { request: { userId } }),
  restore: (userId: string) => invoke<AccountSettingsSyncStatus>("account_settings_sync_restore", { request: { userId } }),
  delete: (userId: string) => invoke("account_settings_sync_delete", { request: { userId } }),
};
