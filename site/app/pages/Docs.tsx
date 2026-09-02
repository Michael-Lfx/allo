import { Link, useParams, useLoaderData, type LoaderFunctionArgs } from "react-router";
import { useTranslation } from "react-i18next";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";

import { applyLanguage, type Language } from "../i18n";
import { DOC_ORDER, loadDoc } from "../lib/docs";

export async function loader({ params }: LoaderFunctionArgs) {
  const lang: Language = params.lang === "en-US" ? "en-US" : "zh-CN";
  applyLanguage(lang);
  const doc = loadDoc(lang, params.slug ?? "");
  if (!doc) throw new Response("Not Found", { status: 404 });
  return { lang, slug: params.slug, ...doc };
}

export default function Docs() {
  const { lang: raw, slug } = useParams();
  const lang: Language = raw === "en-US" ? "en-US" : "zh-CN";
  const { t } = useTranslation();
  const data = useLoaderData() as { title: string; body: string };

  return (
    <div className="docs docs-detail">
      <aside className="docs-sidebar" aria-label={t("docs.title")}>
        <Link className="docs-back" to={`/${lang}/docs`}>
          {t("docs.backToDocs")}
        </Link>
        <nav className="docs-toc">
          {DOC_ORDER.map(({ slug: s, sectionKey }) => (
            <Link key={s} to={`/${lang}/docs/${s}`} className={s === slug ? "is-active" : ""}>
              {t(`docs.sections.${sectionKey}`)}
            </Link>
          ))}
        </nav>
      </aside>

      <article className="docs-content markdown-body">
        <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
          {data.body}
        </ReactMarkdown>
      </article>
    </div>
  );
}
