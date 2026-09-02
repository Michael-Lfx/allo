import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Standalone Web UI for the Agent Store App Server protocol.
// Uses the same major versions as the main `ui/` workspace (Vite 6, React 19).
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5174,
    watch: {
      // Ignore editor temp-staging dirs (e.g. `.file.ts.1234.xyz.tmpdir/`) so a
      // locked temp file during an in-place write can never crash the watcher.
      ignored: ['**/.*.tmpdir/**'],
    },
  },
});