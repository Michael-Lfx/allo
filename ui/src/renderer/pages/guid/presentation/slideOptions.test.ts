import { describe, expect, test } from 'bun:test';
import {
  DEFAULT_SLIDE_OPTIONS,
  SLIDE_HEADER_TAG,
  applySlideHeader,
  buildSlideHeader,
  hasSlideBody,
  stripSlideHeader,
} from './slideOptions';

describe('slide options header', () => {
  test('names the options, the route and the status tool on one line', () => {
    const header = buildSlideHeader({ canvas: 'xiaohongshu', pages: '10-15', mode: 'guided' });
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
