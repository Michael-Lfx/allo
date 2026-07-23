

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('ChatLayout advanced controls', () => {
  test('uses the shared Flowy logo as the default conversation header icon', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));

    expect(source.includes("import appLogo from '@/renderer/assets/logo.svg';")).toBe(true);
    expect(source.includes("<img src={appLogo} alt='Flowy' className='block h-16px w-16px object-contain' />")).toBe(true);
    expect(source.includes('props.headerLeading ??')).toBe(true);
  });

  test('keeps the stable header controls', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));

    expect(source.includes("<AutoWorkControl target={{ kind: 'conversation', id: conversation_id }} />")).toBe(true);
    expect(source.includes("<IdmmControl target={{ kind: 'conversation', id: conversation_id }} />")).toBe(true);
    expect(source.includes("<KnowledgeControl target={{ kind: 'conversation', id: conversation_id }} />")).toBe(true);
  });

  test('does not let workspace file-tree events auto-expand the conversation right rail', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));

    expect(source.includes('autoExpandOnFiles: false')).toBe(true);
  });

  test('keeps the workspace tool rail at the far right of the expanded panel', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));
    const panelIndex = source.indexOf("className={classNames('!bg-1 relative chat-layout-right-sider layout-sider')}");
    const railIndex = source.indexOf('<WorkspaceToolRail');

    expect(panelIndex >= 0).toBe(true);
    expect(railIndex >= 0).toBe(true);
    expect(panelIndex < railIndex).toBe(true);
  });
});
