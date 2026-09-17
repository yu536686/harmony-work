export type SettingsSection =
  | "account-cloud"
  | "model-api"
  | "channels"
  | "workspace-security"
  | "agent-skills"
  | "appearance"
  | "system-diagnostics";

export type WorkspaceRoute =
  | "chat"
  | "creative-music"
  | "creative-image"
  | "marketing-assets"
  | "skills"
  | "team"
  | "members"
  | "experts"
  | "scheduler"
  | "memory"
  | "settings"
  | "about";

export type WorkspaceNavigationItem = {
  id: string;
  route: WorkspaceRoute;
};

export const WORKSPACE_NAVIGATION: Record<"work" | "collaboration" | "system", WorkspaceNavigationItem[]> = {
  work: [
    { id: "chat", route: "chat" },
    { id: "creative", route: "creative-music" },
    { id: "marketing-assets", route: "marketing-assets" },
    { id: "skills", route: "skills" },
  ],
  collaboration: [
    { id: "team", route: "team" },
    { id: "members", route: "members" },
    { id: "experts", route: "experts" },
    { id: "scheduler", route: "scheduler" },
    { id: "memory", route: "memory" },
  ],
  system: [{ id: "settings", route: "settings" }],
};

const ROUTES = new Set<WorkspaceRoute>([
  "chat",
  "creative-music",
  "creative-image",
  "marketing-assets",
  "skills",
  "team",
  "members",
  "experts",
  "scheduler",
  "memory",
  "settings",
  "about",
]);

const LEGACY_ROUTE_ALIASES: Record<string, { route: WorkspaceRoute; settingsSection?: SettingsSection }> = {
  creative: { route: "creative-music" },
  market: { route: "skills" },
  school: { route: "members" },
  pond: { route: "team" },
  tools: { route: "settings", settingsSection: "agent-skills" },
  audit: { route: "settings", settingsSection: "system-diagnostics" },
  debug: { route: "settings", settingsSection: "system-diagnostics" },
};

export function normalizeWorkspaceRoute(route: string): {
  route: WorkspaceRoute;
  settingsSection?: SettingsSection;
} {
  const normalizedRoute = route.trim();
  const legacyRoute = LEGACY_ROUTE_ALIASES[normalizedRoute];
  if (legacyRoute) return legacyRoute;
  if (ROUTES.has(normalizedRoute as WorkspaceRoute)) {
    return { route: normalizedRoute as WorkspaceRoute };
  }
  return { route: "chat" };
}
