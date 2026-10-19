import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { createUpdateStore, type UpdateSnapshot } from "./updateStore";

import { readUpdateAppearance } from "./appearance";

export const nativeUpdateStore = createUpdateStore({
  invoke: (command, args) => invoke(command, command === "update_install" ? { ...args, appearance: readUpdateAppearance() } : args),
  listen: (handler) => listen<UpdateSnapshot>("harmony-update-state", (event) => handler(event.payload)),
});
export function supportsDesktopUpdates() { return navigator.userAgent.includes("Windows"); }
