import { Check, X } from "lucide-react";
import { useClickAway } from "ahooks";
import { useRef } from "react";
import { useTranslation } from "react-i18next";
import type { ConversationModelOptions, ModelSummary, ReasoningEffort } from "../lib/protocol";

function effortLabel(t: (key: string) => string, effort: ReasoningEffort): string {
  const key = effort === "low" ? "modelPicker.effortLow"
    : effort === "medium" ? "modelPicker.effortMedium"
    : effort === "high" ? "modelPicker.effortHigh"
    : effort === "max" ? "modelPicker.effortMax"
    : "modelPicker.effortXhigh";
  return t(key);
}

/** One rendered row: config options win (they carry context limits), the
 *  `models/list` directory fills in providers the config-only projection
 *  omits (e.g. providers registered through the UI into the DB). */
interface PickerEntry {
  key: string;
  name: string;
  displayName: string;
  contextLimit?: number | null;
  isDefault: boolean;
}

function buildEntries(
  options: ConversationModelOptions | null,
  directory: ModelSummary[],
): Array<{ provider: string; entries: PickerEntry[] }> {
  const groups = new Map<string, PickerEntry[]>();
  const seen = new Set<string>();
  for (const provider of options?.providers ?? []) {
    const entries: PickerEntry[] = [];
    for (const model of provider.models) {
      const key = `${provider.name}/${model.name}`;
      seen.add(key);
      entries.push({
        key,
        name: model.name,
        displayName: model.display_name || model.name,
        contextLimit: model.context_limit,
        isDefault: false,
      });
    }
    if (entries.length > 0) groups.set(provider.name, entries);
  }
  // `models/list` is authoritative about the default model and may know
  // providers the config projection lacks.
  for (const entry of directory) {
    const key = `${entry.provider_name}/${entry.model}`;
    const existing = groups.get(entry.provider_name);
    if (existing) {
      const row = existing.find((candidate) => candidate.key === key);
      if (row) {
        row.isDefault = entry.is_default;
        continue;
      }
      existing.push({
        key,
        name: entry.model,
        displayName: entry.display_name || entry.model,
        contextLimit: null,
        isDefault: entry.is_default,
      });
      continue;
    }
    if (seen.has(key)) continue;
    groups.set(entry.provider_name, [{
      key,
      name: entry.model,
      displayName: entry.display_name || entry.model,
      contextLimit: null,
      isDefault: entry.is_default,
    }]);
  }
  return [...groups.entries()].map(([provider, entries]) => ({ provider, entries }));
}

export function ModelPicker({
  options,
  directory = [],
  selectedKey,
  effort,
  hasConversation,
  onSelectModel,
  onSelectEffort,
  onClose,
}: {
  options: ConversationModelOptions | null;
  directory?: ModelSummary[];
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
  const groups = buildEntries(options, directory);
  // Clicking outside the picker closes it.
  useClickAway(() => onClose(), ref, ["mousedown", "touchstart"]);
  return <div className="model-picker" role="dialog" aria-label={t("modelPicker.ariaLabel")} ref={ref}>
    <div className="model-picker-section">
      <span className="model-picker-title">{t("modelPicker.modelTitle")}</span>
      <button
        className={`model-option model-option-default ${selectedKey === null ? "is-active" : ""}`}
        type="button"
        onClick={() => onSelectModel(null)}
      >
        {selectedKey === null && <Check aria-hidden="true" size={15} strokeWidth={2.2} className="model-check" />}
        <span className="model-option-name">{t("modelPicker.defaultModel")}</span>
        <span className="model-option-meta">{t("modelPicker.defaultMeta")}</span>
      </button>
      {groups.map((group) => (
        <div className="model-provider-group" key={group.provider}>
          <span className="model-provider-name">{group.provider}</span>
          {group.entries.map((entry) => (
            <button
              className={`model-option ${selectedKey === entry.key ? "is-active" : ""}`}
              type="button"
              key={entry.key}
              onClick={() => onSelectModel(entry.key)}
            >
              {selectedKey === entry.key && <Check aria-hidden="true" size={15} strokeWidth={2.2} className="model-check" />}
              <span className="model-option-name">{entry.displayName}</span>
              {entry.isDefault && <span className="model-default-badge">{t("modelPicker.defaultBadge")}</span>}
              <span className="model-option-meta">
                {entry.name}
                {entry.contextLimit ? t("modelPicker.contextMeta", { k: Math.round(entry.contextLimit / 1000) }) : ""}
              </span>
            </button>
          ))}
        </div>
      ))}
      {groups.length === 0 && <p className="model-picker-empty">{t("modelPicker.noModels")}</p>}
    </div>
    <div className="model-picker-section">
      <span className="model-picker-title">{t("modelPicker.effortTitle")}</span>
      <div className="effort-row" role="radiogroup" aria-label={t("modelPicker.effortTitle")}>
        <button
          className={`effort-option ${effort === "" ? "is-active" : ""}`}
          type="button"
          role="radio"
          aria-checked={effort === ""}
          onClick={() => onSelectEffort("")}
        >{t("modelPicker.effortDefault")}</button>
        {efforts.map((value) => (
          <button className={`effort-option ${effort === value ? "is-active" : ""}`} type="button" role="radio" aria-checked={effort === value} key={value} onClick={() => onSelectEffort(value)}>{effortLabel(t, value)}</button>
        ))}
      </div>
      <p className="model-picker-hint">{hasConversation ? t("modelPicker.effortHintCurrent") : t("modelPicker.effortHintNew")}</p>
    </div>
    <button className="model-picker-close" type="button" onClick={onClose} aria-label={t("modelPicker.close")}><X size={15} /></button>
  </div>;
}
