import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isMacNativeUI } from "../components/ui/useMacNativeMenu";
import type { UpdateSnapshot, UpdateStore } from "./updateStore";

export function useMacUpdatePanel(snapshot: UpdateSnapshot, store: UpdateStore, onClose: () => void) {
  const [failed, setFailed] = useState(false);
  const callbacks = useRef({ store, onClose });
  callbacks.current = { store, onClose };
  const native = isMacNativeUI() && !failed;
  useEffect(() => {
    if (!native) return;
    const action = (event: Event) => {
      switch ((event as CustomEvent<{ action: string }>).detail?.action) {
        case "check": void callbacks.current.store.check(); break;
        case "install": void callbacks.current.store.install(); break;
        case "cancel": void callbacks.current.store.cancel(); break;
        case "stable": void callbacks.current.store.setChannel("stable"); break;
        case "preview": void callbacks.current.store.setChannel("preview"); break;
        case "close": callbacks.current.onClose(); break;
      }
    };
    window.addEventListener("harmony:native-update-action", action);
    return () => {
      window.removeEventListener("harmony:native-update-action", action);
      void invoke("close_macos_update").catch(() => {});
    };
  }, [native]);
  useEffect(() => {
    if (native) void invoke("show_macos_update", { snapshot }).catch(() => setFailed(true));
  }, [native, snapshot]);
  return native;
}
