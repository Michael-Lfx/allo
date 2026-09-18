/**
 * Campaign marketing helpers — click routing, date labels, HTML sanitise.
 * Keep this module free of React so it can be unit-tested.
 */

import type { CampaignCarouselItem, TvShowVideo } from './types';

export type CampaignCarouselAction = 'detail' | 'link' | 'none';

export function campaignCarouselAction(
  item: Pick<CampaignCarouselItem, 'showInList' | 'linkUrl'>
): CampaignCarouselAction {
  if (item.showInList) return 'detail';
  if (item.linkUrl?.trim()) return 'link';
  return 'none';
}

export function isHttpUrl(value: string): boolean {
  return /^https?:\/\//i.test(value.trim());
}

/** In-app path such as `/video-generation` or `#/video-generation`. */
export function isInAppCampaignPath(value: string): boolean {
  const trimmed = value.trim();
  if (!trimmed) return false;
  if (trimmed.startsWith('#/')) return true;
  if (trimmed.startsWith('/video-generation')) return true;
  return false;
}

export function inAppNavigatePath(value: string): string {
  const trimmed = value.trim();
  if (trimmed.startsWith('#/')) return trimmed.slice(1);
  return trimmed;
}

export type TvShowScope = 'plaza' | 'campaign' | 'mine';

export const TV_SHOW_CHANNELS = [
  'all',
  'idea2video',
  'script2video',
  'novel2video',
  'canvas',
  'action2video',
] as const;

export type TvShowChannel = (typeof TV_SHOW_CHANNELS)[number];
export type TvShowTab = TvShowChannel | 'campaign' | 'mine';
export type TvShowListSort = 'latest' | 'likes';

export function tvShowSortParam(sort: TvShowListSort): 'publishedAtDesc' | 'likeCountDesc' {
  return sort === 'likes' ? 'likeCountDesc' : 'publishedAtDesc';
}

/** Plaza is the default (no `tvScope`); campaign/mine are explicit. */
export function parseTvShowScope(raw: string | null | undefined): TvShowScope {
  if (raw === 'campaign' || raw === 'mine') return raw;
  return 'plaza';
}

export function writeTvShowScope(params: URLSearchParams, scope: TvShowScope): void {
  if (scope === 'plaza') params.delete('tvScope');
  else params.set('tvScope', scope);
}

export function parseTvShowChannel(raw: string | null | undefined): TvShowChannel {
  return TV_SHOW_CHANNELS.includes(raw as TvShowChannel) ? (raw as TvShowChannel) : 'all';
}

export function writeTvShowChannel(params: URLSearchParams, channel: TvShowChannel): void {
  if (channel === 'all') params.delete('tvChannel');
  else params.set('tvChannel', channel);
}

export function parseTvShowTab(
  scopeRaw: string | null | undefined,
  channelRaw: string | null | undefined
): TvShowTab {
  const scope = parseTvShowScope(scopeRaw);
  if (scope === 'campaign' || scope === 'mine') return scope;
  return parseTvShowChannel(channelRaw);
}

export function writeTvShowTab(params: URLSearchParams, tab: TvShowTab): void {
  if (tab === 'campaign' || tab === 'mine') {
    writeTvShowScope(params, tab);
    params.delete('tvChannel');
    return;
  }
  writeTvShowScope(params, 'plaza');
  writeTvShowChannel(params, tab);
}

export function uniqueVideosById(...groups: TvShowVideo[][]): TvShowVideo[] {
  const seen = new Set<number>();
  const out: TvShowVideo[] = [];
  for (const group of groups) {
    for (const video of group) {
      if (seen.has(video.id)) continue;
      seen.add(video.id);
      out.push(video);
    }
  }
  return out;
}

export function campaignHomeSearch(search: string): string {
  const params = new URLSearchParams(search.startsWith('?') ? search.slice(1) : search);
  writeTvShowTab(params, 'campaign');
  const s = params.toString();
  return s ? `?${s}` : '?tvScope=campaign';
}

export function formatCampaignRange(
  startAt: string,
  endAt: string,
  locale: string
): string {
  const start = Date.parse(startAt);
  const end = Date.parse(endAt);
  if (Number.isNaN(start) || Number.isNaN(end)) return '';
  const fmt = new Intl.DateTimeFormat(locale || undefined, {
    month: 'numeric',
    day: 'numeric',
  });
  return `${fmt.format(start)} – ${fmt.format(end)}`;
}

export function campaignCountdownMs(startAt: string, now = Date.now()): number {
  const start = Date.parse(startAt);
  if (Number.isNaN(start)) return 0;
  return Math.max(0, start - now);
}

export function formatCountdown(ms: number): { days: number; hours: number; minutes: number } {
  const totalMinutes = Math.floor(ms / 60_000);
  const days = Math.floor(totalMinutes / (60 * 24));
  const hours = Math.floor((totalMinutes - days * 60 * 24) / 60);
  const minutes = totalMinutes % 60;
  return { days, hours, minutes };
}

const SCRIPT_RE = /<script\b[\s\S]*?<\/script>/gi;
const EVENT_ATTR_RE = /\s+on[a-z]+\s*=\s*(['"][^'"]*['"]|[^\s>]+)/gi;

/** Strip script tags and inline event handlers from ops HTML. Not a full sanitizer. */
export function sanitizeCampaignHtml(html: string): string {
  return html.replace(SCRIPT_RE, '').replace(EVENT_ATTR_RE, '');
}
