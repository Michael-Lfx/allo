import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, test } from 'bun:test';

const root = path.resolve(__dirname, '..', '..');

const walk = function* (dir: string): Generator<string> {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      yield* walk(full);
      continue;
    }
    if (/\.tsx?$/.test(entry.name)) yield full;
  }
};

const rel = (file: string) => path.relative(root, file).split(path.sep).join('/');

const ARCO_MESSAGE_RE = /import[ \t]+(?:type[ \t]+)?\{[^}]*\}\s*from\s*['"]@arco-design\/web-react['"]/g;

const readSource = (relativeUrl: string) => readFileSync(new URL(relativeUrl, import.meta.url), 'utf8');

describe('notification facade source contracts', () => {
  test('no runtime or type Arco Message/Notification import survives anywhere in the renderer', () => {
    const violators = [];
    for (const file of walk(root)) {
      const relative = rel(file);
      if (relative.includes('pages/videoCanvas/oc/')) continue; // antd-based module, separate stack
      if (path.basename(file).includes('.test.')) continue; // tests legitimately embed the pattern as literals
      const src = readFileSync(file, 'utf8');
      for (const match of src.matchAll(ARCO_MESSAGE_RE)) {
        const braces = match[0].slice(match[0].indexOf('{') + 1, match[0].lastIndexOf('}'));
        const members = braces.split(',').map((member) => member.trim());
        const hit = members.find((member) => /^(Message|Notification)$/.test(member) || /^(Message|Notification)\s+as\s+\w+$/.test(member));
        if (hit) violators.push(`${relative} (${hit})`);
      }
    }
    expect(violators).toEqual([]);
  });

  test('passthrough semantics replaced the old className hook completely', () => {
    const banner = readSource('../../pages/conversation/execution/PlanApprovalBanner.tsx');
    const attempt = readSource('../../pages/conversation/execution/ProjectedAttemptView.tsx');
    expect(banner.match(/passthrough: true/g) ?? []).toHaveLength(2);
    expect(attempt.match(/passthrough: true/g) ?? []).toHaveLength(10);
    for (const src of [banner, attempt]) {
      expect(src.includes('nomifun-message-passthrough')).toBe(false);
      expect(src.includes('TOAST_CLASS')).toBe(false);
    }
    const arcoOverride = readSource('../../styles/arco-override.css');
    expect(arcoOverride.includes('nomifun-message-passthrough')).toBe(false);
    const css = readSource('./notifications.css');
    expect(css.includes('.flowy-notification-card--passthrough')).toBe(true);
    expect(css.includes('.flowy-notification-card--exiting')).toBe(true);
    expect(css.includes('.flowy-notification-card--collapsing')).toBe(true);
  });

  test('the host is mounted once in the layout and the hook facade is rewired', () => {
    const layout = readSource('../layout/Layout.tsx');
    expect(layout.includes('<NotificationHost />')).toBe(true);
    const hook = readSource('../../utils/ui/useArcoMessage.ts');
    expect(hook.includes("from '@/renderer/components/notifications'")).toBe(true);
    expect(hook.includes("from '@arco-design/web-react'")).toBe(false);
  });

  test('only bottom-anchored composers opt into notification avoidance', () => {
    const composer = readSource('../../components/chat/ComposerSurface.tsx');
    const sendBox = readSource('../../components/chat/SendBox/index.tsx');
    const homeComposer = readSource('../../pages/guid/components/GuidInputCard.tsx');

    expect(composer.includes('registerNotificationBlocker?: boolean')).toBe(true);
    expect(composer.includes('registerNotificationBlocker = false')).toBe(true);
    expect(composer.includes('useNotificationBlocker(registerNotificationBlocker)')).toBe(true);
    expect(sendBox.includes('registerNotificationBlocker')).toBe(true);
    expect(homeComposer.includes('registerNotificationBlocker={false}')).toBe(true);
  });

  test('defect regressions stay fixed in source', () => {
    const insets = readSource('./notificationInsets.ts');
    expect(insets.includes("'transitionend', update, true")).toBe(true);
    expect(insets.includes("'animationend', update, true")).toBe(true);
    expect(insets.includes("'scroll', update, true")).toBe(true);
    expect(insets.includes("removeEventListener('scroll', update, true)")).toBe(true);
    expect(insets.includes('requestAnimationFrame')).toBe(true);

    const host = readSource('./NotificationHost.tsx');
    expect(host.includes('getCollapsedRecords(records)')).toBe(true);
    expect(host.includes('getCollapseExitKeys')).toBe(true);
    expect(host.includes('NotificationAnnouncementQueue')).toBe(true);
    expect(host.includes('displayedRecords = expanded ? sortByCreatedAt(activeRecords)')).toBe(false);
    expect(host.includes('duration: 240')).toBe(true);
    expect(host.includes('window.requestAnimationFrame(() => counterRef.current?.focus())')).toBe(true);
    expect(host.includes("resumeInteraction('notification-pointer')")).toBe(true);
    expect(host.includes("resumeInteraction('notification-focus')")).toBe(true);
    expect(host.includes("has('assertive')")).toBe(true);
    expect(host.includes("take('assertive')")).toBe(true);

    const model = readSource('./notificationStackModel.ts');
    expect(model.includes('export const getCollapsedRecords')).toBe(true);
  });

  test('notification styles only animate non-layout properties and honor reduced motion', () => {
    const css = readSource('./notifications.css');
    expect(css.includes('linear-gradient')).toBe(false);
    expect(css.includes('backdrop-filter')).toBe(false);
    expect(css.includes('.arco-message')).toBe(false);
    expect(css.includes('counter-pill')).toBe(false);
    for (const token of ['--notification-brand', '--notification-info', '--notification-success', '--notification-warning', '--notification-danger']) {
      expect(css.includes(token)).toBe(true);
    }
    expect(css.includes('prefers-reduced-motion: reduce')).toBe(true);

    const allowed = new Set(['opacity', 'transform', 'color', 'border-color', 'background-color', 'outline-color']);
    const transitionProps = [...css.matchAll(/transition:\s*([^;]+);/g)]
      .flatMap((match) => match[1].split(','))
      .map((part) => part.trim().split(/\s+/)[0])
      .filter((prop) => prop !== 'none');
    for (const prop of transitionProps) {
      expect(allowed.has(prop)).toBe(true);
    }
    for (const match of css.matchAll(/animation:\s*([\w-]+)/g)) {
      const name = match[1];
      // `animation: none` in the reduced-motion block disables, not animates.
      if (name === 'none') continue;
      expect([
        'flowy-notification-enter',
        'flowy-notification-exit',
        'flowy-notification-collapse',
        'flowy-notification-icon-in',
        'flowy-notification-spin',
      ]).toContain(name);
    }
    // Keyframes may only tween opacity/transform.
    for (const block of css.matchAll(/@keyframes[^{]+\{([\s\S]*?)\n\}/g)) {
      const props = [...block[1].matchAll(/^\s*([a-z-]+)\s*:/gm)].map((m) => m[1]);
      for (const prop of props) {
        expect(['opacity', 'transform']).toContain(prop);
      }
    }
  });

  test('notification surfaces remain opaque and isolated from page content', () => {
    const css = readSource('./notifications.css');

    expect(css.includes('--notification-base-surface: var(--flowy-surface-1')).toBe(true);
    expect(css.includes('isolation: isolate')).toBe(true);
    expect(css.includes('background-image: none')).toBe(true);
    expect(css.includes('background-color: var(--notification-base-surface)')).toBe(true);
    expect(css.includes('var(--notification-base-surface) 92%')).toBe(true);
    expect(css.includes('var(--notification-base-surface) 96%')).toBe(true);
  });

  test('the theme contract does not override notification semantic surfaces', () => {
    const themeContract = readSource('../../styles/theme-control-contract.css');
    expect(themeContract.includes('.flowy-notification-card {\n  background-color:')).toBe(false);
    expect(themeContract.includes('border-width: var(--flowy-border-hairline, 1px) !important;')).toBe(true);
    expect(themeContract.includes('border-style: solid !important;')).toBe(true);
    expect(themeContract.includes('.flowy-notification-card--error')).toBe(false);
  });

  test('built-in theme presets do not retain old Arco notification selectors', () => {
    const presetDir = path.resolve(__dirname, '../../pages/settings/DisplaySettings/presets');
    for (const entry of readdirSync(presetDir, { withFileTypes: true })) {
      if (!entry.isFile() || !entry.name.endsWith('.css')) continue;
      const source = readFileSync(path.join(presetDir, entry.name), 'utf8');
      expect(source.includes('.arco-message')).toBe(false);
      expect(source.includes('.arco-notification')).toBe(false);
      expect(source.includes('.arco-message-wrapper')).toBe(false);
    }
  });
});
