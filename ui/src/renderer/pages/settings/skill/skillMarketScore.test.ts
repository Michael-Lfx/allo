import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import i18next from 'i18next';
import enSettings from '../../../services/i18n/locales/en-US/settings.json';
import zhSettings from '../../../services/i18n/locales/zh-CN/settings.json';

// Regression coverage for the market score rendering: the packaged locale
// bundles must interpolate the real value, so a built UI can never show the
// literal placeholder text `{{score}}`.
const createInstance = async (lng: 'zh-CN' | 'en-US') => {
  const instance = i18next.createInstance();
  await instance.init({
    resources: {
      'zh-CN': { translation: { settings: zhSettings } },
      'en-US': { translation: { settings: enSettings } },
    },
    lng,
    fallbackLng: 'en-US',
    interpolation: { escapeValue: false },
  });
  return instance;
};

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('SkillHub market score rendering', () => {
  test('zh-CN interpolates the real score without leaking the placeholder', async () => {
    const i18n = await createInstance('zh-CN');
    const rendered = i18n.t('settings.skillsMarket.score', { score: '35003.3' });
    expect(rendered).toBe('热度分 35003.3');
    expect(rendered).not.toContain('{{');
  });

  test('en-US interpolates the real score without leaking the placeholder', async () => {
    const i18n = await createInstance('en-US');
    const rendered = i18n.t('settings.skillsMarket.score', { score: '35003.3' });
    expect(rendered).toBe('Popularity score 35003.3');
    expect(rendered).not.toContain('{{');
  });

  test('both packaged locales keep the score interpolation slot', () => {
    // A locale that drops the {{score}} slot would silently render a bare
    // label; keep the slot mandatory so the number always reaches the UI.
    expect(zhSettings.skillsMarket.score).toContain('{{score}}');
    expect(enSettings.skillsMarket.score).toContain('{{score}}');
  });

  test('every score rendering site feeds the interpolation variable', () => {
    for (const file of ['SkillMarketCard.tsx', 'SkillMarketListRow.tsx']) {
      const source = readSource(new URL(`./${file}`, import.meta.url));
      expect(source).toMatch(/t\('settings\.skillsMarket\.score', \{ score:/);
    }
  });
});
