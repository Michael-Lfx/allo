import { describe, expect, test } from 'bun:test';
import {
  buildCapabilityHubLocation,
  mapLegacyCapabilityTab,
  parseCapabilityHubFromPathname,
  parseCapabilityHubView,
  resolveLegacyCapabilityLocation,
} from './capabilityHub';

describe('capability hub routing', () => {
  test('parses hub ids from settings and top-level pathnames', () => {
    expect(parseCapabilityHubFromPathname('/presets')).toBe('presets');
    expect(parseCapabilityHubFromPathname('/settings/skills')).toBe('skills');
    expect(parseCapabilityHubFromPathname('/mcp')).toBe('mcp');
    expect(parseCapabilityHubFromPathname('/plugins')).toBeNull();
    expect(parseCapabilityHubFromPathname('/settings/plugins')).toBeNull();
    expect(parseCapabilityHubFromPathname('/settings/system')).toBeNull();
  });

  test('maps supported legacy inner tabs onto market / installed views', () => {
    expect(mapLegacyCapabilityTab('library')).toBe('installed');
    expect(mapLegacyCapabilityTab('servers')).toBe('installed');
    expect(mapLegacyCapabilityTab('market')).toBe('market');
    expect(mapLegacyCapabilityTab('plugins')).toBeNull();
    expect(mapLegacyCapabilityTab('plugin-market')).toBeNull();
    expect(mapLegacyCapabilityTab(null)).toBeNull();
  });

  test('defaults to market unless view, legacy library tabs, or highlight say otherwise', () => {
    expect(parseCapabilityHubView(new URLSearchParams())).toBe('market');
    expect(parseCapabilityHubView(new URLSearchParams('view=installed'))).toBe('installed');
    expect(parseCapabilityHubView(new URLSearchParams('tab=library'))).toBe('installed');
    expect(parseCapabilityHubView(new URLSearchParams('tab=market'))).toBe('market');
    expect(parseCapabilityHubView(new URLSearchParams('highlight=abc'))).toBe('installed');
  });

  test('builds settings-aware locations without a default view query', () => {
    expect(buildCapabilityHubLocation({ hub: 'skills', inSettings: true })).toBe('/settings/skills');
    expect(
      buildCapabilityHubLocation({ hub: 'mcp', inSettings: false, view: 'installed' })
    ).toBe('/mcp?view=installed');
  });

  test('rewrites legacy tab query strings onto canonical hub URLs', () => {
    expect(resolveLegacyCapabilityLocation('/skills', '?tab=library')).toBe('/skills?view=installed');
    expect(resolveLegacyCapabilityLocation('/settings/presets', '?tab=market')).toBe('/settings/presets');
    expect(resolveLegacyCapabilityLocation('/mcp', '?tab=servers')).toBe('/mcp?view=installed');
    expect(resolveLegacyCapabilityLocation('/settings/mcp', '?tab=plugins')).toBe('/settings/mcp');
    expect(resolveLegacyCapabilityLocation('/mcp', '?tab=plugin-market')).toBe('/mcp');
    expect(resolveLegacyCapabilityLocation('/skills', '?highlight=foo')).toBe(
      '/skills?view=installed&highlight=foo'
    );
    expect(resolveLegacyCapabilityLocation('/presets', '?view=market')).toBe('/presets');
    expect(resolveLegacyCapabilityLocation('/presets', '?view=installed')).toBe('/presets');
    expect(resolveLegacyCapabilityLocation('/skills', '')).toBeNull();
    expect(resolveLegacyCapabilityLocation('/skills', '?view=installed')).toBeNull();
  });
});
