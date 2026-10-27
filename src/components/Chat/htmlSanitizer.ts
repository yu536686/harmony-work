import DOMPurify from "dompurify";

const blockedTags = [
  "script", "noscript", "meta", "base", "iframe", "frame", "frameset", "object", "embed",
  "form", "input", "button", "select", "textarea", "option", "a", "area", "svg", "math",
];
const blockedAttributes = [
  "srcdoc", "srcset", "imagesrcset", "action", "formaction", "form", "target", "ping",
  "download", "autofocus", "contenteditable", "xlink:href", "manifest", "background",
];

export function sanitizePreviewHtml(html: string, baseUrl: string): string {
  const base = new URL(baseUrl);
  const token = base.pathname.split("/")[1];
  const localOrigin = (base.protocol === "http:" && base.host === "harmony-preview.localhost")
    || (base.protocol === "harmony-preview:" && base.host === "localhost");
  if (!localOrigin || base.username || base.password || base.search || base.hash
    || !base.pathname.endsWith("/") || !/^[a-f0-9]{32}$/.test(token)) {
    throw new Error("Invalid preview resource base");
  }
  if (!DOMPurify.isSupported) throw new Error("HTML sanitization unavailable");
  const grantRoot = `${base.protocol}//${base.host}/${token}/`;
  const clean = DOMPurify.sanitize(html, {
    WHOLE_DOCUMENT: true,
    USE_PROFILES: { html: true },
    ADD_TAGS: ["link"],
    ADD_ATTR: ["rel"],
    FORBID_TAGS: blockedTags,
    FORBID_ATTR: blockedAttributes,
    ALLOW_DATA_ATTR: false,
    ALLOW_UNKNOWN_PROTOCOLS: true,
  });
  const doc = new DOMParser().parseFromString(clean, "text/html");
  // Resolve resources only against the backend's fixed grant, never artifact markup.
  const resource = (value: string, embeddedImage: boolean): string | null => {
    if (embeddedImage && /^data:image\/(?:png|jpeg|gif|webp|avif|bmp|svg\+xml|x-icon)[;,]/i.test(value)) return value;
    try {
      const url = new URL(value, base);
      return url.href.startsWith(grantRoot) && !url.username && !url.password ? url.href : null;
    } catch {
      return null;
    }
  };
  for (const element of doc.querySelectorAll("*")) {
    if (element.localName === "link" && element.getAttribute("rel")?.toLowerCase() !== "stylesheet") {
      element.remove();
      continue;
    }
    for (const attribute of [...element.attributes]) {
      const name = attribute.name.toLowerCase();
      if (name.startsWith("on") || blockedAttributes.includes(name)) {
        element.removeAttribute(attribute.name);
      } else if (name === "href" || name === "src" || name === "poster") {
        const allowed = (name === "href" && element.localName === "link")
          || (name === "src" && ["img", "audio", "video", "source", "track"].includes(element.localName))
          || (name === "poster" && element.localName === "video");
        const url = allowed ? resource(attribute.value, element.localName === "img" || name === "poster") : null;
        if (url) element.setAttribute(name, url);
        else element.removeAttribute(attribute.name);
      }
    }
    if (element.localName === "link" && !element.hasAttribute("href")) element.remove();
  }
  const policy = doc.createElement("meta");
  policy.httpEquiv = "Content-Security-Policy";
  policy.content = [
    "default-src 'none'", "script-src 'none'", `style-src ${grantRoot} 'unsafe-inline'`,
    `img-src ${grantRoot} data:`, `font-src ${grantRoot} data:`, `media-src ${grantRoot} data:`,
    "connect-src 'none'", "frame-src 'none'", "object-src 'none'", "worker-src 'none'",
    `base-uri ${base.href}`, "form-action 'none'",
  ].join("; ");
  const fixedBase = doc.createElement("base");
  fixedBase.href = base.href;
  const referrer = doc.createElement("meta");
  referrer.name = "referrer";
  referrer.content = "no-referrer";
  doc.head.prepend(policy, fixedBase, referrer);
  return `<!doctype html>\n${doc.documentElement.outerHTML}`;
}
