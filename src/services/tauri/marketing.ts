import { invoke } from "@tauri-apps/api/core";

export interface MarketingUser {
  id: string;
  name: string;
  email: string;
  userRole: "admin" | "user";
  approvalStatus: "pending" | "active" | "disabled";
}

export interface MarketingAsset {
  id: string;
  fileName: string;
  rootFolder: string;
  relativePath: string;
  extension: string;
  assetType: "video" | "image" | "document" | "archive" | "other";
  mimeType: string;
  fileSize: number;
  modifiedAt: string;
  width?: number | null;
  height?: number | null;
  durationSeconds?: number | null;
  previewUrl?: string;
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

const USER_STORAGE_KEY = "workbuddy.marketingUser";

function rememberUser(user: MarketingUser | null) {
  if (user) localStorage.setItem(USER_STORAGE_KEY, JSON.stringify(user));
  else localStorage.removeItem(USER_STORAGE_KEY);
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
  summary: () => invoke<MarketingAssetSummary>("marketing_asset_summary"),
  folders: () => invoke<{ folders: string[] }>("marketing_asset_folders"),
  formats: () => invoke<{ existing: Array<{ extension: string; count: number }>; common: string[] }>("marketing_asset_formats"),
  assets: (query: MarketingAssetQuery) => invoke<MarketingAssetList>("marketing_assets", { query }),
  campaignSummary: (minutes = 1440, platform = "") =>
    invoke<MarketingCampaignSummary>("marketing_campaign_summary", { minutes, platform: platform || null }),
  douyinAccounts: () => invoke<{ accounts: Array<Record<string, string | number | boolean | null>> }>("marketing_douyin_accounts"),
  prepareAssetForChat: (assetId: string, fileName: string, workspaceRoot?: string | null) =>
    invoke<MarketingPreparedAsset>("marketing_prepare_asset_for_chat", {
      request: { assetId, fileName, workspaceRoot: workspaceRoot || null },
    }),
};
