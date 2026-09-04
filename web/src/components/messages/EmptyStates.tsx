import { Bot, Sparkles } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ProviderWithModel } from "../../lib/protocol";

export function WelcomePanel({ onConnect }: { onConnect: () => void }) {
  const { t } = useTranslation();
  return <div className="empty-state welcome-state">
    <div className="empty-symbol" aria-hidden="true"><Sparkles size={25} strokeWidth={1.45} /></div>
    <h1>{t("empty.welcomeTitle")}</h1>
    <p>{t("empty.welcomeBody")}</p>
    <button className="primary-button" type="button" onClick={onConnect}>{t("empty.connectAppServer")}</button>
  </div>;
}

export function EmptyChatPanel({ model, isNew, onSettings }: { model: ProviderWithModel | null; isNew: boolean; onSettings: () => void }) {
  const { t } = useTranslation();
  if (!isNew) {
    // A selected conversation that has no messages yet (e.g. a just-created
    // chat in a re-added workspace): show a visible empty state instead of a
    // blank message area.
    return <div className="empty-state">
      <div className="empty-symbol" aria-hidden="true"><Bot size={30} strokeWidth={1.5} /></div>
      <h1>{t("empty.noMessagesTitle")}</h1>
      <p>{t("empty.noMessagesBody")}</p>
    </div>;
  }
  return <div className="empty-state">
    <div className="empty-symbol product-logo" aria-hidden="true"><Bot size={44} strokeWidth={1.6} /></div>
    <p>{t("empty.hint")}</p>
  </div>;
}
