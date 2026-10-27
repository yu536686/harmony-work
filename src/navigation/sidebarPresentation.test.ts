import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const source = (file: string) => readFileSync(path.join(process.cwd(), "src", file), "utf8");

describe("sidebar presentation contracts", () => {
  it("keeps wheel scrolling on the shared sidebar body, not a containing inner scroller", () => {
    const windows = source("windows.css");
    const css = source("App.css");
    expect(windows).not.toMatch(/\.sidebar-nav\s*\{[^}]*overflow-y:\s*auto/);
    expect(css).toMatch(/\.sidebar-body\s*\{[^}]*overflow-y:\s*auto/);
  });

  it("removes the minimal-mode navigation entry without removing the window API", () => {
    expect(source("App.tsx")).not.toContain('className="nav-item minimal-mode-btn"');
  });

  it("lets search use the full available sidebar width", () => {
    expect(source("App.css")).toMatch(/\.sidebar-search\s*\{[^}]*width:\s*100%/);
  });

  it("uses the effective expanded state for both submenu triggers and their panels", () => {
    expect(source("App.tsx").includes("const expanded = !sidebarCollapsed && (")).toBe(true);
  });
});
