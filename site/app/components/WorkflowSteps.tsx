import { useTranslation } from "react-i18next";
import { ArrowRight } from "lucide-react";

import CopyButton from "./CopyButton";

interface Step {
  title: string;
  desc: string;
  cmd: string;
}

export default function WorkflowSteps() {
  const { t } = useTranslation();
  const steps = (t("landing.workflow", { returnObjects: true }) as Record<string, Step>);

  return (
    <section className="workflow" id="workflow">
      <div className="section-inner">
        <p className="eyebrow">
          <span className="eyebrow-index">02</span>
          {t("landing.eyebrow")}
        </p>
        <h2>{t("landing.workflowTitle")}</h2>
        <p className="subtle">{t("landing.workflowSubtitle")}</p>

        <ol className="steps">
          {Object.values(steps).map((step, i) => (
            <li className="step" key={step.title}>
              <div className="step-head">
                <span className="step-no" aria-hidden="true">
                  {i + 1}
                </span>
                <div>
                  <h3>{step.title}</h3>
                  <p>{step.desc}</p>
                </div>
              </div>
              <div className="step-cmd">
                <code>{step.cmd}</code>
                <CopyButton text={step.cmd} label={t("landing.download.copy")} tone="dark" />
              </div>
              {i < Object.values(steps).length - 1 && (
                <ArrowRight className="step-arrow" size={18} aria-hidden="true" />
              )}
            </li>
          ))}
        </ol>
      </div>
    </section>
  );
}
