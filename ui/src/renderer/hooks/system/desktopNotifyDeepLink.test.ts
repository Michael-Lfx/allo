import { describe, expect, test } from 'bun:test';
import {
  conversationAttentionId,
  conversationNotifyDeepLink,
  navigateDeepLink,
  requirementAttentionId,
  requirementNotifyDeepLink,
  supportAttentionId,
  supportNotifyDeepLink,
} from './desktopNotifyDeepLink';

describe('desktopNotifyDeepLink', () => {
  test('conversation link uses flowy navigate route', () => {
    const link = conversationNotifyDeepLink('0190f5fe-7c00-7a00-8000-000000000001');
    expect(link.startsWith('flowy://navigate?route=')).toBe(true);
    const route = decodeURIComponent(link.slice('flowy://navigate?route='.length));
    expect(route).toBe('/conversation/0190f5fe-7c00-7a00-8000-000000000001');
  });

  test('conversation attention is stable and can travel with the deep link', () => {
    const attentionId = conversationAttentionId('conversation-1', 'turn-7');
    expect(attentionId).toBe('conversation:conversation-1:turn:turn-7');
    const link = conversationNotifyDeepLink('conversation-1', attentionId);
    const route = decodeURIComponent(link.slice('flowy://navigate?route='.length));
    expect(route).toBe('/conversation/conversation-1?attention_id=conversation%3Aconversation-1%3Aturn%3Aturn-7');
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

  test('requirement and support attention ids use source prefixes', () => {
    expect(requirementAttentionId('req-1')).toBe('requirement:req-1');
    expect(supportAttentionId(7)).toBe('support:7');
    expect(supportNotifyDeepLink('support:7')).toBe('flowy://support?attention_id=support%3A7');
  });
});
