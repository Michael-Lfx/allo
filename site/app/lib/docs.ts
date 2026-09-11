import type { Language } from "../i18n";

/**
 * Build-time Markdown loading for the Docs section.
 *
 * `import.meta.glob` with `eager` + `?raw` inlines every doc file into the
 * bundle, so the route `loader` (and therefore SSG) can read content without a
 * network request. Keys look like `../../content/docs/zh-CN/quick-start.md`.
 */
const rawDocs = import.meta.glob("../../content/docs/**/*.md", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const LOCALE_PREFIX = "../../content/docs/";

function keyFor(locale: Language, slug: string): string {
  return `${LOCALE_PREFIX}${locale}/${slug}.md`;
}

/** Slugs available for a locale (falls back to zh-CN if a locale is empty). */
export function docSlugs(locale: Language): string[] {
  const slugs = new Set<string>();
  for (const key of Object.keys(rawDocs)) {
    const m = key.match(new RegExp(`^${LOCALE_PREFIX}(${locale}|zh-CN)/(.+)\\.md$`));
    if (m) slugs.add(m[2]);
  }
  return [...slugs].sort();
}

function parseTitle(body: string): string {
  const line = body.split("\n").find((l) => l.startsWith("# "));
  return line ? line.replace(/^#\s+/, "").trim() : "";
}

export interface DocContent {
  title: string;
  body: string;
}

export function loadDoc(locale: Language, slug: string): DocContent | null {
  const direct = rawDocs[keyFor(locale, slug)];
  const body = direct ?? rawDocs[keyFor("zh-CN", slug)];
  if (body == null) return null;
  return { title: parseTitle(body) || slug, body };
}

/** Stable ordering of docs for the sidebar, shared across locales. */
export const DOC_ORDER: { slug: string; sectionKey: keyof DocSections }[] = [
  { slug: "quick-start", sectionKey: "quickStart" },
  { slug: "cli", sectionKey: "cli" },
  { slug: "plugins-market", sectionKey: "pluginsMarket" },
  { slug: "typescript-sdk", sectionKey: "typescriptSdk" },
  { slug: "upgrade", sectionKey: "upgrade" },
  { slug: "changelog", sectionKey: "changelog" },
  { slug: "configuration", sectionKey: "configuration" },
  { slug: "architecture", sectionKey: "architecture" },
  { slug: "compatibility", sectionKey: "compatibility" },
];

export interface DocSections {
  quickStart: string;
  cli: string;
  pluginsMarket: string;
  typescriptSdk: string;
  upgrade: string;
  changelog: string;
  configuration: string;
  architecture: string;
  compatibility: string;
}