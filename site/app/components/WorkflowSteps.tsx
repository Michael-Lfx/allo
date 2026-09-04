import { useTranslation } from "react-i18next";
import { ArrowRight, PackageOpen, PlayCircle, SquarePlus } from "lucide-react";

import { revealDelay } from "../lib/effects";
import CopyButton from "./CopyButton";

interface Step {
  title: string;
  desc: string;
  cmd: string;
}

const STEP_ICONS = [PlayCircle, SquarePlus, PackageOpen];

export default function WorkflowSteps() {
  const { t } = useTranslation();
  const steps = t("landing.workflow", { returnObjects: true }) as Record<string, Step>;
  const list = Object.values(steps);

  return (
    <section className="workflow" id="workflow">
      <div className="section-inner">
        <p className="eyebrow" data-reveal>
          <span className="eyebrow-index">02</span>
          {t("landing.eyebrow")}
        </p>
        <h2 data-reveal>{t("landing.workflowTitle")}</h2>
        <p className="subtle" data-reveal>{t("landing.workflowSubtitle")}</p>

        <ol className="steps">
          {list.map((step, i) => {
            const Icon = STEP_ICONS[i] ?? PlayCircle;
            return (
              <li className="step" data-reveal style={revealDelay(i * 110)} key={step.title}>
                <div className="step-head">
                  <span className="step-no" aria-hidden="true">
                    <Icon size={15} />
                  </span>
                  <div>
                    <h3>
                      <span className="step-index">{i + 1}</span>
                      {step.title}
                    </h3>
                    <p>{step.desc}</p>
                  </div>
                </div>
                <div className="step-cmd">
                  <code>{step.cmd}</code>
                  <CopyButton text={step.cmd} label={t("landing.download.copy")} tone="dark" />
                </div>
                {i < list.length - 1 && (
                  <ArrowRight className="step-arrow" size={18} aria-hidden="true" />
                )}
              </li>
            );
          })}
        </ol>
      </div>
    </section>
  );
}
