import { reactRouter } from "@react-router/dev/vite";
import { defineConfig } from "vite";

// Static site for GitHub Pages. `BASE_PATH` lets CI point assets at the project
// subpath (e.g. `/flowy-agent-store/`) without touching source. Local dev/build
// defaults to `/` so `react-router dev` / `preview` work at the domain root.
export default defineConfig({
  base: process.env.BASE_PATH ?? "/",
  plugins: [reactRouter()],
});
