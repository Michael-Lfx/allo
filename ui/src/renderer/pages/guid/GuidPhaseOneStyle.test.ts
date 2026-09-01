import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const page = readFileSync(new URL('./GuidPage.tsx', import.meta.url), 'utf8');
const inputCard = readFileSync(new URL('./components/GuidInputCard.tsx', import.meta.url), 'utf8');
const workspaceFootnote = readFileSync(new URL('./components/GuidWorkspaceFootnote.tsx', import.meta.url), 'utf8');
const styles = readFileSync(new URL('./index.module.css', import.meta.url), 'utf8');

describe('guid phase-one visual sample', () => {
  test('keeps the composer as the primary stage without changing its behavior seams', () => {
    expect(page.includes('styles.heroTitlePrimary')).toBe(true);
    expect(page.includes('onPaste={guidInput.onPaste}')).toBe(true);
    expect(page.includes('onSend={handleComposerSend}')).toBe(true);
    expect(page.includes('const handleComposerSend = send.sendMessageHandler;')).toBe(true);
    expect(inputCard.includes('styles.guidInputCardWrapActive')).toBe(true);
    expect(styles.includes('.guidInputCardWrap:focus-within')).toBe(true);
    const composerStyles = styles.slice(
      styles.indexOf('.guidInputCardWrap {'),
      styles.indexOf('.guidInputInner {')
    );
    expect(composerStyles.includes('translateY')).toBe(false);
    expect(composerStyles.includes('scale(')).toBe(false);
    expect(composerStyles.includes('var(--flowy-shadow-focus)')).toBe(true);
    expect(composerStyles.includes('var(--flowy-focus)')).toBe(true);
  });

  test('organizes task starters and supports reduced motion', () => {
    expect(styles.includes('grid-template-columns: repeat(3, minmax(0, 1fr))')).toBe(true);
    expect(styles.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
    expect(styles.includes('.guidResourceIntentChip:active')).toBe(true);
    const hoverStyles = styles.slice(
      styles.indexOf('.guidResourceIntentChip:hover'),
      styles.indexOf('.guidResourceIntentChip:active')
    );
    expect(hoverStyles.includes('translateY')).toBe(false);
  });

  test('keeps the workspace picker above body-level conversation overlays', () => {
    const picker = readFileSync(
      new URL('../../components/workspace/WorkspacePickerPopover.tsx', import.meta.url),
      'utf8',
    );
    expect(workspaceFootnote.includes('<WorkspacePickerPopover')).toBe(true);
    expect(picker.includes('createPortal(')).toBe(true);
    expect(picker.includes('document.body')).toBe(true);
    expect(picker.includes("position: 'fixed'")).toBe(true);
    expect(picker.includes('WORKSPACE_PICKER_POPOVER_Z_INDEX = 10020')).toBe(true);
    expect(picker.includes('zIndex: WORKSPACE_PICKER_POPOVER_Z_INDEX')).toBe(true);
    expect(workspaceFootnote.includes("t('common.filePicker.chooseDifferentFolder')")).toBe(true);
  });
});
