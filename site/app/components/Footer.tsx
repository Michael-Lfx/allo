import { Link, useParams } from "react-router";
import { useTranslation } from "react-i18next";

import type { Language } from "../i18n";
import { githubUrl, releasesPageUrl } from "../lib/platform";

export default function Footer({ lang }: { lang: Language }) {
  const { t } = useTranslation();
  const { lang: raw } = useParams();
  const current: Language = raw === "en-US" ? "en-US" : "zh-CN";
  const docsHref = `/${current}/docs`;

  return (
    <footer className="footer">
      <div className="footer-inner">
        <div className="footer-brand">
          <span className="brand-mark" aria-hidden="true" />
          <div>
            <div className="brand-name">Flowy Agent Store</div>
            <p className="footer-tagline">{t("footer.tagline")}</p>
          </div>
        </div>
        <nav className="footer-links" aria-label="Footer">
          <Link to={docsHref}>{t("footer.docs")}</Link>
          <a href={releasesPageUrl()} target="_blank" rel="noreferrer">
            {t("footer.releases")}
          </a>
          <a href={githubUrl()} target="_blank" rel="noreferrer">
            {t("footer.github")}
          </a>
        </nav>
      </div>
      <p className="footer-copy">{t("footer.copyright")}</p>
    </footer>
  );
}
