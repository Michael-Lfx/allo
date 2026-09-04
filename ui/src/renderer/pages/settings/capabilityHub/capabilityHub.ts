/**
 * Capability hub routing — presets, skills, MCP, and plugins share one chrome.
 * Market is the default view for capability hubs that expose one;
 * `?view=installed` shows the local library. Presets are library-only.
 */

export const CAPABILITY_HUB_IDS = ['presets', 'skills', 'mcp', 'plugins'] as const;

export type CapabilityHubId = (typeof CAPABILITY_HUB_IDS)[number];

export type CapabilityHubView = 'market' | 'installed';

export const CAPABILITY_HUB_ACTIVE_PATHS: CapabilityHubId[] = [...CAPABILITY_HUB_IDS];

export const isCapabilityHubId = (value: string | null | undefined): value is CapabilityHubId =>
  value === 'presets' || value === 'skills' || value === 'mcp' || value === 'plugins';

export const parseCapabilityHubFromPathname = (pathname: string): CapabilityHubId | null => {
  const match = pathname.match(/^(?:\/settings)?\/(presets|skills|mcp|plugins)(?:\/|$)/);
  return match && isCapabilityHubId(match[1]) ? match[1] : null;
};

export const isSettingsCapabilityPathname = (pathname: string): boolean => pathname.startsWith('/settings/');

type LegacyTabTarget = CapabilityHubView | 'plugins-installed' | 'plugins-market';

export const mapLegacyCapabilityTab = (tab: string | null): LegacyTabTarget | null => {
  if (tab === 'library' || tab === 'servers') return 'installed';
  if (tab === 'market') return 'market';
  if (tab === 'plugins') return 'plugins-installed';
  if (tab === 'plugin-market') return 'plugins-market';
  return null;
};

export const parseCapabilityHubView = (searchParams: URLSearchParams): CapabilityHubView => {
  if (searchParams.get('view') === 'installed') return 'installed';
  const legacy = mapLegacyCapabilityTab(searchParams.get('tab'));
  if (legacy === 'installed' || legacy === 'plugins-installed') return 'installed';
  if (searchParams.get('highlight')) return 'installed';
  return 'market';
};

const hubPath = (hub: CapabilityHubId, inSettings: boolean): string =>
  inSettings ? `/settings/${hub}` : `/${hub}`;

const withSearch = (path: string, params: URLSearchParams): string => {
  const search = params.toString();
  return search ? `${path}?${search}` : path;
};

export const buildCapabilityHubLocation = (options: {
  hub: CapabilityHubId;
  inSettings: boolean;
  view?: CapabilityHubView;
  highlight?: string | null;
  params?: URLSearchParams;
}): string => {
  const params = new URLSearchParams(options.params);
  params.delete('tab');
  params.delete('view');
  params.delete('highlight');
  // Presets are always the local library, so keep their public URL stable and
  // avoid exposing an implementation-only view query for the retired market.
  if (options.hub !== 'presets' && options.view === 'installed') params.set('view', 'installed');
  if (options.highlight) params.set('highlight', options.highlight);
  return withSearch(hubPath(options.hub, options.inSettings), params);
};

const normalizeSearch = (search: string): string => {
  if (!search) return '';
  return search.startsWith('?') ? search : `?${search}`;
};

/**
 * Rewrites legacy `?tab=` deep links and forces installed view when `highlight`
 * is present. Returns null when the current location is already canonical.
 */
export const resolveLegacyCapabilityLocation = (pathname: string, search: string): string | null => {
  const hub = parseCapabilityHubFromPathname(pathname);
  if (!hub) return null;

  const inSettings = isSettingsCapabilityPathname(pathname);
  const raw = search.startsWith('?') ? search.slice(1) : search;
  const params = new URLSearchParams(raw);
  const tab = params.get('tab');
  const highlight = params.get('highlight');
  const legacy = mapLegacyCapabilityTab(tab);

  let nextHub = hub;
  let view: CapabilityHubView = params.get('view') === 'installed' ? 'installed' : 'market';
  let changed = false;

  if (legacy === 'plugins-installed' || legacy === 'plugins-market') {
    nextHub = 'plugins';
    view = legacy === 'plugins-installed' ? 'installed' : 'market';
    changed = true;
  } else if (legacy === 'installed' || legacy === 'market') {
    view = legacy;
    changed = true;
  }

  if (highlight && view !== 'installed') {
    view = 'installed';
    changed = true;
  }

  if (tab !== null) {
    params.delete('tab');
    changed = true;
  }

  // Presets no longer expose a remote expert-package market. Keep old links
  // navigable, but canonicalize every market view to the local library.
  if (hub === 'presets' && params.has('view')) {
    params.delete('view');
    view = 'installed';
    changed = true;
  }

  if (!changed) return null;

  const next = buildCapabilityHubLocation({
    hub: nextHub,
    inSettings,
    view,
    highlight,
    params,
  });
  const current = `${pathname}${normalizeSearch(search)}`;
  return next === current ? null : next;
};
