import type { Config } from "@react-router/dev/config";
import { docSlugs } from "./app/lib/docs";

/**
 * React Router v8 framework mode — static site generation.
 *
 * Loaders run at build time to prerender each route (including docs content and
 * per-locale language), producing static HTML in `build/client/`. We deploy only
 * `build/client/` to GitHub Pages; the server bundle stays unused.
 *
 * `ssr` is left enabled (the default) so route `loader`s are allowed — required
 * for data-driven prerendering. Setting `ssr: false` would forbid `loader`s.
 */
const LOCALES = ["zh-CN", "en-US"] as const;

export default {
  async prerender() {
    const paths: string[] = ["/", "/market"];
    for (const lang of LOCALES) {
      paths.push(`/${lang}`, `/${lang}/market`, `/${lang}/docs`);
      for (const slug of docSlugs(lang)) {
        paths.push(`/${lang}/docs/${slug}`);
      }
    }
    return paths;
  },
} satisfies Config;
