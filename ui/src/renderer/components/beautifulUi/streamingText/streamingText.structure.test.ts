import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./StreamingText.tsx', import.meta.url), 'utf8');
const cssSource = readFileSync(new URL('./streamingText.module.css', import.meta.url), 'utf8');
const shadowSource = readFileSync(new URL('../../Markdown/ShadowView.tsx', import.meta.url), 'utf8');

describe('StreamingText', () => {
  test('covers Beautiful UI streaming status without inventing message types', () => {
    expect(source.includes("export type StreamingTextStatus = 'streaming' | 'done'")).toBe(true);
    expect(source.includes("data-testid='beautiful-ui-streaming-text'")).toBe(true);
    expect(source.includes('data-status={status}')).toBe(true);
    expect(cssSource.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
  });

  test('places the streaming caret on the last in-flow child, not a sibling after the body', () => {
    expect(source.includes('styles.caret')).toBe(false);
    expect(source.includes("aria-hidden='true'")).toBe(false);
    expect(cssSource.includes('.body > :last-child::after')).toBe(true);
    expect(cssSource.includes('.body:empty::after')).toBe(true);
    const caretRuleIndex = cssSource.indexOf('.body > :last-child::after');
    const streamingIndex = cssSource.lastIndexOf('.streaming', caretRuleIndex);
    expect(caretRuleIndex).toBeGreaterThan(-1);
    expect(streamingIndex).toBeGreaterThan(-1);
    expect(streamingIndex).toBeLessThan(caretRuleIndex);
    const reducedMotion = cssSource.slice(cssSource.indexOf('@media (prefers-reduced-motion: reduce)'));
    expect(reducedMotion.includes('animation: none')).toBe(true);
    expect(shadowSource.includes('.markdown-shadow-body > :last-child::after')).toBe(true);
    expect(shadowSource.includes('--beautiful-ui-streaming-caret-content')).toBe(true);
  });

  test('suppresses the light-DOM caret when the last child hosts markdown-shadow', () => {
    expect(cssSource.includes('.body > :last-child:has(:global(.markdown-shadow))::after')).toBe(true);
    expect(cssSource.includes(':last-child:has(.markdown-shadow)')).toBe(false);
    expect(cssSource.includes(':last-child.markdown-shadow')).toBe(false);
  });

  test('animates the light-DOM caret with a module-hashed keyframe, not a custom-property name', () => {
    const lightCaretRule = cssSource.slice(
      cssSource.indexOf('.streaming .body:empty::after'),
      cssSource.indexOf('@keyframes streaming-caret')
    );
    expect(lightCaretRule.includes('animation: streaming-caret')).toBe(true);
    expect(lightCaretRule.includes('animation: var(--beautiful-ui-streaming-caret-animation')).toBe(false);
    const reducedMotion = cssSource.slice(cssSource.indexOf('@media (prefers-reduced-motion: reduce)'));
    expect(reducedMotion.includes('animation: none')).toBe(true);
  });

  test('uses an ink caret without a primary glow', () => {
    expect(cssSource.includes('linear-gradient')).toBe(false);
    expect(cssSource.includes('box-shadow: 0 0 8px')).toBe(false);
    expect(cssSource.includes('--beautiful-ui-streaming-caret-bg: var(--color-text-1')).toBe(true);
    expect(cssSource.includes('--beautiful-ui-streaming-caret-shadow: none')).toBe(true);
  });

  test('moves an open-fence caret onto the last line of the code card', () => {
    expect(cssSource.includes(':last-child:global(.message-streaming-code)::after')).toBe(true);
    expect(cssSource.includes(':global(.message-streaming-code__content)::after')).toBe(true);
  });
});
