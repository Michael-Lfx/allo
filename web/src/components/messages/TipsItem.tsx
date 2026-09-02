import { ChevronRight, Info } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { TipsData } from "../../lib/activity";

function tipLabel(t: (key: string) => string, tipType: string): string {
  return tipType === "error" ? t("tip.error") : tipType === "warning" ? t("tip.warning") : t("tip.info");
}

export function TipsItem({ tip }: { tip: TipsData }) {
  const { t } = useTranslation();
  const level = tip.tipType === "error" ? "error" : tip.tipType === "warning" ? "warning" : "success";
  return <details className={`tip-card tip-${level}`}>
    <summary>
      <span className="tip-icon" aria-hidden="true"><Info size={15} strokeWidth={1.7} /></span>
      <span className="tip-label">{tipLabel(t, tip.tipType)}</span>
      <ChevronRight className="tip-caret" aria-hidden="true" size={16} strokeWidth={1.7} />
    </summary>
    <div className="tip-body">{tip.content || t("tip.noInfo")}</div>
  </details>;
}
