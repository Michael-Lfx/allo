

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

  test('keeps only the remaining header controls', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));

    expect(source.includes('AutoWorkControl')).toBe(false);
    expect(source.includes('IdmmControl')).toBe(false);
    expect(source.includes("<KnowledgeControl target={{ kind: 'conversation', id: conversation_id }} />")).toBe(true);
  });

  test('opens workspace views in preview tabs instead of a separate right rail', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));

    expect(source.includes('openWorkspaceTab')).toBe(true);
    expect(source.includes('workspaceContent={workspaceSider}')).toBe(true);
    expect(source.includes("classNames('!bg-1 relative chat-layout-right-sider layout-sider')")).toBe(false);
    expect(source.includes('MobileWorkspaceOverlay')).toBe(false);
    expect(source.includes('onWorkspaceTabActivate={selectWorkspaceTool}')).toBe(true);
  });

  test('opens Shell via the former conversation-terminals rail entry', () => {
    const source = readSource(new URL('./index.tsx', import.meta.url));
    expect(source.includes("nextTab === 'conversation-terminals'")).toBe(true);
    expect(source.includes('void openShellPreview()')).toBe(true);
    expect(source.includes("tab.key !== 'conversation-terminals'")).toBe(true);
    expect(source.includes('shellPreviewActive={shellPreviewActive}')).toBe(true);
  });
});
