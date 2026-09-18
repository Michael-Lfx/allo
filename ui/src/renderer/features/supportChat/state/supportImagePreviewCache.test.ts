import { describe, expect, test } from 'bun:test';
import { supportImagePreviewCache } from './supportImagePreviewCache';

describe('supportImagePreviewCache', () => {
  test('releases replaced and explicitly cleared object URLs', () => {
    supportImagePreviewCache.clear();
    const calls: string[] = [];
    const original = URL.revokeObjectURL;
    Object.defineProperty(URL, 'revokeObjectURL', {
      configurable: true,
      value: (url: string) => calls.push(url),
    });

    try {
      supportImagePreviewCache.set('replace', 'blob:old');
      supportImagePreviewCache.set('replace', 'blob:new');
      supportImagePreviewCache.set('keep', 'blob:keep');
      expect(supportImagePreviewCache.size()).toBe(2);
      expect(supportImagePreviewCache.hasPreviewUrl('blob:keep')).toBe(true);
      expect(supportImagePreviewCache.hasPreviewUrl('blob:missing')).toBe(false);

      supportImagePreviewCache.release('replace');
      expect(supportImagePreviewCache.get('replace')).toBeUndefined();
      expect(supportImagePreviewCache.hasPreviewUrl('blob:new')).toBe(false);
      expect(calls).toEqual(['blob:old', 'blob:new']);

      supportImagePreviewCache.clear();
      expect(supportImagePreviewCache.size()).toBe(0);
      expect(calls).toEqual(['blob:old', 'blob:new', 'blob:keep']);
    } finally {
      supportImagePreviewCache.clear();
      Object.defineProperty(URL, 'revokeObjectURL', {
        configurable: true,
        value: original,
      });
    }
  });
});
