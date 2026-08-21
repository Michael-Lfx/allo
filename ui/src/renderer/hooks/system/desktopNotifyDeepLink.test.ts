import { describe, expect, test } from 'bun:test';
import {
  conversationNotifyDeepLink,
  navigateDeepLink,
  requirementNotifyDeepLink,
} from './desktopNotifyDeepLink';

describe('desktopNotifyDeepLink', () => {
  test('conversation link uses flowy navigate route', () => {
    const link = conversationNotifyDeepLink('0190f5fe-7c00-7a00-8000-000000000001');
    expect(link.startsWith('flowy://navigate?route=')).toBe(true);
    const route = decodeURIComponent(link.slice('flowy://navigate?route='.length));
    expect(route).toBe('/conversation/0190f5fe-7c00-7a00-8000-000000000001');
  });

  test('requirement link encodes tag query', () => {
    const link = requirementNotifyDeepLink('dev/ops', 'req-1');
    const route = decodeURIComponent(link.slice('flowy://navigate?route='.length));
    expect(route.startsWith('/requirements?')).toBe(true);
    expect(route).toContain('tag=dev%2Fops');
    expect(route).toContain('id=req-1');
  });

  test('navigateDeepLink encodes arbitrary route', () => {
    expect(navigateDeepLink('/requirements?x=1')).toBe(
      `flowy://navigate?route=${encodeURIComponent('/requirements?x=1')}`
    );
  });
});
