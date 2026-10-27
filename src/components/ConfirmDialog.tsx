import { useEffect, useCallback, useId, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useMacNativeDialog } from "./ui/useMacNativeDialog";

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  /** "danger" (red) or "primary" (accent) — defaults to "danger" */
  variant?: "danger" | "primary";
  /** Alert mode: single OK button, no cancel */
  alert?: boolean;
  loading?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export default function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel,
  cancelLabel,
  variant = "danger",
  alert = false,
  loading = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const messageId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const resolvedConfirm = confirmLabel ?? t("common.confirm");
  const resolvedCancel = cancelLabel ?? t("common.cancel");
  const nativeDialog = useMacNativeDialog({
    open, loading, title, message,
    confirmLabel: resolvedConfirm, cancelLabel: resolvedCancel,
    danger: variant === "danger", alert,
    onConfirm: () => onConfirm(), onCancel,
  });
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.key === "Escape" && !loading) onCancel();
      if (e.key === "Tab") {
        const buttons = dialogRef.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)");
        if (!buttons?.length) { e.preventDefault(); return; }
        const first = buttons[0]; const last = buttons[buttons.length - 1];
        if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
        else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
      }
    },
    [loading, onCancel],
  );

  useEffect(() => {
    if (!open || nativeDialog) return;
    const previous = document.activeElement;
    dialogRef.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, [open, nativeDialog, handleKeyDown]);

  if (!open || nativeDialog) return null;

  const isDanger = variant === "danger";

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 9999,
        background: "rgba(0,0,0,0.45)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
      onClick={() => !loading && onCancel()}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={messageId}
        style={{
          background: "var(--bg-primary)",
          borderRadius: 12,
          padding: "24px 28px",
          maxWidth: 400,
          width: "90%",
          maxHeight: "85vh",
          overflow: "auto",
          boxShadow: "0 8px 32px rgba(0,0,0,0.3)",
          border: "1px solid var(--border)",
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div
          id={titleId}
          style={{
            fontSize: 15,
            fontWeight: 600,
            color: "var(--text-primary)",
            marginBottom: 10,
          }}
        >
          {title}
        </div>
        <div
          id={messageId}
          style={{
            fontSize: 13,
            color: "var(--text-secondary)",
            marginBottom: 20,
            lineHeight: 1.5,
          }}
        >
          {message}
        </div>
        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
          {!alert && (
            <button
              onClick={onCancel}
              disabled={loading}
              style={{
                padding: "6px 16px",
                fontSize: 13,
                background: "var(--bg-secondary)",
                border: "1px solid var(--border)",
                borderRadius: 6,
                color: "var(--text-secondary)",
                cursor: loading ? "default" : "pointer",
              }}
            >
              {resolvedCancel}
            </button>
          )}
          <button
            onClick={onConfirm}
            disabled={loading}
            style={{
              padding: "6px 16px",
              fontSize: 13,
              fontWeight: 600,
              border: "none",
              borderRadius: 6,
              cursor: loading ? "default" : "pointer",
              opacity: loading ? 0.6 : 1,
              background: isDanger ? "#dc3545" : "var(--accent)",
              color: "#fff",
            }}
          >
            {loading ? "..." : resolvedConfirm}
          </button>
        </div>
      </div>
    </div>
  );
}
