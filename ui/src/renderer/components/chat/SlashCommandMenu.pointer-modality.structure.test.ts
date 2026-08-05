import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = () => readFileSync(new URL('./SlashCommandMenu.tsx', import.meta.url), 'utf8');

describe('slash command menu pointer modality', () => {
  test('does not let stationary-pointer hit testing overwrite keyboard selection', () => {
    const source = readSource();

    // Keyboard navigation can scroll the list while the pointer remains still.
    // A mouseenter emitted by that re-hit-test must not write the active index
    // back to the row that happened to move under the pointer.
    expect(source.includes('onMouseEnter')).toBe(false);
    expect(source.includes('onPointerMove={(event) => handlePointerMove(event, index)}')).toBe(true);
    expect(source.includes('clientX')).toBe(true);
    expect(source.includes('clientY')).toBe(true);
    expect(source.includes('previousPosition.x === nextPosition.x')).toBe(true);
    expect(source.includes('previousPosition.y === nextPosition.y')).toBe(true);
  });

  test('does not crossfade rows while the list scrolls to a keyboard selection', () => {
    const source = readSource();

    // The selected row and the list position change in the same keyboard
    // action. A color transition leaves the previous row painted while it
    // moves, which looks like a second selection hop at the viewport edge.
    expect(source.includes('transition-none')).toBe(true);
    expect(source.includes('transition-colors')).toBe(false);
    expect(source.includes('transition-all')).toBe(false);
  });
});
