import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (relative: string) => readFileSync(new URL(relative, import.meta.url), 'utf8');
const guide = read('../GuidPage.tsx');
const ambient = read('./BuildAmbientWall.tsx');
const ambientCss = read('./BuildAmbientWall.module.css');
const drawer = read('./HomeInputDrawer.tsx');
const homeStage = read('./homeStage.ts');
const drawerCss = read('./HomeInputDrawer.module.css');
const indexCss = read('../index.module.css');
const slidePanel = read('../presentation/SlideCreatePanel.tsx');
const slidePanelCss = read('../presentation/SlideCreatePanel.module.css');

describe('build drawer flow contract', () => {
  test('uses the input-adjacent two-level drawer and removes the full-stage controls', () => {
    expect(guide).toContain("homeView === 'build-examples'");
    expect(guide).toContain("testId='guid-home-build-examples'");
    expect(guide).toContain('backFromBuildExamples');
    expect(guide).toContain('applyBuildPrompt');
    expect(guide).toContain('setSelectedBuildExample');
    expect(guide).toContain('selectedBuildExample');
    expect(guide).toContain('BUILD_DRAWER_COLLAPSE_MS');
    expect(guide).toContain('BUILD_DRAWER_EXPAND_MS');
    expect(guide).toContain('showBuildDirectory');
    expect(guide).toContain("setBuildTransitionDirection('back')");
    expect(guide).toContain("buildTransitionDirection === 'back'");
    expect(guide).toContain('motion={buildDirectoryMotion}');
    expect(guide).toContain('handleDirectoryMotionEnd');
    expect(guide).toContain('handleExamplesMotionEnd');
    expect(guide).toContain('onMotionEnd={handleDirectoryMotionEnd}');
    expect(drawer).toContain('onAnimationEnd');
    expect(guide).toContain('motionDuration={buildDrawerMotion === \'closing\' ? BUILD_DRAWER_COLLAPSE_MS : BUILD_DRAWER_EXPAND_MS}');
    expect(guide).toContain("if (buildDrawerMotion === 'opening')");
    expect(guide).toContain("if (homeCapability !== 'office') switchHomeCapability('office')");
    expect(guide).toContain('cover: buildAppSpending');
    expect(guide).toContain('HOME_BUILD_EXAMPLES');
    expect(guide).toContain("promptKey: 'guid.home.buildPrompts.app.second'");
    expect(guide).toContain('scrollableMedia');
    expect(guide).toContain('guid.home.buildPrompts.website.fourthLabel');
    expect(guide).toContain('guid.home.buildPrompts.app.fourthLabel');
    expect(guide).toContain('guid.home.buildPrompts.game.fourthLabel');
    expect(drawer).toContain('scrollableMedia');
    expect(drawerCss).toContain('.mediaGridScrollable');
    expect(guide).not.toContain('requestBuildPrompt');
    expect(guide).not.toContain('guid-home-build-tab');
    expect(guide).not.toContain('guid-home-build-back');
    expect(guide).not.toContain('guid-home-develop-prep');
    expect(guide).not.toContain('HOME_BUILD_STAGE_COPY');
  });

  test('keeps the build background local and lightweight, not the slide wall', () => {
    expect(guide).toContain('<BuildAmbientWall variant={buildAmbientVariant} />');
    expect(guide).toContain('data-ambient-variant={buildAmbientVariant}');
    expect(guide).not.toContain('showAmbientWall');
    expect(guide).not.toContain('SlideCardWall />\n          </div>');
    expect(ambient).toContain("data-variant={variant}");
    expect(ambient).toContain("'directory' | 'website' | 'app' | 'game'");
    expect(ambient).toContain('<canvas');
    expect(ambient).toContain('requestAnimationFrame');
    expect(ambient).not.toContain('SlideCardWall');
    expect(ambientCss).toContain(".wall[data-variant='website']");
    expect(ambientCss).toContain(".wall[data-variant='app']");
    expect(ambientCss).toContain(".wall[data-variant='directory']");
    expect(ambientCss).toContain(".wall[data-variant='game']");
    expect(ambientCss).toContain('background: transparent');
    expect(ambient).toContain('transparentVariant');
    expect(ambientCss).toContain('.canvas');
    expect(indexCss).toContain('position: absolute');
    expect(indexCss).toContain('data-ambient-variant=\'game\'');
    expect(ambient).not.toContain("fillText('Aa')");
    expect(ambient).not.toContain('drawCreate');
    expect(guide).toContain('const CreateAmbientWall = React.lazy');
    expect(guide).toContain("homeView === 'create-directory'");
    expect(guide).toContain("? 'create'");
  });

  test('returns from slides to the create directory with drawer motion', () => {
    expect(guide).toContain('closeSlidePanelToDirectory');
    expect(guide).toContain('onMotionEnd={finishCloseSlidePanel}');
    expect(slidePanel).toContain('onAnimationEnd');
    expect(slidePanelCss).toContain("data-motion='closing'");
    expect(slidePanelCss).toContain('panelExit');
    expect(slidePanelCss).toContain('contain: layout style');
  });

  test('removes the index.html metadata and keeps the build prompt examples substantial', () => {
    expect(guide).not.toContain("meta: 'index.html'");
    expect(drawer).not.toContain('option.meta');
    expect(drawerCss).toContain("drawer-collapse");
    expect(drawerCss).toContain("drawer-expand");
    expect(drawerCss).toContain("data-motion='exit'");
    expect(drawerCss).toContain("data-motion='expand'");
    expect(drawerCss).toContain('--drawer-motion-duration');
    expect(drawer).toContain("setAttribute('inert'");
    expect(homeStage).not.toContain('meta?: string');

    const zh = JSON.parse(read('../../../services/i18n/locales/zh-CN/guid.json'));
    const en = JSON.parse(read('../../../services/i18n/locales/en-US/guid.json'));
    for (const locale of [zh, en]) {
      for (const kind of ['website', 'app', 'game']) {
        for (const slot of ['first', 'second', 'third']) {
          expect(locale.home.buildPrompts[kind][slot].length).toBeGreaterThanOrEqual(100);
        }
      }
    }
    for (const kind of ['website', 'app', 'game']) {
      for (const slot of ['first', 'second', 'third']) {
        expect(zh.home.buildPrompts[kind][slot]).toMatch(/验收|验证|测试|检查/);
      }
    }
  });
});
