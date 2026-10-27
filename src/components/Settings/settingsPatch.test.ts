import { describe, expect, it } from "vitest";
import type { Settings } from "../../services/tauri";
import { buildSettingsPatch, changedSettingsFields } from "./settingsPatch";

describe("settings dirty patches", () => {
  it("does not resend untouched sections, including bound tokens", () => {
    const baseline = { language: "zh", wechat_bot_token: "bound", vision_api_key: "vision" } as Settings;
    expect(buildSettingsPatch(baseline, { ...baseline, language: "en" })).toEqual({ language: "en" });
  });

  it("represents explicit secret removal with null without clearing other secrets", () => {
    const baseline = { vision_api_key: "vision", wechat_bot_token: "bound", wechat_gateway_token: "gateway" } as Settings;
    expect(buildSettingsPatch(baseline, { ...baseline, wechat_bot_token: "", vision_api_key: "" })).toEqual({
      vision_api_key: null, wechat_bot_token: null,
    });
  });

  it("omits unchanged nested secrets while editing provider labels and SSH hosts", () => {
    const baseline = { llm_providers: [{ id: "p", label: "old", api_key: "keep" }],
      ssh_servers: [{ id: "s", host: "old", password: "keep", private_key: "keep-key" }] } as Settings;
    const draft = { ...baseline, llm_providers: [{ ...baseline.llm_providers![0], label: "new" }],
      ssh_servers: [{ ...baseline.ssh_servers![0], host: "new" }] };
    const patch = buildSettingsPatch(baseline, draft);
    expect(patch.llm_providers).toEqual([{ id: "p", label: "new" }]);
    expect(patch.ssh_servers).toEqual([{ id: "s", host: "new" }]);
  });

  it("clears nested secrets only when the user removed them", () => {
    const baseline = { llm_providers: [{ id: "p", api_key: "old" }], ssh_servers: [] } as unknown as Settings;
    const draft = { ...baseline, llm_providers: [{ ...baseline.llm_providers![0], api_key: "" }] };
    expect(buildSettingsPatch(baseline, draft).llm_providers).toEqual([{ id: "p", api_key: null }]);
  });

  it("can rebase a dirty field onto fresh remote settings without losing either", () => {
    const baseline = { language: "zh", wechat_bot_token: "" } as Settings;
    const local = { ...baseline, language: "en" };
    const latest = { ...baseline, wechat_bot_token: "bound" };
    expect({ ...latest, ...changedSettingsFields(baseline, local) }).toEqual({ language: "en", wechat_bot_token: "bound" });
  });
});
