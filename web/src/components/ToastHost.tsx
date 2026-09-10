import { CircleAlert, Check, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useAppStore, type ToastTone } from "../store/appStore";

/** Tone icon; kept as a switch so no lucide component *type* needs importing. */
function ToneIcon({ tone }: { tone: ToastTone }) {
  if (tone === "success") return <Check aria-hidden="true" size={15} strokeWidth={2} />;
  return <CircleAlert aria-hidden="true" size={15} strokeWidth={1.9} />;
}

/**
 * Global transient notices (W8). Mounted once by `App`; entries are pushed
 * through `pushToast` and auto-dismiss after `TOAST_TTL_MS`.
 */
export function ToastHost() {
  const { t } = useTranslation();
  const toasts = useAppStore((s) => s.toasts);
  const dismissToast = useAppStore((s) => s.dismissToast);

  if (toasts.length === 0) return null;

  return (
    <div className="toast-host" role="region" aria-label={t("toast.regionLabel")}>
      {toasts.map((toast) => (
        <div key={toast.id} className={`toast toast-${toast.tone}`} role="status">
          <ToneIcon tone={toast.tone} />
          <span className="toast-message">{t(toast.messageKey)}</span>
          <button
            className="icon-button toast-dismiss"
            type="button"
            aria-label={t("toast.dismiss")}
            onClick={() => dismissToast(toast.id)}
          >
            <X size={13} />
          </button>
        </div>
      ))}
    </div>
  );
}
