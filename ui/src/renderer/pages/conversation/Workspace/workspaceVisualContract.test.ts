import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const bodySource = readFileSync(new URL('./WorkspaceRailBody.tsx', import.meta.url), 'utf8');
const toolbarSource = readFileSync(new URL('./components/WorkspaceToolbar.tsx', import.meta.url), 'utf8');
const workspaceCss = readFileSync(new URL('./workspace.css', import.meta.url), 'utf8');
const arcoOverrides = readFileSync(new URL('../../../styles/arco-override.css', import.meta.url), 'utf8');
const chatLayoutCss = readFileSync(new URL('../components/ChatLayout/chat-layout.css', import.meta.url), 'utf8');

describe('workspace file tree visual contract', () => {
  test('renders full-width tree rows with stable file-name hooks', () => {
    expect(bodySource.includes('blockNode')).toBe(true);
    expect(bodySource.includes('workspace-node-content flex')).toBe(true);
    expect(bodySource.includes('workspace-node-name overflow')).toBe(true);
    expect(bodySource.includes('title={nodeData.name}')).toBe(true);
    expect(bodySource.includes("import { Down } from '@icon-park/react';")).toBe(true);
    expect(bodySource.includes("import WorkspaceFileIcon from './components/WorkspaceFileIcon';")).toBe(true);
    expect(bodySource.includes('<WorkspaceFileIcon fileName={nodeProps.dataRef?.name ?? \'\'} />')).toBe(true);
    expect(bodySource.includes('loadingIcon: workspaceTreeChevron')).toBe(true);
  });

  test('keeps toolbar actions keyboard and screen-reader friendly', () => {
    expect(toolbarSource.includes('className=\'workspace-toolbar-toggle')).toBe(true);
    expect(toolbarSource.includes('aria-expanded={!isWorkspaceCollapsed}')).toBe(true);
    expect(toolbarSource.includes("aria-label={t('common.fileAttach.addFiles')}")).toBe(true);
    expect(toolbarSource.includes('aria-busy={loading}')).toBe(true);
    expect(toolbarSource.includes("className={loading ? 'loading' : undefined}")).toBe(true);
    expect(toolbarSource.includes('type=\'button\'')).toBe(true);
  });

  test('defines dense, themed row states for desktop and mobile', () => {
    expect(workspaceCss.includes('--workspace-tree-row-height: 28px;')).toBe(true);
    expect(workspaceCss.includes('min-height: var(--workspace-tree-row-height);')).toBe(true);
    expect(workspaceCss.includes('height: var(--workspace-tree-row-height);')).toBe(true);
    expect(workspaceCss.includes('font-size: 13px !important;')).toBe(true);
    expect(workspaceCss.includes('arco-tree-node:hover {')).toBe(true);
    expect(workspaceCss.includes('arco-tree-node-selected {')).toBe(true);
    expect(workspaceCss.includes('arco-tree-node:focus-within')).toBe(true);
    expect(workspaceCss.includes('.workspace-tree-chevron')).toBe(true);
    expect(workspaceCss.includes('.workspace-file-type-icon')).toBe(true);
    expect(workspaceCss).toMatch(
      /\.chat-workspace \.workspace-tree \.arco-tree-node-switcher \{\s+width: 14px !important;\s+height: var\(--workspace-tree-row-height\);\s+margin-right: 0 !important;/,
    );
    expect(workspaceCss).toMatch(
      /\.chat-workspace \.workspace-tree\.arco-tree-show-line \.arco-tree-node-indent-block \{\s+width: 12px !important;\s+margin-right: 4px !important;/,
    );
    expect(workspaceCss.includes('--workspace-tree-row-height: 32px;')).toBe(true);
    expect(workspaceCss.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
  });

  test('keeps workspace tree overrides local to the workspace stylesheet', () => {
    expect(arcoOverrides.includes('.workspace-tree .arco-tree-node-selected .arco-tree-node-title')).toBe(false);
    expect(arcoOverrides.includes('.workspace-tree .arco-tree-node:hover .arco-tree-node-title')).toBe(false);
  });

  test('gives the panel header actions the same interaction treatment', () => {
    expect(chatLayoutCss.includes('.workspace-open-button__btn:hover')).toBe(true);
    expect(chatLayoutCss.includes('.workspace-open-button__btn:focus-visible')).toBe(true);
    expect(chatLayoutCss.includes('.workspace-bind-button__btn:active')).toBe(true);
    expect(chatLayoutCss.includes('@media (prefers-reduced-motion: reduce)')).toBe(true);
  });
});
