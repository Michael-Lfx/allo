import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

describe('blob download lifetime', () => {
  test('revokes the object url from a later timer, not the click turn', () => {
    const source = readFileSync(new URL('./download.ts', import.meta.url), 'utf8');
    const click = source.indexOf('link.click()');
    const revoke = source.indexOf('URL.revokeObjectURL(url)');
    expect(click).toBeGreaterThan(-1);
    expect(revoke).toBeGreaterThan(click);
    expect(source.slice(click, revoke)).toContain('setTimeout');
  });

  test('image preview saves the data url already on the tab', () => {
    const panel = readFileSync(
      new URL('../../pages/conversation/Preview/components/PreviewPanel/PreviewPanel.tsx', import.meta.url),
      'utf8'
    );
    const dataUrl = panel.indexOf("content.startsWith('data:')");
    const diskRead = panel.indexOf('downloadFileFromPath(');
    expect(dataUrl).toBeGreaterThan(-1);
    expect(diskRead).toBeGreaterThan(dataUrl);
    expect(panel).toContain('downloadDataUrl(content, rawFileName)');
  });

  test('desktop save dialog copies any on-disk preview, not only images', () => {
    const panel = readFileSync(
      new URL('../../pages/conversation/Preview/components/PreviewPanel/PreviewPanel.tsx', import.meta.url),
      'utf8'
    );
    expect(panel).toContain('metadata?.file_path && isTauriRuntime()');
    expect(panel).not.toContain("content_type === 'image' && metadata?.file_path && isTauriRuntime()");
    expect(panel).toContain('ipcBridge.shell.copyFile.invoke');
  });
});
