import { Check, X } from "lucide-react";
import { useClickAway } from "ahooks";
import { useRef } from "react";
import { useTranslation } from "react-i18next";
import type { ConversationModelOptions, ReasoningEffort } from "../lib/protocol";

function effortLabel(t: (key: string) => string, effort: ReasoningEffort): string {
  const key = effort === "low" ? "modelPicker.effortLow"
    : effort === "medium" ? "modelPicker.effortMedium"
    : effort === "high" ? "modelPicker.effortHigh"
    : effort === "max" ? "modelPicker.effortMax"
    : "modelPicker.effortXhigh";
  return t(key);
}

export function ModelPicker({
  options,
  selectedKey,
  effort,
  hasConversation,
  onSelectModel,
  onSelectEffort,
  onClose,
}: {
  options: ConversationModelOptions | null;
  selectedKey: string | null;
  effort: ReasoningEffort | "";
  hasConversation: boolean;
  onSelectModel: (key: string | null) => void;
  onSelectEffort: (effort: ReasoningEffort | "") => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement | null>(null);
  // Backend `reasoning_efforts` may not include "max" — always append it.
  const efforts = (() => {
    const base: ReasoningEffort[] = options?.reasoning_efforts ?? ["low", "medium", "high", "xhigh"];
    return base.includes("max") ? base : [...base, "max" as ReasoningEffort];
  })();
  // Clicking outside the picker closes it.
  useClickAway(() => onClose(), ref, ["mousedown", "touchstart"]);
  return <div className="model-picker" role="dialog" aria-label={t("modelPicker.ariaLabel")} ref={ref}>
    <div className="model-picker-section">
      <span className="model-picker-title">{t("modelPicker.modelTitle")}</span>
      {options?.providers.map((provider) => (
        <div className="model-provider-group" key={provider.name}>
          <span className="model-provider-name">{provider.name}</span>
          {provider.models.map((model) => {
            const key = `${provider.name}/${model.name}`;
            return <button
              className={`model-option ${selectedKey === key ? "is-active" : ""}`}
              type="button"
              key={key}
              onClick={() => onSelectModel(key)}
            >
              {selectedKey === key && <Check aria-hidden="true" size={15} strokeWidth={2.2} className="model-check" />}
              <span className="model-option-name">{model.display_name || model.name}</span>
              <span className="model-option-meta">
                {model.name}
                {model.context_limit ? t("modelPicker.contextMeta", { k: Math.round(model.context_limit / 1000) }) : ""}
              </span>
            </button>;
          })}
        </div>
      ))}
      {options && options.providers.length === 0 && <p className="model-picker-empty">{t("modelPicker.noModels")}</p>}
    </div>
    <div className="model-picker-section">
      <span className="model-picker-title">{t("modelPicker.effortTitle")}</span>
      <div className="effort-row" role="radiogroup" aria-label={t("modelPicker.effortTitle")}>
        {efforts.map((value) => (
          <button className={`effort-option ${effort === value ? "is-active" : ""}`} type="button" role="radio" aria-checked={effort === value} key={value} onClick={() => onSelectEffort(value)}>{effortLabel(t, value)}</button>
        ))}
      </div>
    </div>
    <button className="model-picker-close" type="button" onClick={onClose} aria-label={t("modelPicker.close")}><X size={15} /></button>
  </div>;
}
