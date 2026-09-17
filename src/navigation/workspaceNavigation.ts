import { getLegacySettingsSection, type SettingsSection } from "../components/Settings/settingsSections";
import {
  Brain,
  Clock3,
  LineChart,
  Settings2,
  Sparkles,
  SquarePen,
  UserRoundCheck,
  Users,
  type LucideIcon,
} from "lucide-react";

export type { SettingsSection } from "../components/Settings/settingsSections";

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

export type WorkspaceNavigationTab = WorkspaceRoute | "marketing-dashboard" | "harmony-cut";

export type WorkspaceNavigationLabel =
  | { key: "chat.newChat" | "nav.market" | "nav.scheduler" | "nav.memory" | "nav.settings" }
  | { text: string };

export type WorkspaceNavigationAction =
  | "navigate"
  | "new-chat"
  | "toggle-marketing"
  | "toggle-creative";

export type WorkspaceNavigationChild = {
  id: string;
  route: WorkspaceNavigationTab;
  label: WorkspaceNavigationLabel;
  action?: "navigate" | "open-harmony-cut";
};

export type WorkspaceNavigationItem = {
  id: string;
  route: WorkspaceRoute;
  label: WorkspaceNavigationLabel;
  icon: LucideIcon;
  activeRoutes: readonly WorkspaceNavigationTab[];
  action: WorkspaceNavigationAction;
  children?: readonly WorkspaceNavigationChild[];
};

export const WORKSPACE_NAVIGATION: Record<"work" | "collaboration" | "system", WorkspaceNavigationItem[]> = {
  work: [
    {
      id: "chat",
      route: "chat",
      label: { key: "chat.newChat" },
      icon: SquarePen,
      activeRoutes: ["chat"],
      action: "new-chat",
    },
    {
      id: "creative",
      route: "creative-music",
      label: { text: "AI 创作" },
      icon: Sparkles,
      activeRoutes: ["creative-music", "creative-image"],
      action: "toggle-creative",
      children: [
        { id: "creative-music", route: "creative-music", label: { text: "Harvis 音乐创作" } },
        { id: "creative-image", route: "creative-image", label: { text: "Harvis 图片创作" } },
      ],
    },
    {
      id: "marketing-assets",
      route: "marketing-assets",
      label: { text: "线上营销工作台" },
      icon: LineChart,
      activeRoutes: ["marketing-assets", "marketing-dashboard", "harmony-cut"],
      action: "toggle-marketing",
      children: [
        { id: "marketing-assets", route: "marketing-assets", label: { text: "素材库" } },
        { id: "marketing-dashboard", route: "marketing-dashboard", label: { text: "数据看板" } },
        { id: "harmony-cut", route: "harmony-cut", label: { text: "商摄快剪" }, action: "open-harmony-cut" },
      ],
    },
    {
      id: "skills",
      route: "skills",
      label: { key: "nav.market" },
      icon: Sparkles,
      activeRoutes: ["skills"],
      action: "navigate",
    },
  ],
  collaboration: [
    { id: "team", route: "team", label: { text: "团队" }, icon: Users, activeRoutes: ["team"], action: "navigate" },
    { id: "members", route: "members", label: { text: "成员" }, icon: UserRoundCheck, activeRoutes: ["members"], action: "navigate" },
    { id: "experts", route: "experts", label: { text: "专家" }, icon: Brain, activeRoutes: ["experts"], action: "navigate" },
    { id: "scheduler", route: "scheduler", label: { key: "nav.scheduler" }, icon: Clock3, activeRoutes: ["scheduler"], action: "navigate" },
    { id: "memory", route: "memory", label: { key: "nav.memory" }, icon: Brain, activeRoutes: ["memory"], action: "navigate" },
  ],
  system: [
    { id: "settings", route: "settings", label: { key: "nav.settings" }, icon: Settings2, activeRoutes: ["settings"], action: "navigate" },
  ],
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
};

export function normalizeWorkspaceRoute(route: string): {
  route: WorkspaceRoute;
  settingsSection?: SettingsSection;
} {
  const normalizedRoute = route.trim();
  const settingsSection = getLegacySettingsSection(normalizedRoute);
  if (settingsSection) return { route: "settings", settingsSection };
  const legacyRoute = LEGACY_ROUTE_ALIASES[normalizedRoute];
  if (legacyRoute) return legacyRoute;
  if (ROUTES.has(normalizedRoute as WorkspaceRoute)) {
    return { route: normalizedRoute as WorkspaceRoute };
  }
  return { route: "chat" };
}
