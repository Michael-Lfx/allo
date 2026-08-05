import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = () => readFileSync(new URL('./NomiSendBox.tsx', import.meta.url), 'utf8');

describe('Nomi desktop add menu', () => {
  test('offers goal setup without changing the file-selection flow', () => {
    const source = readSource();

    expect(source.includes('enableGoalMenu')).toBe(true);
    expect(source.includes('const [goalModeArmed, setGoalModeArmed] = useState(false)')).toBe(true);
    expect(source.includes('goalModeArmed={goalModeArmed}')).toBe(true);
    expect(source.includes('onGoalModeChange={setGoalModeArmed}')).toBe(true);
    expect(source.includes('armed={goalModeArmed} onArmedChange={setGoalModeArmed}')).toBe(true);
    expect(source.includes('openFileSelector={openFileSelector}')).toBe(true);
  });
});
