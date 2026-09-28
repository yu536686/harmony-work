import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import "./AppDropdown.css";

export type AppMenuItem = {
  id: string;
  label: string;
  icon?: ReactNode;
  selected?: boolean;
  action?: boolean;
  /** Non-interactive row (e.g. current path). */
  disabled?: boolean;
};

export type AppDropdownProps = {
  menuId: string;
  triggerLabel: string;
  triggerTitle?: string;
  items: AppMenuItem[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSelect: (id: string) => void;
  disabled?: boolean;
  icon?: ReactNode;
  searchPlaceholder?: string;
  emptyLabel?: string;
  closeOnSelect?: boolean;
  variant?: "default" | "wide" | "toolbar" | "form";
  /** Panel opens above trigger (composer) or below (toolbars). */
  placement?: "above" | "below";
};

export default function AppDropdown({
  menuId,
  icon,
  triggerLabel,
  triggerTitle,
  items,
  open,
  onOpenChange,
  onSelect,
  disabled = false,
  searchPlaceholder,
  emptyLabel,
  closeOnSelect = true,
  variant = "default",
  placement = "below",
}: AppDropdownProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [nativeFailed, setNativeFailed] = useState(false);
  const nativeWasOpen = useRef(false);
  const nativeMode = navigator.userAgent.includes("Macintosh") && isTauri() && !nativeFailed;
  const nativeItems = useMemo(() => items.map(({ id, label, selected, action, disabled }) => ({
    id, label, selected: Boolean(selected), action: Boolean(action), disabled: Boolean(disabled),
  })), [items]);
  const nativeItemsRef = useRef(nativeItems);
  nativeItemsRef.current = nativeItems;
  const nativeItemsKey = JSON.stringify(nativeItems);
  const lastNativeItemsKey = useRef("");

  useEffect(() => {
    if (!nativeMode) return;
    const onSelectEvent = (event: Event) => {
      const detail = (event as CustomEvent<{ menuId: string; id: string }>).detail;
      if (detail?.menuId !== menuId) return;
      onSelect(detail.id);
      if (closeOnSelect) onOpenChange(false);
    };
    const onCloseEvent = (event: Event) => {
      if ((event as CustomEvent<{ menuId: string }>).detail?.menuId === menuId) onOpenChange(false);
    };
    window.addEventListener("harmony:native-dropdown-select", onSelectEvent);
    window.addEventListener("harmony:native-dropdown-close", onCloseEvent);
    return () => {
      window.removeEventListener("harmony:native-dropdown-select", onSelectEvent);
      window.removeEventListener("harmony:native-dropdown-close", onCloseEvent);
    };
  }, [nativeMode, menuId, onSelect, onOpenChange, closeOnSelect]);

  useEffect(() => {
    if (!nativeMode) return;
    if (!open) {
      if (nativeWasOpen.current) void invoke("close_macos_dropdown", { menuId });
      nativeWasOpen.current = false;
      lastNativeItemsKey.current = "";
      return;
    }
    nativeWasOpen.current = true;
    lastNativeItemsKey.current = JSON.stringify(nativeItemsRef.current);
    const rect = rootRef.current?.querySelector("button")?.getBoundingClientRect();
    if (!rect) return;
    let cancelled = false;
    void invoke("show_macos_dropdown", {
      menuId,
      items: nativeItemsRef.current,
      x: rect.x,
      y: rect.y,
      width: rect.width,
      height: rect.height,
      placement,
      searchPlaceholder,
      emptyLabel,
      closeOnSelect,
    }).then(() => {
      if (cancelled) void invoke("close_macos_dropdown", { menuId });
    }).catch(() => {
      if (!cancelled) setNativeFailed(true);
    });
    return () => { cancelled = true; };
  }, [nativeMode, open, menuId, placement, searchPlaceholder, emptyLabel, closeOnSelect]);

  useEffect(() => {
    if (!nativeMode || !open || !lastNativeItemsKey.current || nativeItemsKey === lastNativeItemsKey.current) return;
    lastNativeItemsKey.current = nativeItemsKey;
    void invoke("update_macos_dropdown", { menuId, items: nativeItems });
  }, [nativeMode, open, menuId, nativeItemsKey]);

  useEffect(() => {
    if (!open) {
      setQuery("");
      return;
    }
    if (!nativeMode && (searchPlaceholder || items.length >= 6)) {
      searchRef.current?.focus();
    }
  }, [open, searchPlaceholder, items.length, nativeMode]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        onOpenChange(false);
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onOpenChange(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open, onOpenChange]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter((item) => item.label.toLowerCase().includes(q));
  }, [items, query]);

  const showSearch = items.length >= 6 || Boolean(searchPlaceholder);
  const panelPlacementClass =
    placement === "above" ? "app-dropdown-panel-above" : "app-dropdown-panel-below";

  return (
    <div
      ref={rootRef}
      className={`app-dropdown app-dropdown-${variant}${open ? " app-dropdown-open" : ""}`}
      data-menu={menuId}
    >
      <button
        type="button"
        className="app-dropdown-trigger select-control"
        disabled={disabled}
        aria-expanded={open}
        aria-haspopup="listbox"
        onClick={() => onOpenChange(!open)}
      >
        {icon && (
          <span className="app-dropdown-trigger-icon" aria-hidden>
            {icon}
          </span>
        )}
        <span className="app-dropdown-trigger-label" title={triggerTitle ?? triggerLabel}>
          {triggerLabel}
        </span>
      </button>
      {open && !nativeMode && (
        <div className={`app-dropdown-panel ${panelPlacementClass}`} role="listbox">
          {showSearch && (
            <div className="app-dropdown-header">
              <input
                ref={searchRef}
                type="search"
                className="app-dropdown-search"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={searchPlaceholder}
                onKeyDown={(e) => e.stopPropagation()}
              />
            </div>
          )}
          <div className="app-dropdown-list">
            {filtered.length === 0 ? (
              <div className="app-dropdown-empty">{emptyLabel ?? "—"}</div>
            ) : (
              filtered.map((item) => {
                const rowClass = `app-dropdown-item${item.selected ? " selected" : ""}${item.action ? " action" : ""}${item.disabled ? " info" : ""}`;
                if (item.disabled) {
                  return (
                    <div key={item.id} role="presentation" className={rowClass} title={item.label}>
                      {item.icon && (
                        <span className="app-dropdown-item-icon" aria-hidden>
                          {item.icon}
                        </span>
                      )}
                      <span className="app-dropdown-item-label">{item.label}</span>
                    </div>
                  );
                }
                return (
                  <button
                    key={item.id}
                    type="button"
                    role="option"
                    aria-selected={item.selected}
                    className={rowClass}
                    onClick={() => {
                      onSelect(item.id);
                      if (closeOnSelect) onOpenChange(false);
                    }}
                  >
                    {item.icon && (
                      <span className="app-dropdown-item-icon" aria-hidden>
                        {item.icon}
                      </span>
                    )}
                    <span className="app-dropdown-item-label">{item.label}</span>
                    {item.selected && <span className="app-dropdown-item-check">✓</span>}
                  </button>
                );
              })
            )}
          </div>
        </div>
      )}
    </div>
  );
}
