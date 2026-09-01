const DEFAULT_REQUEST_TIMEOUT_MS = 10_000;
const DEFAULT_READY_PATHS = [
  '/',
  '/src/renderer/pages/guid/index.tsx',
  '/src/renderer/pages/conversation/index.tsx',
];

function formatHost(host) {
  return host.includes(':') && !host.startsWith('[') ? `[${host}]` : host;
}

function fetchWithTimeout(fetchImpl, url, timeoutMs, expectModule) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);
  return Promise.resolve()
    .then(() => fetchImpl(url, { signal: controller.signal }))
    .then(async (response) => {
      if (!response?.ok) return false;
      const body = typeof response.text === 'function' ? await response.text() : '';
      if (!expectModule) return true;
      const contentType = response.headers?.get?.('content-type')?.toLowerCase() ?? '';
      if (contentType.includes('text/html') || /^\s*<!doctype\s+html|^\s*<html\b/i.test(body)) return false;
      return true;
    })
    .catch(() => false)
    .finally(() => clearTimeout(timeout));
}

/**
 * Create a bounded readiness probe for the Vite dev server. The root document
 * proves the server is serving the app shell; the representative route modules
 * catch the more subtle state where a port is open but a lazy import still
 * fails during the first transform. Keep both the sidebar guide route and the
 * main conversation route because they are independently lazy-loaded.
 */
export function createViteHttpReadinessProbe({
  host,
  port,
  fetchImpl = globalThis.fetch,
  paths = DEFAULT_READY_PATHS,
  requestTimeoutMs = DEFAULT_REQUEST_TIMEOUT_MS,
}) {
  if (typeof fetchImpl !== 'function') {
    throw new TypeError('A fetch implementation is required for Vite readiness checks');
  }
  const baseUrl = `http://${formatHost(host)}:${port}`;

  return async function isViteReady() {
    for (const path of paths) {
      if (!(await fetchWithTimeout(fetchImpl, `${baseUrl}${path}`, requestTimeoutMs, path !== '/'))) return false;
    }
    return true;
  };
}
