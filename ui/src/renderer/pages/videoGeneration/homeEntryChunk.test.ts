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
    expect(composer.includes('loadGenerationPreferencesPopover')).toBe(true);
    expect(composer.includes('prefetchGenerationPreferencesPanel')).toBe(true);
    expect(composer.includes('prefetchVerticalSkillMenu')).toBe(true);
    expect(composer.includes('prefetchLookStyleMenu')).toBe(true);
    expect(composer.includes('prefetchCanvasAssistantPanel')).toBe(true);
    expect(composer.includes("import CampaignCarousel from")).toBe(false);
    expect(composer.includes("lazy(() => import('../components/CampaignCarousel'))")).toBe(true);
    expect(composer.includes("from '../components/ModelSelectors'")).toBe(false);
    expect(composer.includes("from './BriefingControls'")).toBe(false);
    expect(composer.includes("from './BriefingPreferenceFields'")).toBe(false);
    expect(composer.includes("from './BriefingModelFields'")).toBe(false);
    expect(composer.includes("import VerticalSkillMenu from")).toBe(false);
    expect(composer.includes("import LookStyleMenu from")).toBe(false);
    expect(composer.includes("import LookPicker from")).toBe(false);
    expect(composer.includes("lazy(() => import('../styleCatalog/LookPicker'))")).toBe(true);

    const popover = source('./home/GenerationPreferencesPopover.tsx');
    expect(popover.includes("from '../components/ModelSelectors'")).toBe(false);
    expect(popover.includes('useGeneratorModels')).toBe(false);
    expect(popover.includes('warmGenerationPreferences')).toBe(true);
    expect(popover.includes("from './BriefingPreferenceFields'")).toBe(true);
    expect(popover.includes("import('./BriefingPreferenceFields')")).toBe(false);
    expect(popover.includes('TaskModelSelect')).toBe(false);
    expect(popover.includes('useModelsForTask')).toBe(false);

    const briefingFields = source('./home/BriefingPreferenceFields.tsx');
    const briefingModels = source('./home/BriefingModelFields.tsx');
    expect(briefingFields.includes('TaskModelSelect')).toBe(false);
    expect(briefingModels.includes('TaskModelSelect')).toBe(false);
    expect(briefingModels.includes('useMediaModels')).toBe(true);
    expect(briefingModels.includes('useModelsForTask')).toBe(false);

    const prefetch = source('./prefetch.ts');
    expect(prefetch.includes('prefetchGenerationPreferencesPanel')).toBe(true);
    expect(prefetch.includes('prefetchVerticalSkillMenu')).toBe(true);
    expect(prefetch.includes('prefetchLookStyleMenu')).toBe(true);
    expect(prefetch.includes('prefetchCanvasAssistantPanel')).toBe(true);
    expect(prefetch.includes('./home/GenerationPreferencesPopover')).toBe(true);
    expect(prefetch.includes('@/renderer/hooks/agent/useMediaModels')).toBe(true);
    expect(prefetch.includes('./home/VerticalSkillMenu')).toBe(true);
    expect(prefetch.includes('./styleCatalog/LookPicker')).toBe(true);
    expect(composer.includes("import VerticalSkillCreateModal from")).toBe(false);
    expect(composer.includes("import CameoCastEditor from")).toBe(false);
    expect(composer.includes('onPasteCapture')).toBe(true);
    expect(composer.includes('filesFromClipboardData')).toBe(true);
    expect(upload.includes("from 'fflate'")).toBe(false);
  });
});
