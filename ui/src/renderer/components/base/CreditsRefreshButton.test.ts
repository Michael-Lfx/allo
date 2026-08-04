import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const componentSource = readFileSync(new URL('./CreditsRefreshButton.tsx', import.meta.url), 'utf8');
const siderUserMenuSource = readFileSync(
  new URL('../layout/Sider/SiderUserMenu.tsx', import.meta.url),
  'utf8'
);
const mediaSettingsSource = readFileSync(
  new URL('../../pages/settings/MediaSettings.tsx', import.meta.url),
  'utf8'
);

const CREDITS_SOURCES = [
  ['CreditsRefreshButton', componentSource],
  ['SiderUserMenu', siderUserMenuSource],
  ['MediaSettings', mediaSettingsSource],
] as const;

// 积分元素的强调色只能走「随皮肤」的 token 族（--primary-6 / --control-*）。
// --flowy-accent / --flowy-accent-fg / --flowy-accent-muted / --flowy-focus-ring 是静态青
// （只在 flowy-visual-system.css 定义，6 套皮肤无一覆盖）——正是积分元素原本「不契合主题」
// 的根因。这里按「实际取值」(var(--… ) 判定，避免代码注释里的字面量误伤。
const BANNED_STATIC_USAGE = ['var(--flowy-accent', 'var(--flowy-focus-ring'];

describe('credits refresh button', () => {
  test('derives its accent from the skin-driven primary token', () => {
    expect(componentSource.includes('rgb(var(--primary-6))')).toBe(true);
  });

  test('renders the throttle countdown as a frame-free number (no background box that misaligns with the balance)', () => {
    // 倒计时只靠强调色 {n}s 数字表达，根元素保持透明——不套软洗方框，
    // 避免与相邻余额数字错位 / 显得多余立体。
    expect(componentSource.includes('backgroundColor')).toBe(false);
  });

  test('uses a span[role=button] instead of a native <button> so UA padding/min-height cannot offset the countdown', () => {
    // 原生 <button> 自带 UA 内边距 / 最小高 / OS 外观，会令倒计时方框高出余额数字所在文本行。
    // 改用 <span role="button"> 后无这些 UA 负担，高度随内容、与余额数字等高对齐。
    expect(componentSource.includes(`role='button'`)).toBe(true);
    expect(componentSource.includes('<button')).toBe(false);
  });

  test('uses the per-skin control focus ring for keyboard focus', () => {
    expect(componentSource.includes('var(--control-focus-ring)')).toBe(true);
  });

  test('never reintroduces a static (non-skin) accent token', () => {
    const violations: string[] = [];
    for (const [name, source] of CREDITS_SOURCES) {
      for (const usage of BANNED_STATIC_USAGE) {
        if (source.includes(usage)) violations.push(`${name} → ${usage}`);
      }
    }
    expect(violations).toEqual([]);
  });

  test('keeps the refresh icon optically centered with the balance (block + leading-none)', () => {
    // @icon-park 图标默认带行高，会令图标盒高于余额数字、视觉上浮；block + leading-none 压平。
    expect(componentSource.includes('block leading-none')).toBe(true);
  });

  test('renders the countdown as a parenthesized annotation on a fixed-width slot (no reflow, no "1"-like bar)', () => {
    // 竖线紧贴数字会被误读成「1」；改用括号注解 (n)s：括号是结构分隔，零误读。
    // tabular-nums 让 n=1..5 等宽；固定槽位 w-28px 容纳 (n)s，图标态与倒计时态同宽，点刷新切换不重绘。
    expect(componentSource.includes('({cooldownSeconds}s)')).toBe(true);
    expect(componentSource.includes('w-1px')).toBe(false); // 已弃用竖线分隔
    expect(componentSource.includes('tabular-nums')).toBe(true);
    expect(componentSource.includes('w-28px')).toBe(true);
  });
});
