import { describe, expect, test } from 'bun:test';
import { historyTabAfterPathChange, type SiderHistoryTab } from './historyTab';

describe('historyTabAfterPathChange', () => {
  test('follows a video-generation path onto the video tab', () => {
    expect(historyTabAfterPathChange('/video-generation', 'workspaces')).toBe('video');
    expect(historyTabAfterPathChange('/video-generation/clip/abc', 'companions')).toBe('video');
  });

  test('follows a nomi path onto the companions tab', () => {
    expect(historyTabAfterPathChange('/nomi', 'workspaces')).toBe('companions');
    expect(historyTabAfterPathChange('/nomi?tab=overview', 'video')).toBe('companions');
  });

  test('does not steal the companions tab on a companion conversation', () => {
    expect(historyTabAfterPathChange('/conversation/conv-1', 'companions')).toBe('companions');
  });

  test('leaves the video tab when opening a workpath conversation', () => {
    expect(historyTabAfterPathChange('/conversation/conv-1', 'video')).toBe('workspaces');
    expect(historyTabAfterPathChange('/conversation/conv-1', 'workspaces')).toBe('workspaces');
  });

  test('keeps companions on guid and terminal routes so a tab click is not snapped back', () => {
    expect(historyTabAfterPathChange('/guid', 'companions')).toBe('companions');
    expect(historyTabAfterPathChange('/terminal-new', 'companions')).toBe('companions');
    expect(historyTabAfterPathChange('/terminal/sess-1', 'companions')).toBe('companions');
  });

  test('moves off video onto workspaces when landing on guid or terminal', () => {
    expect(historyTabAfterPathChange('/guid', 'video')).toBe('workspaces');
    expect(historyTabAfterPathChange('/terminal-new', 'video')).toBe('workspaces');
    expect(historyTabAfterPathChange('/terminal/sess-1', 'workspaces')).toBe('workspaces');
  });

  test('leaves the current tab unchanged on dock and unrelated routes', () => {
    const current: SiderHistoryTab = 'video';
    expect(historyTabAfterPathChange('/knowledge', current)).toBe('video');
    expect(historyTabAfterPathChange('/learn', 'companions')).toBe('companions');
    expect(historyTabAfterPathChange('/settings/system', 'workspaces')).toBe('workspaces');
    expect(historyTabAfterPathChange('/scheduled', 'companions')).toBe('companions');
  });
});
