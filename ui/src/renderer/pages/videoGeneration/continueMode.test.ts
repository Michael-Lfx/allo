import { describe, expect, test } from 'bun:test';
import { shouldContinueAsRender } from './continueMode';

describe('shouldContinueAsRender', () => {
  test('ignores cancelled / interrupted / failed tail events', () => {
    expect(
      shouldContinueAsRender({
        events: [
          { stage: 'design_storyboard', message: '', at: 'a' },
          { stage: 'planned', message: '', at: 'b' },
          { stage: 'render_start', message: '', at: 'c' },
          { stage: 'video_poll', message: '', at: 'd' },
          { stage: 'cancelled', message: '', at: 'e' },
        ],
        stage: 'cancelled',
      })
    ).toBe(true);
  });

  test('resumes planning when cancel happened during creative work', () => {
    expect(
      shouldContinueAsRender({
        events: [
          { stage: 'develop_story', message: '', at: 'a' },
          { stage: 'extract_characters', message: '', at: 'b' },
          { stage: 'cancelled', message: '', at: 'c' },
        ],
        stage: 'extract_characters',
      })
    ).toBe(false);
  });

  test('treats planned as render-ready even with a terminal tail', () => {
    expect(
      shouldContinueAsRender({
        events: [
          { stage: 'planned', message: '', at: 'a' },
          { stage: 'interrupted', message: '', at: 'b' },
        ],
        stage: 'interrupted',
      })
    ).toBe(true);
  });

  test('falls back to session stage when the event log is empty', () => {
    expect(shouldContinueAsRender({ events: [], stage: 'video_poll' })).toBe(true);
    expect(shouldContinueAsRender({ events: [], sessionStage: 'write_script' })).toBe(false);
  });
});
