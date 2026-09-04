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
  onClose: () => void;
  labelledBy: string;
  eyebrow?: string;
  title: string;
  titleId: string;
  width?: "default" | "wide";
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const panelRef = useRef<HTMLElement | null>(null);

  // Click anywhere outside the panel closes it.
  useClickAway(() => onClose(), panelRef, ["mousedown", "touchstart"]);

  // Escape closes it too.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  return (
    <div className="settings-backdrop">
      <section
        className={`settings-dialog ${width === "wide" ? "is-wide" : ""}`}
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
          <IconButton label={t("common.close")} onClick={onClose}>
            <X size={19} strokeWidth={1.7} />
          </IconButton>
        </div>
        {children}
      </section>
    </div>
  );
}
