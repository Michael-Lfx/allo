import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relative: string) => readFileSync(new URL(relative, import.meta.url), 'utf8');
const cards = read('./ShowcaseCards.tsx');
const guide = read('../GuidPage.tsx');

describe('home showcase entry contract', () => {
  test('keeps video and image shortcuts while replacing the app card with a website card', () => {
    expect(cards).toContain("data-testid=\"guid-home-showcase-video\"");
    expect(cards).toContain("data-testid=\"guid-home-showcase-website\"");
    expect(cards).toContain("website-main.webp");
    expect(cards).toContain("guid.home.showcaseWebsiteTitle");
    expect(cards).toContain("guid.home.showcaseWebsiteBadge");
    expect(cards).not.toContain('showcaseApp');
    expect(cards).not.toContain('AppGlyph');
  });

  test('opens the website build examples instead of a blank develop capability', () => {
    expect(guide).toContain('handleSelectShowcaseWebsite');
    expect(guide).toContain('showBuildExamples(\'website\', false)');
    expect(guide).toContain('showBuildExamples(\'website\', true)');
    expect(guide).toContain('onSelectWebsite={handleSelectShowcaseWebsite}');
    expect(guide).toContain("capabilityHasDraft('develop')");
    expect(guide).toContain('openWebsiteExamples');
    expect(guide).toContain('setPendingExample({');
    expect(guide).not.toContain('handleSelectShowcaseApp');
  });
});
