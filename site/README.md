# Flowy Agent Store — Official Site

Marketing + docs site for **Flowy Agent Store**: a local-first, single-file agent
runtime that embeds a Web UI and is launched from the command line.

Built with **React Router v8 (framework mode)** + Vite + React 19 + TypeScript.
Static HTML is generated at build time via React Router's built-in SSG
(`ssr: false` + `prerender` in `react-router.config.ts`), so the site is pure
static files — no server required — and deploys to GitHub Pages.

## Stack

- React Router v8 framework mode (built-in SSG, no extra prerender plugin)
- Vite 8 + React 19 + TypeScript
- `i18next` / `react-i18next` for `zh-CN` / `en-US` (reuses the `web/` pattern)
- `react-markdown` + `remark-gfm` + `rehype-highlight` for docs
- `lucide-react` icons; custom CSS design tokens (no Tailwind/UnoCSS)

## Develop

```bash
bun install
bun run dev          # react-router dev → http://localhost:5173
```

## Build & preview

```bash
bun run build        # outputs site/build/client (static HTML per route)
bun run preview      # serve the production build
bun run typecheck    # tsc --noEmit
```

For GitHub Pages the build needs the repo subpath as the base URL:

```bash
BASE_PATH=/<repo>/ bun run build
```

## Configure before shipping

1. **Binary repo** — edit `app/lib/platform.ts`: set `GITHUB_REPO` to
   `your-org/flowy-agent-store`. The download CTA points here for every platform
   asset (`flowy-agent-store-<tag>-<os>-<arch>.zip`).
2. **CLI name** — docs use `flowy-agent-store` as the runtime command; adjust if
   the distributable is named differently.

## Deploy (GitHub Pages)

`.github/workflows/deploy-site.yml` builds with `BASE_PATH` derived from the
repository name and publishes `site/build/client` to GitHub Pages. It also copies
`index.html` → `404.html` so unknown deep links fall back to the SPA shell.

## Layout

```
app/            React Router app (root, routes, layouts, pages, components, i18n, lib)
content/docs/   Markdown docs, zh-CN + en-US (kept in sync with docs/agent-store)
react-router.config.ts   ssr:false + prerender
vite.config.ts          base = BASE_PATH
```
