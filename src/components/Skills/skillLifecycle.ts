export type SkillRuntimeState = "unverified" | "ready" | "repair-required";

export type SkillLifecycle = {
  installed: boolean;
  enabled: boolean;
  dependenciesReady: boolean;
  runtimeState: SkillRuntimeState;
  repairHint?: string;
};

export function canInvokeSkill(state: SkillLifecycle): boolean {
  return state.installed
    && state.enabled
    && state.dependenciesReady
    && state.runtimeState === "ready";
}
