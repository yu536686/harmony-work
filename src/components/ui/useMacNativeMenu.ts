import { useId, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

type NativeMenuItem = {
  id: string;
  label: string;
  disabled: boolean;
  selected: boolean;
  separator: boolean;
};

export function isMacNativeUI() {
  return navigator.userAgent.includes("Macintosh") && isTauri();
}

/** Mirrors an existing button menu to NSMenu on macOS; its DOM remains the Windows fallback. */
export function useMacNativeMenu(
  menuRef: RefObject<HTMLElement>,
  open: boolean,
  onClose: () => void,
  identity = "",
) {
  const menuId = useId();
  const [nativeFailed, setNativeFailed] = useState(false);
  const nativeMode = isMacNativeUI() && !nativeFailed;
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useLayoutEffect(() => {
    if (!nativeMode || !open || !menuRef.current) return;
    const source = menuRef.current;
    const buttons = new Map<string, HTMLElement>();
    const items: NativeMenuItem[] = [];
    for (const child of Array.from(source.children)) {
      const element = child as HTMLElement;
      if (/separator|divider|(?:^|-)sep(?:$|-)/.test(element.className)) {
        items.push({ id: "", label: "", disabled: true, selected: false, separator: true });
        continue;
      }
      const button = element instanceof HTMLButtonElement ? element : element.querySelector("button");
      const action = button ?? (element.hasAttribute("data-native-menu-action") ? element : null);
      const label = (button ?? element).textContent?.replace(/\s+/g, " ").trim() ?? "";
      if (!label) continue;
      const id = `item-${items.length}`;
      if (action) buttons.set(id, action);
      items.push({ id, label, disabled: !action || (button?.disabled ?? false), selected: false, separator: false });
    }
    if (!buttons.size) return;
    const rect = source.getBoundingClientRect();
    const onSelect = (event: Event) => {
      const detail = (event as CustomEvent<{ menuId: string; id: string }>).detail;
      if (detail?.menuId === menuId) {
        const action = buttons.get(detail.id);
        if (action?.hasAttribute("data-native-menu-action")) action.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
        else action?.click();
      }
    };
    const onDismiss = (event: Event) => {
      if ((event as CustomEvent<{ menuId: string }>).detail?.menuId === menuId) closeRef.current();
    };
    window.addEventListener("harmony:native-menu-select", onSelect);
    window.addEventListener("harmony:native-menu-close", onDismiss);
    void invoke("show_macos_menu", { menuId, items, x: rect.left, y: rect.top }).catch(() => {
      setNativeFailed(true);
    });
    return () => {
      window.removeEventListener("harmony:native-menu-select", onSelect);
      window.removeEventListener("harmony:native-menu-close", onDismiss);
    };
  }, [nativeMode, open, identity, menuId, menuRef]);

  return nativeMode;
}
