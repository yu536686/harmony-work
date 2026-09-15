export const HARMONY_API_BASE = "https://api.harmonywatch.cn/v1";
export const MARKETING_BASE = "https://portal.harmonywatch.cn";
export const MARKET_BASE = "https://market.harmonywatch.cn";
export const REGISTER_URL = MARKETING_BASE;

// Only known public origins are migrated; explicit LAN/custom endpoints stay intact.
export function migratePublicServiceUrl(raw: string): string {
  let url: URL;
  try { url = new URL(raw); } catch { return raw; }
  if (!["http:", "https:"].includes(url.protocol) || url.username || url.password) return raw;
  const host = url.hostname;
  const port = url.port;
  let target = "";
  if (host === "api.harmonywatch.cn" && ["", "13001"].includes(port)) target = "api.harmonywatch.cn";
  if (["www.harmonywatch.cn", "portal.harmonywatch.cn", "61.150.31.150"].includes(host) && port === "8090") target = "portal.harmonywatch.cn";
  if (host === "portal.harmonywatch.cn" && !port) target = host;
  if (["www.harmonywatch.cn", "market.harmonywatch.cn", "61.150.31.150"].includes(host) && port === "8787") target = "market.harmonywatch.cn";
  if (host === "market.harmonywatch.cn" && !port) target = host;
  if (!target || (url.protocol === "https:" && host === target && !port)) return raw;
  url.protocol = "https:";
  url.hostname = target;
  url.port = "";
  return `${url.origin}${url.pathname === "/" && !raw.endsWith("/") && !url.search && !url.hash ? "" : url.pathname}${url.search}${url.hash}`;
}
