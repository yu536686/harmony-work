/**
 * UI values shared by the application shell and documented in design.md.
 * Keep this module dependency-free so it can be used by lightweight views.
 */
export const DESIGN_TOKENS = {
  layout: {
    sidebarExpanded: 280,
    sidebarCollapsed: 64,
  },
  motion: {
    fast: 150,
    standard: 220,
    slow: 320,
  },
} as const;

export function getSidebarWidth(collapsed: boolean): number {
  return collapsed
    ? DESIGN_TOKENS.layout.sidebarCollapsed
    : DESIGN_TOKENS.layout.sidebarExpanded;
}
