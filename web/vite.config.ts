import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Standalone Web UI for the Agent Store App Server protocol.
// Uses the same major versions as the main `ui/` workspace (Vite 6, React 19).
//
// The App Server returns root-relative asset URLs (avatars / market icons,
// e.g. `/api/app-server/imports/<snap>/assets/avatars/team.png`). Without a
// proxy Vite's SPA fallback answers those `<img>` requests with index.html
// (text/html), so every avatar renders as a broken image. Forward `/api`
// (including the app-server WebSocket) to the backend.
const BACKEND_ORIGIN = process.env.DSH_BACKEND_ORIGIN ?? 'http://127.0.0.1:8787';

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5174,
    proxy: {
      '/api': {
        target: BACKEND_ORIGIN,
        changeOrigin: true,
        ws: true,
      },
    },
    watch: {
      // Ignore editor temp-staging dirs (e.g. `.file.ts.1234.xyz.tmpdir/`) so a
      // locked temp file during an in-place write can never crash the watcher.
      //
      // Also ignore the vendored runtime binary. `publish-packages.ts` copies a
      // ~181 MiB `flowy-agent-store.exe` in there — dry runs included — and
      // Windows locks the file mid-write, so the watcher dies with
      // `EBUSY: resource busy or locked, watch '…/runtime/vendor/…exe'`, which is
      // an unhandled FSWatcher error and takes the whole dev server down with it.
      // Nothing in the app imports from this directory: it is release payload.
      ignored: ['**/.*.tmpdir/**', '**/packages/runtime/vendor/**'],
    },
  },
});