/** Only the two cosmetic values needed by the external updater cross the IPC boundary. */
export function readUpdateAppearance() {
  const { theme, colorMode } = document.documentElement.dataset;
  return { theme: theme === "gold" ? "gold" : "violet", colorMode: colorMode === "dark" ? "dark" : "light" } as const;
}
