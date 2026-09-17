import type { LucideIcon } from "lucide-react";
import {
  Bot,
  Cloud,
  Paintbrush,
  Plug,
  Settings2,
  ShieldCheck,
  Wrench,
} from "lucide-react";

export type SettingsSection =
  | "account-cloud"
  | "model-api"
  | "channels"
  | "workspace-security"
  | "agent-skills"
  | "appearance"
  | "system-diagnostics";

export type SettingsSectionDefinition = {
  id: SettingsSection;
  label: string;
  icon: LucideIcon;
  fieldIds: string[];
  legacyRoutes: string[];
};

export const SETTINGS_SECTIONS: SettingsSectionDefinition[] = [
  {
    id: "account-cloud",
    label: "账户与云端",
    icon: Cloud,
    fieldIds: ["account-settings-sync"],
    legacyRoutes: [],
  },
  {
    id: "model-api",
    label: "模型与 API",
    icon: Settings2,
    fieldIds: ["ai-provider", "named-llm-providers"],
    legacyRoutes: [],
  },
  {
    id: "channels",
    label: "渠道与连接",
    icon: Plug,
    fieldIds: ["im-gateway", "email"],
    legacyRoutes: [],
  },
  {
    id: "workspace-security",
    label: "工作区与安全",
    icon: ShieldCheck,
    fieldIds: ["workspace", "security"],
    legacyRoutes: [],
  },
  {
    id: "agent-skills",
    label: "Agent 与技能",
    icon: Bot,
    fieldIds: ["agent-config", "ssh-servers"],
    legacyRoutes: [],
  },
  {
    id: "appearance",
    label: "外观与交互",
    icon: Paintbrush,
    fieldIds: ["appearance"],
    legacyRoutes: [],
  },
  {
    id: "system-diagnostics",
    label: "系统与诊断",
    icon: Wrench,
    fieldIds: ["runtime-dependencies", "tools", "audit", "debug"],
    legacyRoutes: ["tools", "audit", "debug"],
  },
];

export function getLegacySettingsSection(route: string): SettingsSection | undefined {
  return SETTINGS_SECTIONS.find((section) => section.legacyRoutes.includes(route))?.id;
}
