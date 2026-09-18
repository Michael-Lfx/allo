/**
 * Warm the video-generation home route before the sider click.
 * Keep this module free of page-level imports so the sider stays out of that chunk.
 */
import { loadCanvasAssistantPanel } from '../videoCanvas/loadAssistantPanel';
import { loadVideoCanvasProjectPage } from '../videoCanvas/loadProjectPage';

export function prefetchGenerationPreferencesPanel(): void {
  void import('@/renderer/hooks/agent/useMediaModels')
    .then((mod) => mod.fetchMediaModels())
    .catch(() => undefined);
  void import('./home/GenerationPreferencesPopover').then((mod) => {
    mod.warmGenerationPreferences();
  }).catch(() => undefined);
}

export function prefetchVerticalSkillMenu(): void {
  void import('./home/VerticalSkillMenu').catch(() => undefined);
}

export function prefetchLookStyleMenu(): void {
  void import('./styleCatalog/LookPicker').catch(() => undefined);
}

export function prefetchCanvasAssistantPanel(): void {
  void loadCanvasAssistantPanel().catch(() => undefined);
}

export function prefetchCanvasWorkspace(): void {
  void loadVideoCanvasProjectPage().catch(() => undefined);
  prefetchCanvasAssistantPanel();
}

export function prefetchVideoGenerationHome(): void {
  void import('./index').catch(() => undefined);
  void import('./components/TvShowPanel').catch(() => undefined);
  void import('./components/CampaignCarousel').catch(() => undefined);
  void import('./components/CampaignPanel').catch(() => undefined);
  void import('./home/CanvasProjectGallery').catch(() => undefined);
  prefetchGenerationPreferencesPanel();
  prefetchVerticalSkillMenu();
  prefetchLookStyleMenu();
}
