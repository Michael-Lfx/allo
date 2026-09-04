import { describe, expect, test } from 'bun:test';
import {
  campaignCarouselAction,
  campaignCountdownMs,
  campaignHomeSearch,
  formatCountdown,
  inAppNavigatePath,
  isHttpUrl,
  isInAppCampaignPath,
  parseTvShowScope,
  sanitizeCampaignHtml,
  writeTvShowScope,
} from './campaign';

describe('campaign carousel click rules', () => {
  test('list campaigns open the detail page first', () => {
    expect(
      campaignCarouselAction({
        showInList: true,
        linkUrl: 'https://example.com',
      })
    ).toBe('detail');
  });

  test('promo-only slides follow linkUrl', () => {
    expect(
      campaignCarouselAction({
        showInList: false,
        linkUrl: 'https://example.com/promo',
      })
    ).toBe('link');
  });

  test('promo slides without a link stay static', () => {
    expect(campaignCarouselAction({ showInList: false, linkUrl: '  ' })).toBe('none');
    expect(campaignCarouselAction({ showInList: false, linkUrl: null })).toBe('none');
  });
});

describe('campaign links', () => {
  test('detects http(s) vs in-app paths', () => {
    expect(isHttpUrl('https://cdn.example.com/a')).toBe(true);
    expect(isHttpUrl('/video-generation')).toBe(false);
    expect(isInAppCampaignPath('/video-generation/campaigns/1')).toBe(true);
    expect(isInAppCampaignPath('#/video-generation?tvScope=campaign')).toBe(true);
    expect(inAppNavigatePath('#/video-generation')).toBe('/video-generation');
  });

  test('preserves existing home search when pinning the campaign tab', () => {
    expect(campaignHomeSearch('?mode=creation')).toBe('?mode=creation&tvScope=campaign');
    expect(campaignHomeSearch('')).toBe('?tvScope=campaign');
  });
});

describe('tv show scope query', () => {
  test('reads campaign and mine, defaults everything else to plaza', () => {
    expect(parseTvShowScope('campaign')).toBe('campaign');
    expect(parseTvShowScope('mine')).toBe('mine');
    expect(parseTvShowScope(null)).toBe('plaza');
    expect(parseTvShowScope('plaza')).toBe('plaza');
    expect(parseTvShowScope('other')).toBe('plaza');
  });

  test('leaving campaign clears the query so plaza/mine do not bounce back', () => {
    const fromCampaign = new URLSearchParams('mode=creation&tvScope=campaign');
    writeTvShowScope(fromCampaign, 'plaza');
    expect(fromCampaign.get('tvScope')).toBeNull();
    expect(fromCampaign.get('mode')).toBe('creation');
    expect(parseTvShowScope(fromCampaign.get('tvScope'))).toBe('plaza');

    writeTvShowScope(fromCampaign, 'mine');
    expect(fromCampaign.get('tvScope')).toBe('mine');
    expect(parseTvShowScope(fromCampaign.get('tvScope'))).toBe('mine');
  });
});

describe('campaign countdown', () => {
  test('clamps past start times to zero', () => {
    expect(campaignCountdownMs('2000-01-01T00:00:00.000Z', Date.parse('2026-01-01'))).toBe(0);
  });

  test('splits remaining time into days/hours/minutes', () => {
    expect(formatCountdown(((2 * 24 + 3) * 60 + 7) * 60_000)).toEqual({
      days: 2,
      hours: 3,
      minutes: 7,
    });
  });
});

describe('campaign html sanitise', () => {
  test('drops script tags and inline handlers', () => {
    const html = `<p onclick="alert(1)">hi</p><script>alert(2)</script><img src="a.png">`;
    const out = sanitizeCampaignHtml(html);
    expect(out.includes('<script')).toBe(false);
    expect(out.toLowerCase().includes('onclick')).toBe(false);
    expect(out.includes('<img src="a.png">')).toBe(true);
  });
});
