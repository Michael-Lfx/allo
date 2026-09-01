/**
 * Warm the video-generation home route before the sider click.
 * Keep this module free of page-level imports so the sider stays out of that chunk.
 */
import { loadCanvasAssistantPanel } from '../videoCanvas/loadAssistantPanel';
import { loadVideoCanvasProjectPage } from '../videoCanvas/loadProjectPage';

export function prefetchGenerationPreferencesPanel(): void {
  void import('./home/GenerationPreferencesPopover').then((mod) => {
    mod.warmGenerationPreferences();
  });
}

export function prefetchVerticalSkillMenu(): void {
  void import('./home/VerticalSkillMenu');
}

export function prefetchCanvasAssistantPanel(): void {
  void loadCanvasAssistantPanel();
}

export function prefetchCanvasWorkspace(): void {
  void loadVideoCanvasProjectPage();
  prefetchCanvasAssistantPanel();
}

export function prefetchVideoGenerationHome(): void {
  void import('./index');
  void import('./components/TvShowPanel');
  void import('./components/CampaignCarousel');
  void import('./components/CampaignPanel');
  void import('./home/CanvasProjectGallery');
  prefetchGenerationPreferencesPanel();
  prefetchVerticalSkillMenu();
}
