# Flowy Agent Store — Official Site

Marketing + docs site for **Flowy Agent Store**: a local-first, single-file agent
runtime that embeds a Web UI and is launched from the command line.

Built with **React Router v8 (framework mode)** + Vite + React 19 + TypeScript.
Static HTML is generated at build time via React Router's built-in SSG
(`ssr: false` + `prerender` in `react-router.config.ts`), so the site is pure
static files — no server required. Publishing is **manual** today: see
[Deploy](#deploy) before assuming a merge reaches production.

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

## Content sources

1. **Source repo** — `app/lib/platform.ts` `GITHUB_REPO` (`Michael-Lfx/allo`) is
   the **source** repository; release binaries are not published there. They ship
   from the VPS download host (`DOWNLOAD_HOST`, currently a bare-IP HTTP origin).
2. **Download links** — `RELEASED_PLATFORMS` in the same file is the single source
   of truth for what has actually shipped: only released platforms (Windows x64
   today) get a direct asset URL (`flowy-agent-store-<tag>-<os>-<arch>.zip`);
   every other platform is routed to the download center instead of a 404. Keep it
   consistent with `content/docs/{zh-CN,en-US}/compatibility.md`.
3. **CLI name** — docs use `flowy-agent-store` as the runtime command; adjust if
   the distributable is named differently.

## Deploy

`.github/workflows/deploy-site.yml` builds with `BASE_PATH` derived from the
repository name and publishes `site/build/client` to GitHub Pages. It also copies
`index.html` → `404.html` so unknown deep links fall back to the SPA shell.

**Trigger: manual only.** The workflow's `push:` trigger is commented out, so it
runs solely on `workflow_dispatch` (Actions → “Deploy site” → Run workflow).
Merging to `main` does **not** publish the site.

Known follow-up (undecided): the final hosting entry point and its domain/HTTPS.
This is the same open question as the bare-IP HTTP download origin, which is what
makes security tooling wary of the `irm … | iex` one-liner — see
`docs/agent-store/16-sdk-webui-site-priority-plan.zh.md` (Q2 / C5).

## Layout

```
app/            React Router app (root, routes, layouts, pages, components, i18n, lib)
content/docs/   Markdown docs, zh-CN + en-US (kept in sync with docs/agent-store)
react-router.config.ts   ssr:false + prerender
vite.config.ts          base = BASE_PATH
```
