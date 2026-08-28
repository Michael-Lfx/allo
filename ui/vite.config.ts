import { defineConfig, type Plugin } from 'vite';
import { readFileSync } from 'node:fs';
import { resolve } from 'path';
import react from '@vitejs/plugin-react';
import UnoCSS from 'unocss/vite';
import tailwindcss from '@tailwindcss/vite';
import unoConfig from './uno.config.ts';
import { createUiBuildManifest } from '../scripts/ui-build-manifest';

const uiPackage = JSON.parse(readFileSync(resolve(__dirname, 'package.json'), 'utf8')) as {
  version: string;
  dependencies?: Record<string, string>;
};
const codeMirrorPackages = [
  '@codemirror/state',
  '@codemirror/view',
  '@codemirror/language',
  '@codemirror/commands',
  '@lezer/common',
] as const;
const rawApiContractVersion = readFileSync(resolve(__dirname, '../ui-api-contract-version.txt'), 'utf8').trim();
const manifest = createUiBuildManifest(uiPackage.version, rawApiContractVersion);

function uiBuildManifestPlugin(): Plugin {
  return {
    name: 'nomifun-ui-build-manifest',
    apply: 'build' as const,
    generateBundle() {
      this.emitFile({
        type: 'asset',
        fileName: 'nomifun-build.json',
        source: `${JSON.stringify(manifest, null, 2)}\n`,
      });
    },
  };
}

// Ported from the original electron.vite.config.ts: rewrites named imports from
// '@icon-park/react' into HOC-wrapped components (replaces the old webpack loader).
function iconParkPlugin() {
  return {
    name: 'vite-plugin-icon-park',
    enforce: 'pre' as const,
    transform(source: string, id: string) {
      if (!id.endsWith('.tsx') || id.includes('node_modules')) return null;
      if (!source.includes('@icon-park/react')) return null;
      const transformed = source.replace(
        /import\s+\{\s*([^}]+)\s*\}\s+from\s+['"]@icon-park\/react['"](;?)/g,
        (str, match: string, semi: string) => {
          if (!match?.trim()) return str;
          const specs = match.split(',').map((part) => part.trim()).filter(Boolean);
          const parsed = specs.map((spec) => {
            const aliasMatch = /^([A-Za-z_][\w]*)\s+as\s+([A-Za-z_][\w]*)$/.exec(spec);
            if (aliasMatch) {
              return { exportName: aliasMatch[1], localName: aliasMatch[2] };
            }
            return { exportName: spec, localName: spec };
          });
          const importList = parsed
            .map(({ exportName, localName }) => `${exportName} as _${localName}`)
            .join(', ');
          const importLine = `import { ${importList} } from '@icon-park/react'${semi || ';'}`;
          const hoc = `import IconParkHOC from '@renderer/components/IconParkHOC';
${parsed.map(({ localName }) => `const ${localName} = IconParkHOC(_${localName})`).join(';\n')}`;
          return `${importLine}${hoc}`;
        }
      );
      return transformed !== source ? { code: transformed, map: null } : null;
    },
  };
}

const src = resolve(__dirname, 'src');
const ocRoot = resolve(src, 'renderer/pages/videoCanvas/oc');
const buildId = manifest.frontend_build_id;
const codeMirrorRuntimeVersions = Object.fromEntries(
  codeMirrorPackages.map((packageName) => [packageName, uiPackage.dependencies?.[packageName] ?? 'unknown'])
);

export default defineConfig(({ mode }) => {
  // WebUI dev mode (`vite --mode webdev`, driven by the UI `dev:web` script and
  // the root `dev:webui` one-click). The SPA is served by Vite (with HMR) but the
  // auth backend + API + WebSocket live in the separate `nomifun-web` host. The
  // browser SPA makes *same-origin relative* calls in WebUI mode (`/api/*`,
  // `/login`, `/logout`, `/ws` — see ui/src/common/adapter/httpBridge.ts), so we
  // proxy that backend surface to nomifun-web. Without this, those calls hit the
  // static dev server and fail at the network layer ("连接失败").
  //
  // Gated on `mode === 'webdev'` so plain `ui:dev` and the Tauri desktop dev
  // server (mode 'development', which talks to the embedded backend via an
  // absolute `window.__backendPort` URL) are completely unaffected.
  const webdev = mode === 'webdev';
  const apiPort = process.env.NOMIFUN_WEB_PORT ?? '8787';
  const apiTarget = `http://127.0.0.1:${apiPort}`;
  const wsTarget = `ws://127.0.0.1:${apiPort}`;
  // Same loopback host on both sides — keep the original Host header so the
  // backend's host-only session cookie maps cleanly back onto 127.0.0.1:5173.
  const httpProxy = { target: apiTarget, changeOrigin: false };
  const proxy = webdev
    ? {
        '/api': httpProxy,
        '/login': httpProxy,
        '/logout': httpProxy,
        '/qr-login': httpProxy,
        '/health': httpProxy,
        '/ws': { target: wsTarget, ws: true, changeOrigin: false },
      }
    : undefined;

  return {
    root: __dirname,
    // Pin the dev server so it always matches the Tauri `devUrl` (5173).
    server: {
      port: 5173,
      strictPort: true,
      host: '127.0.0.1',
      proxy,
    },
    // Tailwind v4 powers the ported open-ai-canvas (oc/) utility classes + globals.css.
    // UnoCSS remains for the rest of allo UI.
    plugins: [iconParkPlugin(), react(), tailwindcss(), UnoCSS({ ...unoConfig }), uiBuildManifestPlugin()],
    define: {
      __NOMI_BUILD_ID__: JSON.stringify(buildId),
      __NOMI_CODEMIRROR_VERSIONS__: JSON.stringify(codeMirrorRuntimeVersions),
    },
    resolve: {
      alias: {
        '@': src,
        '@common': resolve(src, 'common'),
        '@renderer': resolve(src, 'renderer'),
        '@oc': ocRoot,
      },
      // CodeMirror extension values are branded by their module instance.
      // Force every language package and the React wrapper through the same
      // state/view/language copies in dev and packaged builds.
      dedupe: ['@codemirror/state', '@codemirror/view', '@codemirror/language', '@codemirror/commands', '@lezer/common'],
      extensions: ['.ts', '.tsx', '.js', '.json'],
    },
    build: {
      outDir: 'dist',
      emptyOutDir: true,
      reportCompressedSize: false,
      target: 'safari15.5',
      cssTarget: 'safari15.5',
      rollupOptions: {
        output: {
          // A manual chunk otherwise absorbs transitive imports in Rollup 4.
          // Keep shared runtime modules with their natural importers so a
          // canvas-only vendor does not become an app-entry dependency.
          onlyExplicitManualChunks: true,
          manualChunks(id) {
            // Keep video canvas heavy deps out of the first-enter shared graph
            // so /video-generation list can load without leafer/vidstack.
            if (id.includes('node_modules')) {
              if (id.includes('@vidstack') || id.includes('vidstack')) return 'vendor-vidstack';
              if (id.includes('leafer') || id.includes('@leafer-ui') || id.includes('leafer-ui')) {
                return 'vendor-leafer';
              }
            }
            return undefined;
          },
        },
      },
    },
  };
});
