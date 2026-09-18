/**
 * Settings → 市场源: the marketplace registry.
 *
 * This is where the registry lives now. It used to be a tab on the catalog page;
 * when that page became three *noun* tabs (专家 / 技能 / 连接器) the registry had
 * no home left in it, and it is host-level configuration anyway — `market/add`
 * writes the host's own registry and removing a market cascades into
 * uninstalling the components that came from it, which is the same judgement
 * `16` §6 applies to `config/*`.
 *
 * The panel owns its own state, client calls and error banner; this file only
 * gives it a place in the dialog.
 */

import { useTranslation } from "react-i18next";

import { MarketSourcesPanel } from "../catalog/MarketSourcesPanel";

export function MarketSettingsSection() {
  const { t } = useTranslation();
  return (
    <>
      <h2 className="settings-group-title">{t("settings.marketGroup")}</h2>
      <MarketSourcesPanel />
    </>
  );
}
