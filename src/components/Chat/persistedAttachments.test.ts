import { describe, expect, it } from "vitest";
import { parsePersistedAttachments } from "./persistedAttachments";

describe("persisted chat attachments", () => {
  it("normalizes canonical Windows drive and UNC paths from the runtime without accepting devices", () => {
    const paths = [String.raw`\\?\C:\work\photo.png`, String.raw`\\?\UNC\nas\share\photo.png`, String.raw`\\nas\share\report.pdf`];
    const attachments = paths.map(path => ({ path, name: "attachment", mediaType: "image/png" }));
    expect(parsePersistedAttachments(JSON.stringify({ attachments }))).toEqual([
      { ...attachments[0], path: String.raw`C:\work\photo.png` },
      { ...attachments[1], path: String.raw`\\nas\share\photo.png` },
      attachments[2],
    ]);
  });
  it.each([
    String.raw`\\.\PhysicalDrive0`, String.raw`\\?\GLOBALROOT\Device\HarddiskVolume1\private.txt`,
    String.raw`\\?\Volume{123}\private.txt`, String.raw`\\?\PIPE\private`,
    String.raw`\??\C:\private.txt`, String.raw`\\?\C:relative.txt`,
    String.raw`\\?\UNC\server`, String.raw`\\server`,
    String.raw`C:\work\NUL`, String.raw`\\?\C:\work\CON.txt`, String.raw`\\nas\share\COM1.log`,
    String.raw`C:\work\photo.png:secret`, String.raw`\\?\C:\work\..\CON`,
  ])("rejects device or incomplete Windows path %s", path => {
    expect(parsePersistedAttachments(JSON.stringify({ attachments: [{ path }] }))).toEqual([]);
  });
  it("restores structured images and files without changing legacy annotations", () => {
    const attachments = [{ path: "C:\\工作区\\图一.png", name: "图一.png", mediaType: "image/png" }, { path: "/workspace/report.pdf", name: "report.pdf", mediaType: "application/pdf" }];
    expect(parsePersistedAttachments(JSON.stringify({ annotations: [{ id: "note" }], attachments }))).toEqual(attachments);
    expect(parsePersistedAttachments('[{"id":"legacy-note"}]')).toEqual([]);
  });
  it("rejects remote, script, relative and malformed paths", () => {
    expect(parsePersistedAttachments(JSON.stringify({ attachments: [
      { path: "javascript:alert(1)", name: "bad" }, { path: "https://evil/image.png" },
      { path: "../private.txt" }, null, { path: "C:relative.txt" }, { path: "/safe/image.png", name: "image.png" },
    ] }))).toEqual([{ path: "/safe/image.png", name: "image.png", mediaType: "application/octet-stream" }]);
    expect(parsePersistedAttachments("broken JSON")).toEqual([]);
  });
});
