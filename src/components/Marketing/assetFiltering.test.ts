import { describe, expect, it } from "vitest";
import {
  isFallbackBusinessExtension,
  resolveVisibleSelection,
  shouldShowAsset,
} from "./assetFiltering";

describe("assetFiltering", () => {
  it("hides auxiliary records by default and shows them only when requested", () => {
    const nfo = { extension: "nfo", isAuxiliary: true, contentClass: "auxiliary" };
    expect(shouldShowAsset(nfo, false)).toBe(false);
    expect(shouldShowAsset(nfo, true)).toBe(true);
  });

  it("falls back to a safe whitelist when the portal provides no classification", () => {
    expect(isFallbackBusinessExtension("jpg")).toBe(true);
    expect(isFallbackBusinessExtension("psd")).toBe(true);
    expect(isFallbackBusinessExtension("nfo")).toBe(false);
    expect(isFallbackBusinessExtension("srt")).toBe(false);

    const jpg = { extension: "jpg" };
    const nfo = { extension: "nfo" };
    expect(shouldShowAsset(jpg, false)).toBe(true);
    expect(shouldShowAsset(nfo, false)).toBe(false);
  });

  it("keeps classified business and source files visible by default", () => {
    expect(shouldShowAsset({ extension: "jpg", contentClass: "business", isAuxiliary: false }, false)).toBe(true);
    expect(shouldShowAsset({ extension: "psd", contentClass: "source", isAuxiliary: false }, false)).toBe(true);
  });

  it("clears the selected asset when it is no longer present in the filtered list", () => {
    const visible = [{ id: "a" }, { id: "b" }];
    expect(resolveVisibleSelection("a", visible)).toBe("a");
    expect(resolveVisibleSelection("missing", visible)).toBeNull();
    expect(resolveVisibleSelection(null, visible)).toBeNull();
  });
});
