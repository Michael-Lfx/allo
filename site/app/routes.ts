import { type RouteConfig, route } from "@react-router/dev/routes";

export default [
  // The project root `/` renders the default-locale landing directly (no
  // client-side redirect) so dev preview and SSG both have content at `/`.
  // `:lang` is undefined here, so Lang.tsx falls back to zh-CN. The explicit
  // `id` avoids a duplicate-route-id clash with the `:lang` Lang layout below.
  route("/", "./layouts/Lang.tsx", { id: "root-lang" }, [
    route("", "./pages/Landing.tsx", { id: "root-landing" }),
    route("market", "./pages/Market.tsx", { id: "root-market" }),
  ]),
  route(":lang", "./layouts/Lang.tsx", { id: "lang" }, [
    route("", "./pages/Landing.tsx"),
    route("market", "./pages/Market.tsx"),
    route("docs", "./pages/DocsIndex.tsx"),
    route("docs/:slug", "./pages/Docs.tsx"),
  ]),
  // The splat catches unknown deep links and redirects to the default locale.
  route("*", "./HomeRedirect.tsx"),
] satisfies RouteConfig;
