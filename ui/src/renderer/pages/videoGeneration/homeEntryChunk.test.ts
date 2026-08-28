import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');

describe('video generation home entry chunk', () => {
  test('does not statically pull preference catalogs, skill hub, or canvas API', () => {
    const page = source('./index.tsx');
    const composer = source('./home/VideoHomeComposer.tsx');
    const upload = source('./home/documentUpload.ts');

    expect(page.includes("from './components/DurationTimelineBar'")).toBe(false);
    expect(/\bimport\s+(?!type\b)[^;]*from '\.\.\/videoCanvas\/api'/.test(page)).toBe(false);
    expect(page.includes("from './components/SessionCard'")).toBe(false);
    expect(page.includes("from './components/TvShowPanel'")).toBe(false);
    expect(page.includes("from '@/common'")).toBe(false);
    expect(composer.includes("import GenerationPreferencesPopover from")).toBe(false);
    expect(composer.includes("import VerticalSkillMenu from")).toBe(false);
    expect(composer.includes("import VerticalSkillCreateModal from")).toBe(false);
    expect(composer.includes("import CameoCastEditor from")).toBe(false);
    expect(composer.includes('onPasteCapture')).toBe(true);
    expect(composer.includes('filesFromClipboardData')).toBe(true);
    expect(upload.includes("from 'fflate'")).toBe(false);
  });
});
