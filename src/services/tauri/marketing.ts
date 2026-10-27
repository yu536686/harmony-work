import { invoke } from "@tauri-apps/api/core";

export interface MarketingUser {
  id: string;
  name: string;
  email: string;
  userRole: "admin" | "user";
  accountType: "standard" | "marketing";
  approvalStatus: "pending" | "active" | "disabled";
}

export interface MarketingAccountOverview {
  user: MarketingUser;
  balance: number | null;
  usage: Record<string, number> | null;
  gatewayApiKey: string | null;
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

export interface AccountSettingsSyncFieldSummary {
  label: string;
  configured: boolean;
  sensitive: boolean;
}

export interface AccountSettingsSyncCategory {
  id: string;
  label: string;
  description: string;
  fields: AccountSettingsSyncFieldSummary[];
  configuredFields: string[];
}

export interface AccountSettingsSyncCatalog {
  categories: AccountSettingsSyncCategory[];
  localOnlyItems: string[];
  snapshot: AccountSettingsSyncStatus["snapshot"];
}

export interface AccountSettingsSyncPreview {
  snapshot: NonNullable<AccountSettingsSyncStatus["snapshot"]>;
  categories: AccountSettingsSyncCategory[];
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
  contentClass?: "business" | "source" | "auxiliary";
  isAuxiliary?: boolean;
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
  contentClass?: "business" | "source" | "auxiliary";
  includeAuxiliary?: boolean;
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
let sessionGeneration = 0;
let accountGeneration = 0;
const SESSION_CHANGED = "workbuddy:marketing-session-changed";

export interface MarketingSessionSnapshot {
  readonly userId: string | null;
  readonly generation: number;
}

function invalidateAccountSession() {
  accountGeneration++;
  window.dispatchEvent(new Event(SESSION_CHANGED));
}

export function captureMarketingSession(): MarketingSessionSnapshot {
  const id = readRememberedMarketingUser()?.id;
  return Object.freeze({ userId: typeof id === "string" && id.trim() ? id : null, generation: accountGeneration });
}

export function isCurrentMarketingSession(session: MarketingSessionSnapshot): boolean {
  const current = captureMarketingSession();
  return session.generation === current.generation && session.userId === current.userId;
}

export function subscribeMarketingSession(onChange: () => void): () => void {
  window.addEventListener(SESSION_CHANGED, onChange);
  return () => window.removeEventListener(SESSION_CHANGED, onChange);
}

window.addEventListener("storage", event => {
  if (event.key === USER_STORAGE_KEY || event.key === null) {
    sessionGeneration++;
    invalidateAccountSession();
  }
});

function ensureCurrentSession(generation: number) {
  if (generation !== sessionGeneration) throw new Error("账户会话已变更，请重新登录。");
}

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
  const previous = readRememberedMarketingUser()?.id ?? null;
  if (user) localStorage.setItem(USER_STORAGE_KEY, JSON.stringify(user));
  else localStorage.removeItem(USER_STORAGE_KEY);
  if (previous !== (user?.id ?? null)) invalidateAccountSession();
}

export function forgetRememberedMarketingUser() {
  sessionGeneration++;
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
    const generation = ++sessionGeneration;
    rememberUser(null);
    invalidateAccountSession();
    const result = await invoke<{ user: MarketingUser }>("marketing_login", { request: { email, password } });
    ensureCurrentSession(generation);
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
      ensureCurrentSession(generation);
      window.dispatchEvent(new CustomEvent("workbuddy:account-settings-sync-prepared", { detail: prompt }));
    } catch (error) {
      // Settings sync is optional. Authentication remains available if a legacy
      // cloud vault cannot be unlocked on this device.
      console.warn("Unable to prepare account settings sync", error);
    }
    ensureCurrentSession(generation);
    return result.user;
  },
  logout: async () => {
    sessionGeneration++;
    rememberUser(null);
    invalidateAccountSession();
    try {
      await invoke("marketing_logout");
    } catch (error) {
      // Local logout must complete even if the remote service is offline.
      console.warn("Marketing logout command unavailable; remembered user cleared", error);
    }
  },
  me: async () => {
    const generation = sessionGeneration;
    const result = await invoke<{ user: MarketingUser }>("marketing_me");
    ensureCurrentSession(generation);
    rememberUser(result.user);
    return result.user;
  },
  accountOverview: () => invoke<MarketingAccountOverview>("marketing_account_overview"),
  summary: async () => normalizeSummary(await invoke<MarketingAssetSummary>("marketing_asset_summary")),
  folders: () => invoke<{ folders: string[] }>("marketing_asset_folders"),
  formats: () => invoke<{ existing: Array<{ extension: string; count: number }>; common: string[] }>("marketing_asset_formats"),
  assets: async (query: MarketingAssetQuery) => normalizeAssetList(await invoke<MarketingAssetList>("marketing_assets", { query })),
  uploadAssets: (filePaths: string[], folder?: string) =>
    invoke<{ assets: MarketingAsset[] }>("marketing_upload_assets", { filePaths, folder: folder || null }),
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
  catalog: () => invoke<AccountSettingsSyncCatalog>("account_settings_sync_catalog"),
  preview: (userId: string) => invoke<AccountSettingsSyncPreview | null>("account_settings_sync_preview", { request: { userId } }),
  backup: (userId: string) => invoke<AccountSettingsSyncStatus>("account_settings_sync_backup", { request: { userId } }),
  backupSelected: (userId: string, categoryIds: string[]) =>
    invoke<AccountSettingsSyncStatus>("account_settings_sync_backup_selected", { request: { userId, categoryIds } }),
  restore: (userId: string) => invoke<AccountSettingsSyncStatus>("account_settings_sync_restore", { request: { userId } }),
  restoreSelected: (userId: string, categoryIds: string[]) =>
    invoke<AccountSettingsSyncStatus>("account_settings_sync_restore_selected", { request: { userId, categoryIds } }),
  delete: (userId: string) => invoke("account_settings_sync_delete", { request: { userId } }),
};
