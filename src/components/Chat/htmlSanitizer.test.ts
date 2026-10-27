// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { sanitizePreviewHtml } from "./htmlSanitizer";

const base = "http://harmony-preview.localhost/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/deck/";
const parse = (html: string) => new DOMParser().parseFromString(html, "text/html");

describe("static HTML preview sanitizer (real DOMPurify and DOMParser)", () => {
  it("removes executable, navigating, embedded, and event-bearing markup", () => {
    const doc = parse(sanitizePreviewHtml(`<!doctype html><html><head>
      <meta http-equiv="refresh" content="0;url=https://example.com/leak">
      <base href="https://example.com/"><script>location='https://example.com'</script>
      </head><body onload="fetch('https://example.com')"><h1 onclick="alert(1)">Deck</h1>
      <form action="https://example.com"><input name="secret"><button formaction="https://example.com">Send</button></form>
      <a href="https://example.com" ping="https://example.com/ping" target="_top">Visible link</a>
      <iframe srcdoc="<script>alert(1)</script>"></iframe><object data="a.html"></object><embed src="b.html">
      <svg><a href="https://example.com"><text>svg link</text></a><animate attributeName="href" to="https://example.com" /></svg>
      <link rel="prefetch" href="https://example.com"><link rel="stylesheet" href="style.css">
      </body></html>`, base));
    expect(doc.querySelector("script, form, input, button, iframe, object, embed, a, svg, meta[http-equiv=refresh]" )).toBeNull();
    expect(doc.querySelectorAll("base")).toHaveLength(1);
    expect(doc.querySelector("base")?.getAttribute("href")).toBe(base);
    expect(doc.body.textContent).toContain("Visible link");
    expect(doc.body.textContent).toContain("Deck");
    for (const element of doc.querySelectorAll("*")) {
      expect([...element.attributes].some(attr => attr.name.startsWith("on"))).toBe(false);
    }
    expect(doc.querySelector("link[rel=prefetch]")).toBeNull();
    expect(doc.querySelector("link[rel=stylesheet]")?.getAttribute("href")).toBe(`${base}style.css`);
  });

  it("allows grant-local styles/images/media but never scripts, connections or another grant", () => {
    const doc = parse(sanitizePreviewHtml(`<style>body{color:red}</style><img src="../assets/photo.png">
      <img src="https://example.com/image" srcset="https://example.com/2x 2x">
      <video src="clip.mp4" poster="poster.png" controls></video><audio src="audio.mp3"></audio>
      <link rel="stylesheet" href="https://example.com/a.css"><img src="file:///private.txt">
      <img src="http://harmony-preview.localhost/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/secret.png">`, base));
    const policy = doc.querySelector('meta[http-equiv="Content-Security-Policy"]')?.getAttribute("content") ?? "";
    expect(policy).toContain("script-src 'none'");
    expect(policy).toContain("connect-src 'none'");
    expect(policy).toContain("default-src 'none'");
    expect(policy).toContain("http://harmony-preview.localhost/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/");
    expect(policy).not.toContain("https:");
    expect(policy).not.toContain("'self'");
    expect(doc.querySelector("img")?.getAttribute("src")).toBe("http://harmony-preview.localhost/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/assets/photo.png");
    expect(doc.querySelector("video")?.getAttribute("src")).toBe(`${base}clip.mp4`);
    expect(doc.querySelectorAll("img[src]")).toHaveLength(1);
    expect(doc.querySelectorAll("[srcset]")).toHaveLength(0);
    expect(doc.querySelector("style")?.textContent).toContain("color:red");
    expect(doc.querySelector("link")).toBeNull();
    expect(doc.head.firstElementChild?.getAttribute("http-equiv")).toBe("Content-Security-Policy");
  });

  it("keeps macOS custom-scheme local assets and rejects an untrusted base", () => {
    const macBase = base.replace("http://harmony-preview.localhost", "harmony-preview://localhost");
    const doc = parse(sanitizePreviewHtml('<img src="photo.png">', macBase));
    expect(doc.querySelector("img")?.getAttribute("src")).toBe(`${macBase}photo.png`);
    expect(() => sanitizePreviewHtml("<h1>Deck</h1>", "https://example.com/")).toThrow();
    expect(() => sanitizePreviewHtml("<h1>Deck</h1>", `${base}?override=1`)).toThrow();
  });

  it("does not reconstruct a navigation through malformed HTML or XML attributes", () => {
    const doc = parse(sanitizePreviewHtml(`<math><mtext><table><mglyph><style><!--</style><img title="--><img src=x onerror=alert(1)>">
      <a href="&#106;avascript:alert(1)">text</a><img src="//example.com/x"><input type=image src="https://example.com">
      <div href="https://example.com" xlink:href="https://example.com" contenteditable="true" autofocus>ok</div>`, base));
    expect(doc.querySelector("script, math, svg, a, input")).toBeNull();
    expect(doc.querySelector("[onerror], [href]:not(base):not(link), [xlink\\:href], [autofocus], [contenteditable]")).toBeNull();
  });
});
