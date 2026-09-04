import { useTranslation } from "react-i18next";
import { Activity, Boxes, ListTree, Package, Terminal } from "lucide-react";

import { revealDelay } from "../lib/effects";

const DAG_NODES = ["plan", "explore", "code", "review"];

/**
 * CSS mock of the workbench: sidebar nav + run timeline + plan DAG.
 * Uses the real product vocabulary (catalog / runs / artifacts, DAG,
 * browser.open, todo-api.ts) so the preview matches what users get.
 */
export default function WorkbenchShowcase() {
  const { t } = useTranslation();

  return (
    <section className="showcase" aria-label={t("landing.showcase.label")}>
      <div className="showcase-inner">
        <p className="eyebrow eyebrow-center" data-reveal>
          {t("landing.showcase.label")}
        </p>
        <div className="showcase-stage">
          <div className="showcase-glow" aria-hidden="true" />
          <figure className="wb-window" data-reveal style={revealDelay(120)}>
            <figcaption className="wb-chrome">
              <span className="term-dots" aria-hidden="true">
                <i />
                <i />
                <i />
              </span>
              <span className="wb-title">{t("landing.showcase.runTitle")}</span>
              <span className="wb-status" aria-hidden="true">
                <i className="wb-status-dot" />
                {t("landing.showcase.status")}
              </span>
            </figcaption>
            <div className="wb-body">
              <div className="wb-side">
                <span className="wb-side-item is-active">
                  <Boxes size={15} aria-hidden="true" />
                  {t("landing.showcase.navCatalog")}
                </span>
                <span className="wb-side-item">
                  <Activity size={15} aria-hidden="true" />
                  {t("landing.showcase.navRuns")}
                </span>
                <span className="wb-side-item">
                  <Package size={15} aria-hidden="true" />
                  {t("landing.showcase.navArtifacts")}
                </span>
              </div>
              <div className="wb-main">
                <div className="wb-tl">
                  <p className="wb-tl-item">
                    <ListTree className="wb-tl-icon is-plan" size={16} aria-hidden="true" />
                    {t("landing.showcase.tlPlan")}
                  </p>
                  <p className="wb-tl-item">
                    <Terminal className="wb-tl-icon is-tool" size={16} aria-hidden="true" />
                    {t("landing.showcase.tlTool")}
                  </p>
                  <p className="wb-tl-item">
                    <Package className="wb-tl-icon is-art" size={16} aria-hidden="true" />
                    {t("landing.showcase.tlArt")}
                  </p>
                </div>
                <div className="wb-dag" aria-hidden="true">
                  {DAG_NODES.map((node, i) => (
                    <span key={node} style={revealDelay(700 + i * 110)} className="wb-dag-node">
                      {node}
                    </span>
                  )).flatMap((el, i) =>
                    i === 0 ? [el] : [<span key={`a${i}`} className="wb-dag-arrow">→</span>, el],
                  )}
                </div>
              </div>
            </div>
          </figure>
        </div>
      </div>
    </section>
  );
}
