import { Link, useParams } from "react-router";
import { useTranslation } from "react-i18next";

import type { Language } from "../i18n";
import { DOC_ORDER } from "../lib/docs";
import logoUrl from "../assets/logo.png";
import { githubUrl, releasesPageUrl } from "../lib/platform";

export default function Footer({ lang }: { lang: Language }) {
  const { t } = useTranslation();
  const { lang: raw } = useParams();
  const current: Language = raw === "en-US" ? "en-US" : "zh-CN";
  void lang;

  return (
    <footer className="footer">
      <div className="footer-inner">
        <div className="footer-brand">
          <img className="brand-mark" src={logoUrl} alt="" width={28} height={28} />
          <div>
            <div className="brand-name">Flowy Agent Store</div>
            <p className="footer-tagline">{t("footer.tagline")}</p>
          </div>
        </div>
        <div className="footer-cols">
          <nav className="footer-col" aria-label={t("footer.docs")}>
            <h4>{t("footer.docs")}</h4>
            {DOC_ORDER.map(({ slug, sectionKey }) => (
              <Link key={slug} to={`/${current}/docs/${slug}`}>
                {t(`docs.sections.${sectionKey}`)}
              </Link>
            ))}
          </nav>
          <nav className="footer-col" aria-label={t("footer.resources")}>
            <h4>{t("footer.resources")}</h4>
            <Link to={`/${current}/market`}>{t("nav.market")}</Link>
            <a href={releasesPageUrl()} target="_blank" rel="noreferrer">
              {t("footer.releases")}
            </a>
            <a href={githubUrl()} target="_blank" rel="noreferrer">
              {t("footer.github")}
            </a>
          </nav>
        </div>
      </div>
      <div className="footer-bottom">
        <p>{t("footer.copyright")}</p>
      </div>
    </footer>
  );
}
