import { Link, useParams } from "react-router";
import { useTranslation } from "react-i18next";
import type { CSSProperties } from "react";

import { revealDelay } from "../lib/effects";
import CopyButton from "./CopyButton";

const SDK_SNIPPET = `import { launchClient } from "@flowy-agent-store/sdk";

const session = await launchClient({
  client: { name: "my-app", version: "0.1.0" },
});

const items = await session.client.listStore();
const receipt = await session.client.runs.agent({
  agentId: "frontend-backend-experts",
  goal: "Generate a todo REST API",
});

await session.close();`;

const PACKAGES = ["pkgProtocol", "pkgClient", "pkgSdk"] as const;

/** TypeScript SDK section: three package cards + real quick-start snippet. */
export default function DevSection() {
  const { t } = useTranslation();
  const { lang: raw } = useParams();
  const lang = raw === "en-US" ? "en-US" : "zh-CN";

  return (
    <section className="dev" id="developers">
      <div className="section-inner">
        <p className="eyebrow" data-reveal>
          <span className="eyebrow-index">04</span>
          {t("landing.eyebrow")}
        </p>
        <h2 data-reveal>{t("landing.dev.title")}</h2>
        <p className="subtle" data-reveal>
          {t("landing.dev.subtitle")}
        </p>

        <div className="dev-grid">
          <div className="dev-cards">
            {PACKAGES.map((pkg, i) => (
              <article
                className="dev-card"
                key={pkg}
                data-reveal
                style={revealDelay(i * 90)}
              >
                <code className="dev-pkg-name">{t(`landing.dev.${pkg}.name`)}</code>
                <p>{t(`landing.dev.${pkg}.desc`)}</p>
              </article>
            ))}
            <Link className="dev-link" to={`/${lang}/docs/typescript-sdk`} data-reveal>
              {t("landing.dev.cta")} →
            </Link>
          </div>

          <figure className="dev-code" data-reveal style={revealDelay(150)}>
            <figcaption className="term-head">
              <span className="term-dots" aria-hidden="true">
                <i />
                <i />
                <i />
              </span>
              <span className="term-title">{t("landing.dev.codeTitle")}</span>
              <span className="term-copy">
                <CopyButton text={SDK_SNIPPET} label={t("landing.download.copy")} tone="dark" />
              </span>
            </figcaption>
            <pre className="term-body dev-code-body">
              <code>{SDK_SNIPPET}</code>
            </pre>
          </figure>
        </div>
      </div>
    </section>
  );
}
