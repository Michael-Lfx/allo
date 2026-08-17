
import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const detailSource = readFileSync(new URL('./KnowledgeDetailPage/index.tsx', import.meta.url), 'utf8');
const actionBarSource = readFileSync(new URL('./KnowledgeDetailPage/KnowledgeDetailActionBar.tsx', import.meta.url), 'utf8');

describe('Knowledge detail document action bar', () => {
  test('keeps the back link icon and label vertically centered as one row', () => {
    expect(detailSource.includes('knowledge-detail-back-link')).toBe(true);
    expect(detailSource.includes('knowledge-detail-back-icon')).toBe(true);
    expect(detailSource.includes('[&_svg]:block')).toBe(true);
    expect(detailSource.includes("<Left theme='outline' size='14' />\n          <span>")).toBe(false);
  });

  test('uses a soft borderless action bar for folder and upload actions', () => {
    expect(detailSource.includes('knowledge-doc-actions')).toBe(true);
    expect(detailSource.includes('knowledge-doc-action')).toBe(true);
    expect(detailSource.includes('Bottom actions: new + upload */}\n                <div className=\'flex gap-7px mt-8px border-t')).toBe(false);
    expect(detailSource.includes('border-none bg-transparent')).toBe(true);
  });

  test('places document actions above document search and includes folder creation', () => {
    const actionsIndex = detailSource.indexOf('knowledge-doc-actions');
    const searchIndex = detailSource.indexOf('knowledge-doc-search');
    expect(actionsIndex).toBeGreaterThan(-1);
    expect(searchIndex).toBeGreaterThan(-1);
    expect(actionsIndex).toBeLessThan(searchIndex);
    expect(detailSource.includes('openNewFolderModal')).toBe(true);
    expect(detailSource.includes('FolderPlus')).toBe(true);
  });

  test('uses compact per-node menus instead of inline delete text in the document tree', () => {
    expect(detailSource.includes('knowledge-tree-node-row')).toBe(true);
    expect(detailSource.includes('knowledge-tree-node-name')).toBe(true);
    expect(detailSource.includes('knowledge-tree-node-action')).toBe(true);
    expect(detailSource.includes('knowledge-tree-node-more')).toBe(true);
    expect(detailSource.includes('handleTreeNodeMenuClick')).toBe(true);
    expect(detailSource.includes("key='new-file'")).toBe(false);
    expect(detailSource.includes("key='new-folder'")).toBe(true);
    expect(detailSource.includes("key='rename'")).toBe(true);
    expect(detailSource.includes("key='delete'")).toBe(true);
    expect(detailSource.includes('deleteFolderWarning')).toBe(true);
    expect(detailSource.includes("className='!hidden group-hover:!inline-flex shrink-0'")).toBe(false);
  });

  test('right-aligns tree row actions and reveals them only for the active row', () => {
    expect(detailSource.includes('knowledge-doc-tree')).toBe(true);
    expect(detailSource.includes('[&_.arco-tree-node-title-wrapper]:flex')).toBe(true);
    expect(detailSource.includes('[&_.arco-tree-node-title]:flex-1')).toBe(true);
    expect(detailSource.includes('knowledge-tree-node-row group flex w-full')).toBe(true);
    expect(detailSource.includes('knowledge-tree-node-action ml-auto w-24px')).toBe(true);
    expect(detailSource.includes('opacity-0')).toBe(true);
    expect(detailSource.includes('group-hover:opacity-100')).toBe(true);
    expect(detailSource.includes('focus-within:opacity-100')).toBe(true);
    expect(detailSource.includes("aria-label={t('common.more'")).toBe(true);
  });

  test('carries no Feishu connector UI (removed integration; only the create-flow placeholder remains)', () => {
    expect(detailSource.includes('FEISHU_KNOWLEDGE_CREATION_ENABLED')).toBe(false);
    expect(detailSource.includes('KnowledgeConnectorDrawer')).toBe(false);
    expect(detailSource.includes('setConnectorVisible')).toBe(false);
    expect(detailSource.includes('syncSource')).toBe(false);
  });

  test('imports supported documents and folders through the multipart knowledge API', () => {
    expect(detailSource.includes('handleUploadFiles')).toBe(true);
    expect(detailSource.includes('handleUpload')).toBe(true);
    expect(detailSource.includes('importDocument.invoke')).toBe(true);
    expect(detailSource.includes('KNOWLEDGE_IMPORT_EXTENSIONS')).toBe(true);
    expect(detailSource.includes('KNOWLEDGE_IMPORT_CONCURRENCY = 2')).toBe(true);
    expect(detailSource.includes('openUploadModal')).toBe(true);
    expect(detailSource.includes('pendingUploadSource')).toBe(true);
    expect(detailSource.includes('clearPendingUpload')).toBe(true);
    expect(detailSource.includes('uploadTargetPath')).toBe(true);
    expect(detailSource.includes('uploadTargetFolders')).toBe(true);
    expect(detailSource.includes('accept={KNOWLEDGE_IMPORT_ACCEPT}')).toBe(true);
    expect(detailSource.includes("setAttribute('webkitdirectory', '')")).toBe(true);
    expect(detailSource.includes('uploadTodo')).toBe(false);
  });

  test('uses theme-aware contrast for detail badges and settings fields', () => {
    expect(detailSource.includes('knowledge-detail-soft-active')).toBe(true);
    expect(detailSource.includes('knowledge-detail-kind-badge')).toBe(true);
    expect(detailSource.includes('knowledge-detail-user-tag')).toBe(true);
    expect(detailSource.includes('knowledge-detail-add-tag')).toBe(true);
    expect(detailSource.includes('knowledge-detail-tabs')).toBe(true);
    expect(detailSource.includes('knowledge-detail-settings-input')).toBe(true);
    expect(detailSource.includes('knowledge-detail-danger-panel')).toBe(true);
    expect(detailSource.includes("textClass: 'text-[rgb(var(--primary-5))]'")).toBe(false);
    expect(detailSource.includes("textClass: 'text-[rgb(var(--success-5))]'")).toBe(false);
    expect(detailSource.includes("textClass: 'text-[rgb(var(--warning-5))]'")).toBe(false);
    expect(detailSource.includes('!bg-primary-1 !text-primary-6 font-600')).toBe(false);
  });

  test('keeps document actions below the viewer and stabilizes scrollbar layout', () => {
    expect(detailSource.includes("className='box-border flex h-56px")).toBe(true);
    expect(detailSource.includes('autoSize={{ minRows: 14, maxRows: 16 }}')).toBe(true);
    expect(detailSource.includes('[scrollbar-gutter:stable]')).toBe(true);
    expect(detailSource.includes('justify-between gap-10px mt-12px')).toBe(true);
    expect(detailSource.includes("className='flex items-center justify-end gap-8px'")).toBe(true);
    expect(detailSource.includes("t('knowledge.detail.docs.edit'")).toBe(true);
    expect(detailSource.includes('overflow-y-auto')).toBe(true);
  });

  test('opts icon and label actions into the shared horizontal layout contract', () => {
    expect(detailSource.includes('KnowledgeDetailActionBar')).toBe(true);
    expect(actionBarSource.includes("className='flowy-icon-text-btn'")).toBe(true);
    expect(actionBarSource.includes("icon={<Search")).toBe(true);
    expect(actionBarSource.includes("icon={<LinkOne")).toBe(true);
    expect(actionBarSource.includes("data-testid='knowledge-detail-action-more'")).toBe(true);
    expect(actionBarSource.includes("icon={<More")).toBe(true);
  });
});
