import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Nomi overview preset surface', () => {
  test('keeps an applied preset snapshot read-only without a reuse/apply control', () => {
    const source = readSource(new URL('./PersonaSection.tsx', import.meta.url));

    expect(source).toContain('profile.applied_preset');
    expect(source).toContain("nomi.settings.appliedPreset");
    expect(source).toContain('继续沿用已保存的设定快照');
    expect(source).not.toContain('PresetApplyControl');
    expect(source).not.toContain('onApplyPreset');
    expect(source).not.toContain('ipcBridge.presets');
  });
});
