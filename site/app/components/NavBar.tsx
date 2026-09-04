import { Link, useLocation, useNavigate } from "react-router";
import { useTranslation } from "react-i18next";
import { Download, Languages } from "lucide-react";

function GitHubMark() {
  return (
    <svg viewBox="0 0 16 16" width={16} height={16} fill="currentColor" aria-hidden="true">
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  );
}

import type { Language } from "../i18n";
import { setLanguage } from "../i18n";
import { githubUrl, releaseAssetUrl } from "../lib/platform";
import logoUrl from "../assets/logo.png";
import ThemeToggle from "./ThemeToggle";

function otherLang(l: Language): Language {
  return l === "zh-CN" ? "en-US" : "zh-CN";
}

export default function NavBar({ lang }: { lang: Language }) {
  const { t } = useTranslation();
  const location = useLocation();
  const navigate = useNavigate();

  const withLang = (path: string) => {
    const clean = path.replace(/^\/(zh-CN|en-US)/, "");
    return `/${lang}${clean === "" ? "" : clean}`;
  };

  const switchLang = () => {
    const target = otherLang(lang);
    // Persist the choice so a later reload of `/` keeps it (loader re-applies
    // it), then navigate to the SAME page under the *target* locale. `withLang`
    // re-prefixes with the current locale, so we must rebuild with `target`.
    setLanguage(target);
    const clean = location.pathname.replace(/^\/(zh-CN|en-US)/, "");
    navigate(`/${target}${clean === "" ? "" : clean}`);
  };

  return (
    <header className="navbar">
      <div className="navbar-inner">
        <Link to={withLang("/")} className="brand" aria-label="Flowy Agent Store">
          <img className="brand-mark" src={logoUrl} alt="" width={24} height={24} />
          <span className="brand-name">Flowy Agent Store</span>
        </Link>
        <nav className="navbar-links" aria-label="Primary">
          <Link
            to={withLang("/market")}
            className={location.pathname.includes("/market") ? "nav-link is-active" : "nav-link"}
          >
            {t("nav.market")}
          </Link>
          <Link
            to={withLang("/docs")}
            className={location.pathname.includes("/docs") ? "nav-link is-active" : "nav-link"}
          >
            {t("nav.docs")}
          </Link>
          <a className="nav-link nav-github" href={githubUrl()} target="_blank" rel="noreferrer">
            <GitHubMark />
            {t("nav.github")}
          </a>
          <a className="btn btn-primary nav-download" href={releaseAssetUrl("latest", { os: "windows", arch: "x86_64" })}>
                      <Download size={16} />
                      {t("nav.download")}
                    </a>
          <button className="icon-btn" onClick={switchLang} aria-label={t("nav.lang")}>
            <Languages size={18} />
          </button>
          <ThemeToggle />
        </nav>
      </div>
    </header>
  );
}
