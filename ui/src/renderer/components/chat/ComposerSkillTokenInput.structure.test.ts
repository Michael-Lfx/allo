import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./ComposerSkillTokenInput.tsx', import.meta.url), 'utf8');
const styles = readFileSync(new URL('./ComposerSkillTokenInput.module.css', import.meta.url), 'utf8');

describe('ComposerSkillTokenInput', () => {
  test('renders Skills as non-editable atoms inside one editable document', () => {
    expect(source.includes('contentEditable={!disabled}')).toBe(true);
    expect(source.includes("contentEditable={false}")).toBe(true);
    expect(source.includes("data-composer-skill-token='true'")).toBe(true);
    expect(source.includes('insertSkillAtActiveSlash')).toBe(true);
  });

  test('moves across every text or Skill document unit through normal editing keys', () => {
    expect(source.includes("event.key === 'ArrowLeft'")).toBe(true);
    expect(source.includes("event.key === 'ArrowRight'")).toBe(true);
    expect(source.includes('const moveCaret = useCallback')).toBe(true);
    expect(source.includes('moveCaret(selection.start === selection.end ? selection.start - 1')).toBe(true);
    expect(source.includes('moveCaret(selection.start === selection.end ? selection.end + 1')).toBe(true);
    expect(source.includes("event.key === 'Backspace'")).toBe(true);
    expect(source.includes("event.key === 'Delete'")).toBe(true);
    expect(source.includes("deleteSelection('backward')")).toBe(true);
    expect(source.includes("deleteSelection('forward')")).toBe(true);
  });

  test('reads the native selection at write time so arrow-key movement is not lost', () => {
    expect(source.includes('const getLiveSelection = useCallback')).toBe(true);
    expect(source.includes('const selection = getLiveSelection();')).toBe(true);
    expect(source.includes('[applyDraft, getLiveSelection]')).toBe(true);
  });

  test('counts committed text that lands in the initial zero-width guard', () => {
    expect(source.includes("if (node.dataset.composerTokenGuard === 'true') {\n    return getLogicalTextLength(node);")).toBe(true);
  });

  test('owns deletion keys even when the document has no deletable content', () => {
    const backspaceStart = source.indexOf("if (event.key === 'Backspace')");
    const deleteStart = source.indexOf("if (event.key === 'Delete')");
    const arrowKeyStart = source.indexOf('if (!event.shiftKey');
    const backspaceBlock = source.slice(backspaceStart, deleteStart);
    const deleteBlock = source.slice(deleteStart, arrowKeyStart);

    expect(backspaceBlock.includes("deleteSelection('backward');")).toBe(true);
    expect(backspaceBlock.includes('event.preventDefault();')).toBe(true);
    expect(backspaceBlock.includes('if (deleteSelection')).toBe(false);
    expect(deleteBlock.includes("deleteSelection('forward');")).toBe(true);
    expect(deleteBlock.includes('event.preventDefault();')).toBe(true);
    expect(deleteBlock.includes('if (deleteSelection')).toBe(false);
  });

  test('defers React draft reconciliation until IME composition has settled', () => {
    expect(source.includes('const isComposingRef = useRef(false);')).toBe(true);
    expect(source.includes('const handleCompositionStartCapture')).toBe(true);
    expect(source.includes('const handleCompositionEndCapture')).toBe(true);
    expect(source.includes('if (isComposingRef.current || nativeEvent.isComposing) {')).toBe(true);
    expect(source.includes('requestAnimationFrame(() => {')).toBe(true);
    expect(source.includes('onCompositionStartCapture={handleCompositionStartCapture}')).toBe(true);
    expect(source.includes('onCompositionEndCapture={handleCompositionEndCapture}')).toBe(true);
  });

  test('guards malformed beforeinput inputType values before calling includes', () => {
    const inputTypeRead = source.indexOf('const inputType = nativeEvent.inputType;');
    const typeGuard = source.indexOf("if (typeof inputType !== 'string')", inputTypeRead);
    const compositionIncludes = source.indexOf("inputType.includes('Composition')", typeGuard);

    expect(inputTypeRead).toBeGreaterThan(-1);
    expect(typeGuard).toBeGreaterThan(inputTypeRead);
    expect(compositionIncludes).toBeGreaterThan(typeGuard);
    expect(source.slice(typeGuard, compositionIncludes)).toContain('return;');
  });

  test('hides the placeholder while the IME owns preedit text', () => {
    expect(source.includes('const [isComposing, setIsComposing] = useState(false);')).toBe(true);
    expect(source.includes('setIsComposing(true);')).toBe(true);
    expect(source.includes('setIsComposing(false);')).toBe(true);
    expect(source.includes("data-empty={!hasVisibleText && !hasSkills && !isComposing ? 'true' : undefined}")).toBe(true);
  });

  test('keeps an empty-state placeholder out of the editable text flow', () => {
    expect(styles.includes('.root {\n  position: relative;')).toBe(true);
    expect(styles.includes(".root[data-empty='true']::before {\n  content: attr(data-placeholder);\n  position: absolute;")).toBe(true);
    expect(styles.includes('inset-inline-start: var(--composer-placeholder-inset, 2px);')).toBe(true);
    expect(source.includes('const paddingStart = style?.paddingInlineStart ?? style?.paddingLeft;')).toBe(true);
    expect(source.includes("'--composer-placeholder-inset': placeholderInset")).toBe(true);
  });

  test('keeps Skill tokens aligned with the composer text line box', () => {
    const skillRule = styles.match(/\.skill\s*\{[\s\S]*?\}/)?.[0];

    expect(skillRule).toContain('line-height: inherit;');
    expect(skillRule).toContain('vertical-align: top;');
    expect(source.includes("<Cube theme='outline' size={16} fill='currentColor' />")).toBe(true);
  });
});
