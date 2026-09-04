import { useTranslation } from "react-i18next";
import type { CSSProperties } from "react";

const ITEMS = ["q1", "q2", "q3", "q4"] as const;

/** Trust / FAQ — four questions on local-first boundary, platforms, auth, import. */
export default function FaqSection() {
  const { t } = useTranslation();

  return (
    <section className="faq" id="faq">
      <div className="section-inner">
        <p className="eyebrow" data-reveal>
          <span className="eyebrow-index">05</span>
          {t("landing.eyebrow")}
        </p>
        <h2 data-reveal>{t("landing.faq.title")}</h2>

        <div className="faq-list">
          {ITEMS.map((item, i) => (
            <details
              className="faq-item"
              key={item}
              data-reveal
              style={{ "--reveal-delay": `${i * 70}ms` } as CSSProperties}
              open={i === 0}
            >
              <summary>{t(`landing.faq.items.${item}.q`)}</summary>
              <p>{t(`landing.faq.items.${item}.a`)}</p>
            </details>
          ))}
        </div>
      </div>
    </section>
  );
}
