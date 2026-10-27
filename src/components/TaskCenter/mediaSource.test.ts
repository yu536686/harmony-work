import { describe, expect, it } from "vitest";
import { resolveMediaSource } from "./mediaSource";

describe("TaskCenter safe existing media sources", () => {
  it("keeps signed HTTPS media URLs intact", () => {
    const url = "https://cdn.example/video.mp4?token=a%2Fb&expires=42";
    expect(resolveMediaSource(url, "video")).toEqual({ url, extension: "mp4" });
    expect(resolveMediaSource("https://cdn.example/speech.wav", "tts")?.extension).toBe("wav");
  });
  it("supports only matching media data URL MIME types", () => {
    expect(resolveMediaSource("data:video/mp4;base64,aGVsbG8=", "video")?.extension).toBe("mp4");
    expect(resolveMediaSource("data:audio/wav;base64,aGVsbG8=", "tts")?.extension).toBe("wav");
    expect(resolveMediaSource("data:audio/wav;base64,aGVsbG8=", "video")).toBeNull();
    expect(resolveMediaSource("data:video/mp4;base64,aGVsbG8=", "tts")).toBeNull();
  });
  it.each([
    "javascript:alert(1)", "file:///C:/secret.mp4", "http://cdn.example/a.mp4", "//cdn.example/a.mp4",
    "data:text/html;base64,PHNjcmlwdD4=", "data:image/svg+xml;base64,PHN2Zz4=", "data:constructor;base64,aGVsbG8=", "data:video/mp4;base64,invalid!", "data:video/mp4;base64,",
    "https://user:password@cdn.example/a.mp4", "blob:https://example.com/id", "https://cdn.example/a\n.mp4",
  ])("rejects unsafe or malformed sources: %s", (url) => {
    expect(resolveMediaSource(url, "video")).toBeNull();
  });
});
