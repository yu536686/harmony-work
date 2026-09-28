import { useEffect, useId, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isMacNativeUI } from "./useMacNativeMenu";

interface NativeDialogOptions {
  open: boolean;
  loading: boolean;
  title: string;
  message: string;
  confirmLabel: string;
  cancelLabel: string;
  danger?: boolean;
  alert?: boolean;
  prompt?: boolean;
  value?: string;
  placeholder?: string;
  error?: string | null;
  extraLabel?: string;
  detail?: string;
  onConfirm: (value: string) => void;
  onCancel: () => void;
  onExtra?: () => void;
}

export function useMacNativeDialog(options: NativeDialogOptions) {
  const baseId = useId();
  const sequence = useRef(0);
  const activeId = useRef("");
  const callbacks = useRef({ onConfirm: options.onConfirm, onCancel: options.onCancel, onExtra: options.onExtra });
  callbacks.current = { onConfirm: options.onConfirm, onCancel: options.onCancel, onExtra: options.onExtra };
  const [nativeFailed, setNativeFailed] = useState(false);
  const nativeMode = isMacNativeUI() && !nativeFailed;
  const {
    open, loading, title, message, confirmLabel, cancelLabel,
    danger, alert, prompt, value, placeholder, error, extraLabel, detail,
  } = options;

  useEffect(() => {
    if (!nativeMode || !open || loading) return;
    const dialogId = `${baseId}-${++sequence.current}`;
    activeId.current = dialogId;
    const onResult = (event: Event) => {
      const result = (event as CustomEvent<{ dialogId: string; accepted: boolean; extra: boolean; value: string }>).detail;
      if (result?.dialogId !== activeId.current) return;
      if (result.extra) callbacks.current.onExtra?.();
      else if (result.accepted) callbacks.current.onConfirm(result.value);
      else callbacks.current.onCancel();
    };
    window.addEventListener("harmony:native-dialog-result", onResult);
    void invoke("show_macos_dialog", {
      dialogId, title, message: [message, error].filter(Boolean).join("\n"),
      confirmLabel, cancelLabel, danger: Boolean(danger), alert: Boolean(alert),
      prompt: Boolean(prompt), value: value ?? "", placeholder, extraLabel, detail,
    }).catch(() => setNativeFailed(true));
    return () => {
      if (activeId.current === dialogId) activeId.current = "";
      void invoke("close_macos_dialog", { dialogId }).catch(() => {});
      window.removeEventListener("harmony:native-dialog-result", onResult);
    };
  }, [nativeMode, open, loading, title, message, confirmLabel, cancelLabel, danger, alert, prompt, placeholder, error, extraLabel, detail, baseId]);

  return nativeMode;
}
