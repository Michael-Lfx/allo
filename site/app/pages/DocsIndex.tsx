import { Link, useParams } from "react-router";
import { useTranslation } from "react-i18next";
import { ArrowRight } from "lucide-react";

import type { Language } from "../i18n";
import { DOC_ORDER } from "../lib/docs";

export default function DocsIndex() {
  const { lang: raw } = useParams();
  const lang: Language = raw === "en-US" ? "en-US" : "zh-CN";
  const { t } = useTranslation();

  return (
    <div className="docs">
      <div className="docs-inner">
        <header className="docs-header">
          <p className="eyebrow">{t("landing.eyebrow")}</p>
          <h1>{t("docs.title")}</h1>
          <p className="subtle">{t("docs.subtitle")}</p>
        </header>

        <ul className="doc-list">
          {DOC_ORDER.map(({ slug, sectionKey }) => (
            <li key={slug}>
              <Link className="doc-row" to={`/${lang}/docs/${slug}`}>
                <span className="doc-row-title">{t(`docs.sections.${sectionKey}`)}</span>
                <ArrowRight size={16} className="doc-row-arrow" aria-hidden="true" />
              </Link>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
