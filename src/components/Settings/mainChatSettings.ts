import type { Settings } from "../../services/tauri";
import { mainChatCapability } from "../../utils/providerCapabilities";

type ChatSelection = {
  provider?: string;
  model?: string;
  enabled_models?: string[];
  base_url?: string;
  custom_base_url?: string;
};

function selectionIdentity(selection: ChatSelection): string {
  const model = selection.model?.trim() ?? "";
  return JSON.stringify([
    selection.provider ?? "harmony",
    model,
    [...new Set([model, ...(selection.enabled_models ?? [])].map(value => value.trim()).filter(Boolean))].sort(),
    (selection.base_url ?? selection.custom_base_url ?? "").trim(),
  ]);
}

export function mainChatSelectionError(previous: ChatSelection | undefined, next: ChatSelection): string | null {
  const capability = mainChatCapability(next.provider ?? "harmony");
  if (capability.selectable || (previous && selectionIdentity(previous) === selectionIdentity(next))) return null;
  return capability.message;
}

export function mainChatSettingsError(previous: Partial<Settings>, next: Partial<Settings>): string | null {
  const mainError = mainChatSelectionError(previous, next);
  if (mainError) return mainError;
  for (const provider of next.llm_providers ?? []) {
    const error = mainChatSelectionError(previous.llm_providers?.find(item => item.id === provider.id), provider);
    if (error) return `${provider.label || provider.id}: ${error}`;
  }
  return null;
}
