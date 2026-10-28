

type WebuiQrStatusLike = {
  running?: boolean;
  allowRemote?: boolean;
  localUrl?: string;
  networkUrl?: string;
  networkUrls?: string[];
  port?: number;
};

const normalizeBaseUrl = (url: string): string => url.trim().replace(/\/+$/, '');

const parseIpv4Host = (host: string): number[] | null => {
  const parts = host.split('.');
  if (parts.length !== 4) return null;
  const octets = parts.map((part) => {
    if (!/^\d+$/.test(part)) return Number.NaN;
    return Number(part);
  });
  return octets.every((octet) => Number.isInteger(octet) && octet >= 0 && octet <= 255) ? octets : null;
};

const isUsableRemoteHost = (host: string): boolean => {
  const normalized = host.trim().toLowerCase();
  if (!normalized || normalized === 'localhost') return false;

  const ipv4 = parseIpv4Host(normalized);
  if (!ipv4) return true;

  const [a, b, c] = ipv4;
  if (a === 0 || a === 127) return false;
  if (a === 169 && b === 254) return false;
  if (a === 198 && (b === 18 || b === 19)) return false;
  if (a === 192 && b === 0 && c === 2) return false;
  if (a === 198 && b === 51 && c === 100) return false;
  if (a === 203 && b === 0 && c === 113) return false;
  if (a >= 224) return false;
  return true;
};

const isUsableRemoteUrl = (url: string): boolean => {
  try {
    return isUsableRemoteHost(new URL(url).hostname);
  } catch {
    return true;
  }
};

const pushUniqueUrl = (urls: string[], url?: string | null, remote = false) => {
  if (!url) return;
  const normalized = normalizeBaseUrl(url);
  if (remote && !isUsableRemoteUrl(normalized)) return;
  if (normalized && !urls.includes(normalized)) {
    urls.push(normalized);
  }
};

export const getWebuiQrBaseUrls = (
  status: WebuiQrStatusLike | null | undefined,
  accessUrls: string[],
  fallbackPort: number
): string[] => {
  const remoteUrls: string[] = [];
  for (const url of accessUrls) pushUniqueUrl(remoteUrls, url, true);
  for (const url of status?.networkUrls ?? []) pushUniqueUrl(remoteUrls, url, true);
  pushUniqueUrl(remoteUrls, status?.networkUrl, true);

  if (status?.allowRemote && remoteUrls.length > 0) {
    return remoteUrls;
  }

  const localUrls: string[] = [];
  pushUniqueUrl(localUrls, status?.localUrl);
  if (localUrls.length > 0) return localUrls;
  return [`http://localhost:${status?.port ?? fallbackPort}`];
};

export const buildWebuiQrLoginUrl = (baseUrl: string, token: string): string =>
  `${normalizeBaseUrl(baseUrl)}/qr-login?token=${encodeURIComponent(token)}`;

/**
 * Lower is better for a phone on the same Wi-Fi.
 * 172.16.0.0/12 stays available, but on a Windows desktop it is usually Hyper-V,
 * WSL, or Docker, which a phone cannot open.
 */
const phoneBrowserRank = (url: string): number => {
  let host = '';
  try {
    host = new URL(url).hostname;
  } catch {
    return 9;
  }
  const ipv4 = parseIpv4Host(host);
  if (!ipv4) return 5;
  const [a, b] = ipv4;
  if (a === 192 && b === 168) return 0;
  if (a === 10) return 1;
  if (a === 172 && b >= 16 && b <= 31) return 3;
  return 2;
};

export const isVirtualAdapterUrl = (url: string): boolean => phoneBrowserRank(url) === 3;

/** LAN bases a phone browser can open. Never falls back to localhost. */
export const listBrowserScanBaseUrls = (
  status: WebuiQrStatusLike | null | undefined,
  accessUrls: string[]
): string[] => {
  if (!status?.running || status.allowRemote === false) return [];
  const ranked = getWebuiQrBaseUrls({ ...status, allowRemote: true }, accessUrls, status.port ?? 25808)
    .filter((url) => isUsableRemoteUrl(url))
    .sort((left, right) => phoneBrowserRank(left) - phoneBrowserRank(right));
  const reachable = ranked.filter((url) => !isVirtualAdapterUrl(url));
  return reachable.length > 0 ? reachable : ranked;
};
