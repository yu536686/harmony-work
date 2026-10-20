import { afterEach, expect, it } from "vitest";
import { readUpdateAppearance } from "./appearance";
afterEach(() => { delete document.documentElement.dataset.theme; delete document.documentElement.dataset.colorMode; });
it("captures the currently rendered client appearance", () => {
  document.documentElement.dataset.theme = "gold";
  document.documentElement.dataset.colorMode = "dark";
  expect(readUpdateAppearance()).toEqual({ theme: "gold", colorMode: "dark" });
});
it("normalizes missing or unexpected DOM values to the supported light theme", () => {
  document.documentElement.dataset.theme = "../../file";
  document.documentElement.dataset.colorMode = "unexpected";
  expect(readUpdateAppearance()).toEqual({ theme: "violet", colorMode: "light" });
});
