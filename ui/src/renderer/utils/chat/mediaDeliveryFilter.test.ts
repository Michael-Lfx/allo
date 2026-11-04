import { describe, expect, test } from 'bun:test';
import { hasMediaDeliveryTag, stripMediaDeliveryTags } from './mediaDeliveryFilter';

const HERO =
  'MEDIA:C:\\Users\\38788\\AppData\\Local\\Flowy\\Nomi-dev\\media\\generated\\2026-11-02\\6202a5d6-b555-4442-bf22-2f6ecbce472a.jpg';
const RAIN =
  'MEDIA:C:\\Users\\38788\\AppData\\Local\\Flowy\\Nomi-dev\\media\\generated\\2026-11-02\\2af6d896-8edf-43c0-a783-87f8b03abc3d.jpg';
const MACRO =
  'MEDIA:C:\\Users\\38788\\AppData\\Local\\Flowy\\Nomi-dev\\media\\generated\\2026-11-02\\2c25b0ec-f7d9-42ac-ad5f-89415310aa18.jpg';

describe('stripMediaDeliveryTags', () => {
  test('drops generated-media path lines and keeps the surrounding answer', () => {
    const input = `半透明耳翼的通透感都拍出来了。\n\n${HERO}\n${RAIN}\n${MACRO}\n\n**02 提交给生成 API 的提示词**：`;
    expect(stripMediaDeliveryTags(input)).toBe(
      '半透明耳翼的通透感都拍出来了。\n\n**02 提交给生成 API 的提示词**：'
    );
    expect(stripMediaDeliveryTags(input)).not.toContain('media\\generated');
    expect(stripMediaDeliveryTags(input)).not.toContain('MEDIA:');
  });

  test('is a no-op when the reply only mentions the protocol in prose', () => {
    const input = 'Deliver with MEDIA: when local_path is available.';
    expect(hasMediaDeliveryTag(input)).toBe(false);
    expect(stripMediaDeliveryTags(input)).toBe(input);
  });

  test('removes several absolute tags from one line', () => {
    const input = `画面如下。 ${HERO} ${RAIN}`;
    expect(stripMediaDeliveryTags(input)).toBe('画面如下。');
  });

  test('removes a unix absolute tag without touching the sentence around it', () => {
    const input = '结果在 MEDIA:/tmp/generated/a.jpg 这里。';
    expect(stripMediaDeliveryTags(input)).toBe('结果在 这里。');
  });
});
