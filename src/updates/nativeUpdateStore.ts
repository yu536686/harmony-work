import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { createUpdateStore, type UpdateSnapshot } from "./updateStore";

export const nativeUpdateStore = createUpdateStore({
  invoke,
  listen: (handler) => listen<UpdateSnapshot>("harmony-update-state", (event) => handler(event.payload)),
});
export function supportsDesktopUpdates() { return navigator.userAgent.includes("Windows"); }
