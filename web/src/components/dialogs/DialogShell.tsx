import { useEffect, useRef, type ReactNode } from "react";
import { useClickAway } from "ahooks";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import { IconButton } from "../IconButton";

/**
 * Shared modal shell for every App Server dialog.
 *
 * Uses `useClickAway` (ahooks) so clicking anywhere OUTSIDE the dialog body —
 * including the backdrop — reliably closes it, and Escape closes it too.
 * This replaces the brittle hand-rolled `onMouseDown` + `stopPropagation`
 * pattern that failed on some interactions (drag-out, scrollbar, text select).
 *
 * `onClose` being absent *is* the blocking mode: no close button, and neither
 * click-away nor Escape dismisses the dialog. The connection gate uses it
 * deliberately — while there is no App Server there is nothing usable behind
 * it, so there is no honest way out.
 */
export function DialogShell({
  onClose,
  labelledBy,
  eyebrow = "ALLO APP SERVER",
  title,
  titleId,
  width = "default",
  children,
}: {
  /** Omit to render a blocking dialog (see above). */
  onClose?: () => void;
  labelledBy: string;
  eyebrow?: string;
  title: string;
  titleId: string;
  /**
   * `default` = 单列窄框（设置/重命名等）；`wide` = 双列大框（设置、目录选择）；
   * `large` = 单列但更宽（连接门：只有一组配置，撑成 `wide` 的 860px 会显得空）。
   */
  width?: "default" | "wide" | "large";
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const panelRef = useRef<HTMLElement | null>(null);

  // Click anywhere outside the panel closes it (unless it is a blocking dialog).
  useClickAway(() => {
    onClose?.();
  }, panelRef, ["mousedown", "touchstart"]);

  // Escape closes it too.
  useEffect(() => {
    if (!onClose) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  return (
    <div className="settings-backdrop">
      <section
        className={`settings-dialog ${width === "wide" ? "is-wide" : ""} ${width === "large" ? "is-large" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
        ref={panelRef}
      >
        <div className="dialog-header">
          <div>
            <span className="eyebrow">{eyebrow}</span>
            <h1 id={titleId}>{title}</h1>
          </div>
          {onClose && (
            <IconButton label={t("common.close")} onClick={onClose}>
              <X size={19} strokeWidth={1.7} />
            </IconButton>
          )}
        </div>
        {children}
      </section>
    </div>
  );
}
