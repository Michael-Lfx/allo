import { WifiOff } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useAppStore } from "../store/appStore";

/**
 * Disconnect banner (W8). A lost link pauses every control gated on
 * `phase === "online"`, so this offers the manual reconnect that re-runs the
 * handshake and re-arms the live subscription (T8).
 */
export function ConnectionBanner() {
  const { t } = useTranslation();
  const connectionLost = useAppStore((s) => s.connectionLost);
  const phase = useAppStore((s) => s.phase);
  const connect = useAppStore((s) => s.connect);
  const dismissConnectionLost = useAppStore((s) => s.dismissConnectionLost);

  if (!connectionLost) return null;
  const reconnecting = phase === "connecting";

  return (
    <div className="connection-banner" role="alert">
      <WifiOff aria-hidden="true" size={16} strokeWidth={1.9} />
      <div className="connection-banner-text">
        <strong>{t("connection.lostTitle")}</strong>
        <span>{t("connection.lostBody")}</span>
      </div>
      <div className="connection-banner-actions">
        <button className="primary-button" type="button" disabled={reconnecting} onClick={() => void connect()}>
          {reconnecting ? t("connection.reconnecting") : t("connection.reconnect")}
        </button>
        <button className="quiet-button" type="button" onClick={dismissConnectionLost}>
          {t("connection.ignore")}
        </button>
      </div>
    </div>
  );
}
