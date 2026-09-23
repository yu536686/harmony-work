import { beforeEach, describe, expect, it, vi } from "vitest";
import { messageImageSrc, safeMessageUrl } from "./messageImage";

const convertFileSrc = vi.fn((path: string) => `asset:${path}`);
vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc: (path: string) => convertFileSrc(path) }));

describe("conversation images", () => {
  beforeEach(() => convertFileSrc.mockClear());

  it("converts Windows and file URI paths through the asset protocol", () => {
    expect(messageImageSrc("E:\\skills\\theme-style-grid.png")).toBe("asset:E:\\skills\\theme-style-grid.png");
    expect(messageImageSrc("file:///C:/Program%20Files/theme.png")).toBe("asset:C:\\Program Files\\theme.png");
    expect(convertFileSrc).toHaveBeenCalledTimes(2);
  });

  it("keeps safe remote and embedded images but rejects executable URLs", () => {
    expect(messageImageSrc("https://example.com/theme.png")).toBe("https://example.com/theme.png");
    expect(messageImageSrc("data:image/png;base64,aGVsbG8=")).toBe("data:image/png;base64,aGVsbG8=");
    expect(messageImageSrc("javascript:alert(1)")).toBe("");
    expect(safeMessageUrl("file:///E:/skills/theme.png")).toBe("file:///E:/skills/theme.png");
    expect(safeMessageUrl("data:image/png;base64,aGVsbG8=")).toBe("data:image/png;base64,aGVsbG8=");
    expect(safeMessageUrl("javascript:alert(1)")).toBe("");
  });
});
