import { iconColors } from '@/renderer/styles/colors';
import { Branch, Change, ChartHistogram, Close, FileText, FolderOpen, Plus, Terminal, WebPage } from '@icon-park/react';
import { Dropdown, Menu } from '@arco-design/web-react';
import { IconShrink } from '@arco-design/web-react/icon';
import React from 'react';
import { useTranslation } from 'react-i18next';
import WorkspaceFileIcon from '@/renderer/pages/conversation/Workspace/components/WorkspaceFileIcon';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import type { TabFadeState } from '../../hooks/useTabOverflow';
import {
  inferPreviewTabKind,
  type PreviewTabKind,
  type WorkspacePreviewTabDefinition,
} from '../../previewTabKind';
import './preview.css';

/**
 * Tab 信息
 * Tab information
 */
export interface PreviewTab {
  /**
   * Tab ID
   */
  id: string;

  /**
   * Tab 标题
   * Tab title
   */
  title: string;

  /**
   * 是否有未保存的修改
   * Whether there are unsaved changes
   */
  isDirty?: boolean;

  /**
   * Mixed editor-group kind. Defaults to file when omitted.
   */
  kind?: PreviewTabKind;
  workspaceTabKey?: string;

  /**
   * Filename used for the file-type icon.
   */
  fileName?: string;
}

/**
 * PreviewTabs 组件属性
 * PreviewTabs component props
 */
interface PreviewTabsProps {
  /**
   * Tabs 列表
   * Tabs list
   */
  tabs: PreviewTab[];

  /**
   * 当前活动的 Tab ID
   * Current active tab ID
   */
  activeTabId: string | null;

  /**
   * Tab 渐变状态（左右溢出指示器）
   * Tab fade state (left/right overflow indicators)
   */
  tabFadeState: TabFadeState;

  /**
   * Tabs 容器引用
   * Tabs container ref
   */
  tabsContainerRef: React.Ref<HTMLDivElement | null>;

  /**
   * 切换 Tab 回调
   * Switch tab callback
   */
  onSwitchTab: (tabId: string) => void;

  /**
   * 关闭 Tab 回调
   * Close tab callback
   */
  onCloseTab: (tabId: string) => void;

  /**
   * Tab 右键菜单回调
   * Tab context menu callback
   */
  onContextMenu: (e: React.MouseEvent, tabId: string) => void;

  /**
   * 关闭预览面板回调
   * Close preview panel callback
   */
  onClosePanel?: () => void;

  onAddFile?: () => void;
  onAddTerminal?: () => void;
  onAddBrowser?: () => void;
  workspaceTabs?: readonly WorkspacePreviewTabDefinition[];
  onOpenWorkspaceTab?: (definition: WorkspacePreviewTabDefinition) => void;
}

const TabKindIcon: React.FC<{ tab: PreviewTab }> = ({ tab }) => {
  const kind = inferPreviewTabKind({ kind: tab.kind });
  switch (kind) {
    case 'terminal':
      return <Terminal theme='outline' size={12} fill='currentColor' />;
    case 'browser':
      return <WebPage theme='outline' size={12} fill='currentColor' />;
    case 'file':
      return <WorkspaceFileIcon fileName={tab.fileName || tab.title} size={12} />;
    case 'workspace':
      switch (tab.workspaceTabKey) {
        case 'files':
          return <FolderOpen theme='outline' size={12} fill='currentColor' />;
        case 'changes':
          return <Change theme='outline' size={12} fill='currentColor' />;
        case 'conversation-terminals':
          return <Terminal theme='outline' size={12} fill='currentColor' />;
        case 'nomi-session-metrics':
          return <ChartHistogram theme='outline' size={12} fill='currentColor' />;
        case 'agent-execution':
          return <Branch theme='outline' size={12} fill='currentColor' />;
        default:
          return <FolderOpen theme='outline' size={12} fill='currentColor' />;
      }
    default: {
      const _exhaustive: never = kind;
      return _exhaustive;
    }
  }
};

/**
 * Compact Cursor-style mixed tab strip for the preview column.
 */
