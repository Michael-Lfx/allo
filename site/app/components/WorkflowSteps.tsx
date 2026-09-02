import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowRight, Check, Copy } from "lucide-react";

interface Step {
  title: string;
  desc: string;
  cmd: string;
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      console.error("clipboard copy failed");
    }
  };
  return (
    <button className="copy-btn" onClick={copy} aria-label="Copy command">
      {copied ? <Check size={14} /> : <Copy size={14} />}
    </button>
  );
}

export default function WorkflowSteps() {
  const { t } = useTranslation();
  const steps = (t("landing.workflow", { returnObjects: true }) as Record<string, Step>);

  return (
    <section className="workflow" id="workflow">
      <div className="section-inner">
        <p className="eyebrow">{t("landing.eyebrow")}</p>
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
                <CopyButton text={step.cmd} />
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
