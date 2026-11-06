import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import {
  DEFAULT_SLIDE_OPTIONS,
  SLIDE_HEADER_TAG,
  applySlideHeader,
  buildSlideHeader,
  fileBaseName,
  hasSlideBody,
  isSameSlideTemplate,
  stripSlideHeader,
} from './slideOptions';
import { SLIDE_SUGGESTIONS, SLIDE_SUGGESTIONS_PER_PAGE, suggestionPage } from './slideGallery';
import { SLIDE_THEMES, SLIDE_THEME_IDS } from './slideThemes';

describe('slide options header', () => {
  test('names the options, the route and the status tool on one line', () => {
    const header = buildSlideHeader({ ...DEFAULT_SLIDE_OPTIONS, canvas: 'xiaohongshu', pages: '10-15', mode: 'guided' });
    expect(header.startsWith(SLIDE_HEADER_TAG)).toBe(true);
    expect(header).toContain('canvas=xiaohongshu');
    expect(header).toContain('pages=10-15');
    expect(header).toContain('route=Default');
    expect(header).toContain('nomi_presentation_status');
    expect(header.includes('\n')).toBe(false);
  });

  test('applies once and replaces on option change, keeping the user text', () => {
    const first = applySlideHeader('季度复盘，面向管理层', DEFAULT_SLIDE_OPTIONS);
    expect(first.split('\n')).toHaveLength(2);
    expect(first.endsWith('\n季度复盘，面向管理层')).toBe(true);

    const changed = applySlideHeader(first, { ...DEFAULT_SLIDE_OPTIONS, canvas: 'ppt43' });
    expect(changed.match(/\[ppt-master\]/g)).toHaveLength(1);
    expect(changed).toContain('canvas=ppt43');
    expect(stripSlideHeader(changed)).toBe('季度复盘，面向管理层');
  });

  test('a header alone is not a deck request', () => {
    expect(hasSlideBody(applySlideHeader('', DEFAULT_SLIDE_OPTIONS))).toBe(false);
    expect(hasSlideBody(applySlideHeader('  topic ', DEFAULT_SLIDE_OPTIONS))).toBe(true);
    expect(stripSlideHeader('plain text')).toBe('plain text');
  });
});

describe('slide template choice', () => {
  test('free design adds no template field', () => {
    expect(buildSlideHeader(DEFAULT_SLIDE_OPTIONS)).not.toContain('template=');
  });

  test('a theme passes its visual style, palette and typography', () => {
    const header = buildSlideHeader({ ...DEFAULT_SLIDE_OPTIONS, template: { kind: 'theme', id: 'inkwash' } });
    expect(header).toContain('visual_style=ink-wash');
    expect(header).toContain('references/visual-styles/ink-wash.md');
    expect(header).toContain('accent #B22222');
    expect(header).toContain('brush');
    expect(header).not.toContain('template=');
    expect(header.includes('\n')).toBe(false);
  });

  test('a gradient theme names both stops', () => {
    const header = buildSlideHeader({ ...DEFAULT_SLIDE_OPTIONS, template: { kind: 'theme', id: 'aurora' } });
    expect(header).toContain('background #1C2A6B→#7B3FD1');
  });

  test('an imported file asks for Create Template first and stays on one line', () => {
    const header = buildSlideHeader({
      ...DEFAULT_SLIDE_OPTIONS,
      template: { kind: 'import', path: 'C:\\Users\\me\\Desktop\\brand\nkit.pptx' },
    });
    expect(header).toContain('"brand kit.pptx"');
    expect(header).toContain('Create Template');
    expect(header.includes('\n')).toBe(false);
    expect(fileBaseName('/home/me/deck.pdf')).toBe('deck.pdf');
  });

  test('compares templates by kind and identity', () => {
    expect(isSameSlideTemplate({ kind: 'free' }, { kind: 'free' })).toBe(true);
    expect(isSameSlideTemplate({ kind: 'theme', id: 'chalk' }, { kind: 'theme', id: 'pixel' })).toBe(false);
    expect(isSameSlideTemplate({ kind: 'import', path: 'a.pptx' }, { kind: 'import', path: 'a.pptx' })).toBe(true);
    expect(isSameSlideTemplate({ kind: 'free' }, { kind: 'theme', id: 'chalk' })).toBe(false);
  });
});

describe('create panel gallery', () => {
  test('every theme uses a real ppt-master visual style and suggestions map to themes', () => {
    const catalog = [
      'swiss-minimal', 'soft-rounded', 'glassmorphism', 'dark-tech', 'blueprint', 'editorial', 'photo-editorial',
      'data-journalism', 'brutalist', 'memphis', 'zine', 'vintage-poster', 'paper-cut', 'sketch-notes', 'ink-notes',
      'chalkboard', 'ink-wash', 'pixel-art',
    ];
    for (const id of SLIDE_THEME_IDS) {
      expect(catalog).toContain(SLIDE_THEMES[id].visualStyle);
      for (const color of Object.values(SLIDE_THEMES[id].palette)) expect(color).toMatch(/^#[0-9A-F]{6}$/);
    }
    // Every catalog style appears at least once in the gallery.
    for (const style of catalog) expect(SLIDE_THEME_IDS.some((id) => SLIDE_THEMES[id].visualStyle === style)).toBe(true);
    for (const suggestion of SLIDE_SUGGESTIONS) expect(SLIDE_THEME_IDS).toContain(suggestion.theme);
  });

  test('suggestion pages wrap around the pool', () => {
    expect(suggestionPage(0)).toHaveLength(SLIDE_SUGGESTIONS_PER_PAGE);
    const pages = Math.ceil(SLIDE_SUGGESTIONS.length / SLIDE_SUGGESTIONS_PER_PAGE);
    expect(suggestionPage(pages).map((item) => item.id)).toEqual(suggestionPage(0).map((item) => item.id));
  });

  test('every topic and template has text in both locales', () => {
    const read = (locale: string) =>
      JSON.parse(readFileSync(new URL(`../../../services/i18n/locales/${locale}/presentation.json`, import.meta.url), 'utf8'));
    for (const locale of ['zh-CN', 'en-US']) {
      const create = read(locale).create;
      for (const suggestion of SLIDE_SUGGESTIONS) {
        expect(create.suggestions[suggestion.id].title).toBeTruthy();
        expect(create.suggestions[suggestion.id].prompt).toBeTruthy();
      }
      expect(create.free.name).toBeTruthy();
      for (const id of SLIDE_THEME_IDS) {
        expect(create.themes[id].name).toBeTruthy();
        expect(create.themes[id].description).toBeTruthy();
      }
    }
  });
});