const PreviewTabs: React.FC<PreviewTabsProps> = ({
  tabs,
  activeTabId,
  tabFadeState,
  tabsContainerRef,
  onSwitchTab,
  onCloseTab,
  onContextMenu,
  onClosePanel,
  onAddFile,
  onAddTerminal,
  onAddBrowser,
  workspaceTabs,
  onOpenWorkspaceTab,
}) => {
  const { t } = useTranslation();
  const layout = useLayoutContext();
  const { left: showLeftFade, right: showRightFade } = tabFadeState;
  const showAddMenu = Boolean(onAddFile || onAddTerminal || onAddBrowser);
  const showWorkspaceMenu = Boolean(layout?.isMobile && workspaceTabs?.length && onOpenWorkspaceTab);

  const addTabMenu = showAddMenu ? (
    <Dropdown
      trigger='click'
      position='bl'
      droplist={
        <Menu>
          {onAddFile && (
            <Menu.Item key='file' onClick={onAddFile}>
              <span className='inline-flex items-center gap-6px'>
                <FileText theme='outline' size={12} fill='currentColor' />
                {t('preview.addFile')}
              </span>
            </Menu.Item>
          )}
          {onAddTerminal && (
            <Menu.Item key='terminal' onClick={onAddTerminal}>
              <span className='inline-flex items-center gap-6px'>
                <Terminal theme='outline' size={12} fill='currentColor' />
                {t('preview.addTerminal')}
              </span>
            </Menu.Item>
          )}
          {onAddBrowser && (
            <Menu.Item key='browser' onClick={onAddBrowser}>
              <span className='inline-flex items-center gap-6px'>
                <WebPage theme='outline' size={12} fill='currentColor' />
                {t('preview.addBrowser')}
              </span>
            </Menu.Item>
          )}
        </Menu>
      }
    >
      <button
        type='button'
        className='flex items-center justify-center w-20px h-20px rd-4px flex-shrink-0 text-t-secondary hover:text-t-primary hover:bg-3 transition-colors border-none bg-transparent cursor-pointer p-0'
        title={t('preview.addTab')}
        aria-label={t('preview.addTab')}
      >
        <Plus theme='outline' size={12} fill='currentColor' />
      </button>
    </Dropdown>
  ) : null;

  return (
    <div className='preview-tabs'>
      <div className='preview-tabs__row'>
        <div className='preview-tabs__scroller-wrap'>
          <div ref={tabsContainerRef} className='preview-tabs__scroller'>
            {tabs.length > 0 ? (
              tabs.map((tab) => {
                const isActive = tab.id === activeTabId;
                return (
                  <div
                    key={tab.id}
                    role='tab'
                    tabIndex={0}
                    aria-selected={isActive}
                    className={`group flex items-center gap-4px px-7px h-24px rd-6px cursor-pointer transition-colors flex-shrink-0 border-1px border-solid ${
                      isActive
                        ? 'bg-1 text-t-primary font-medium border-[var(--color-border-2)]'
                        : 'text-t-secondary border-transparent hover:bg-3 hover:text-t-primary'
                    }`}
                    onClick={() => onSwitchTab(tab.id)}
                    onContextMenu={(e) => onContextMenu(e, tab.id)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        onSwitchTab(tab.id);
                      }
                    }}
                  >
                    <span
                      className={`flex items-center ${isActive ? 'text-t-primary' : 'text-t-tertiary group-hover:text-t-primary'}`}
                    >
                      <TabKindIcon tab={tab} />
                    </span>
                    <span className='text-12px whitespace-nowrap leading-none'>{tab.title}</span>
                    <span className='relative flex items-center justify-center w-12px h-12px flex-shrink-0'>
                      {tab.isDirty && (
                        <span
                          className='w-6px h-6px rd-full bg-primary group-hover:opacity-0 group-focus-within:opacity-0'
                          title={t('preview.unsavedChangesTitle')}
                        />
                      )}
                      <Close
                        theme='outline'
                        size='12'
                        fill={iconColors.secondary}
                        className={`absolute hover:fill-primary ${
                          tab.isDirty
                            ? 'opacity-0 group-hover:opacity-100 group-focus-within:opacity-100'
                            : `opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 ${isActive ? '!opacity-100' : ''}`
                        }`}
                        onClick={(e) => {
                          e.stopPropagation();
                          onCloseTab(tab.id);
                        }}
                      />
                    </span>
                  </div>
                );
              })
            ) : (
              <div className='text-12px text-t-tertiary px-6px'>{t('preview.noTabs')}</div>
            )}
          </div>
          {showLeftFade && (
            <div
              className='pointer-events-none absolute left-0 top-0 bottom-0 w-24px'
              style={{
                background: 'linear-gradient(90deg, var(--bg-2) 0%, transparent 100%)',
              }}
            />
          )}
          {showRightFade && (
            <div
              className='pointer-events-none absolute right-0 top-0 bottom-0 w-24px'
              style={{
                background: 'linear-gradient(270deg, var(--bg-2) 0%, transparent 100%)',
              }}
            />
          )}
        </div>

        {addTabMenu ? <div className='preview-tabs__add'>{addTabMenu}</div> : null}

        {showWorkspaceMenu && (
          <Dropdown
            trigger='click'
            position='bl'
            droplist={
              <Menu>
                {workspaceTabs?.map((tab) => (
                  <Menu.Item key={tab.key} onClick={() => onOpenWorkspaceTab?.(tab)}>
                    {tab.title}
                  </Menu.Item>
                ))}
              </Menu>
            }
          >
            <button
              type='button'
              className='flex items-center justify-center w-20px h-20px rd-4px flex-shrink-0 text-t-secondary hover:text-t-primary hover:bg-3 transition-colors border-none bg-transparent cursor-pointer p-0 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary'
              title={t('preview.workspaceViews')}
              aria-label={t('preview.workspaceViews')}
            >
              <FolderOpen theme='outline' size={12} fill='currentColor' />
            </button>
          </Dropdown>
        )}

        {onClosePanel && (
          <div className='preview-tabs__collapse'>
            <button
              type='button'
              className='preview-tabs__collapse-btn'
              onClick={onClosePanel}
              title={t('preview.collapsePanel')}
              aria-label={t('preview.collapsePanel')}
            >
              <IconShrink style={{ fontSize: 12, color: iconColors.secondary }} />
            </button>
          </div>
        )}
      </div>
    </div>
  );
};

export default PreviewTabs;
