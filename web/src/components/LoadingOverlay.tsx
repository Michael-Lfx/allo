import { Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";

/**
 * Full-screen frosted-glass loading dialog shown while the client is
 * connecting to the App Server backend (phase === "connecting").
 */
export function LoadingOverlay() {
  const { t } = useTranslation();
  return (
    <div className="loading-backdrop" role="alert" aria-busy="true">
      <div className="loading-panel">
        <div className="loading-spinner" aria-hidden="true">
          <Loader2 size={26} strokeWidth={1.8} className="spin" />
        </div>
        <div className="loading-title">{t("loading.connecting")}</div>
        <div className="loading-hint">{t("loading.hint")}</div>
      </div>
    </div>
  );
}
