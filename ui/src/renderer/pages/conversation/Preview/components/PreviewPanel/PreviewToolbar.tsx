

import type { PreviewHistoryTarget } from '@/common/types/office/preview';
import { iconColors } from '@/renderer/styles/colors';
import { Dropdown } from '@arco-design/web-react';
import { Close } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import { supportsPreviewHistory } from '../../constants';

/**
 * 工具栏按钮的样式令牌。
 * Toolbar button style tokens.
 *
 * Exported so viewers that publish their own buttons into the toolbar-extras
 * portal render identical chrome instead of pasting the literals again.
 * Complete literal class strings — never composed at runtime.
 */
export const PREVIEW_TOOLBAR_BTN_CLASS =
  'flex items-center gap-2px px-8px py-3px rd-4px cursor-pointer transition-colors duration-150 text-12px font-medium text-t-secondary hover:text-t-primary hover:bg-3';
export const PREVIEW_TOOLBAR_BTN_ACTIVE_CLASS = '!text-white bg-brand hover:!text-white hover:bg-brand-hover';

/**
 * PreviewToolbar 组件属性
 * PreviewToolbar component props
 */
interface PreviewToolbarProps {
  /**
   * 内容类型
   * Content type
   */
  content_type: string;

  /**
   * 是否为 Markdown 文件
   * Whether it's a Markdown file
   */
  isMarkdown: boolean;

  /**
   * 是否为 HTML 文件
   * Whether it's an HTML file
   */
  isHTML: boolean;

  /**
   * 是否可编辑
   * Whether editable
   */
  isEditable: boolean;

  /**
   * 是否处于编辑模式
   * Whether in edit mode
   */
  isEditMode: boolean;

  /**
   * 当前视图模式
   * Current view mode
   */
  viewMode: 'source' | 'preview';

  /**
   * 是否启用分屏模式
   * Whether split-screen mode is enabled
   */
  isSplitScreenEnabled: boolean;

  /**
   * 文件名
   * Filename
   */
  file_name?: string;

  /**
   * 是否显示"在系统中打开"按钮
   * Whether to show "Open in System" button
   */
  showOpenInSystemButton: boolean;

  /**
   * 历史目标
   * History target
   */
  historyTarget: PreviewHistoryTarget | null;

  /**
   * 是否正在保存快照
   * Whether snapshot is saving
   */
  snapshotSaving: boolean;

  /**
   * 设置视图模式
   * Set view mode
   */
  onViewModeChange: (mode: 'source' | 'preview') => void;

  /**
   * 设置分屏模式
   * Set split-screen mode
   */
  onSplitScreenToggle: () => void;

  /**
   * 编辑按钮点击
   * Edit button click
   */
  onEditClick: () => void;

  /**
   * 退出编辑按钮点击
   * Exit edit button click
   */
  onExitEdit: () => void;

  /**
   * 保存快照
   * Save snapshot
   */
  onSaveSnapshot: () => void;

  /**
   * 刷新历史列表
   * Refresh history list
   */
  onRefreshHistory: () => void;

  /**
   * 渲染历史下拉菜单
   * Render history dropdown
   */
  renderHistoryDropdown: () => React.ReactNode;

  /**
   * 在系统中打开文件
   * Open file in system
   */
  onOpenInSystem: () => void;

  /**
   * 下载文件
   * Download file
   */
  onDownload: () => void;

  /**
   * 是否显示「打开文件所在位置」（图片且有磁盘路径时）
   * Whether to show "open file location" (images with a disk path)
   */
  showRevealInFolder?: boolean;

  /**
   * 在文件管理器中定位该文件
   * Reveal the file in its folder
   */
  onRevealInFolder?: () => void;

  /**
   * 关闭预览面板
   * Close preview panel
   */
  onClose: () => void;

  /**
   * 左侧额外渲染内容
   * Extra content rendered on the left section
   */
  leftExtra?: React.ReactNode;

  /**
   * 右侧额外渲染内容
   * Extra content rendered on the right section
   */
  rightExtra?: React.ReactNode;
}

/**
 * 预览面板工具栏组件
 * Preview panel toolbar component
 *
 * 包含文件名、视图模式切换、编辑按钮、快照/历史按钮、下载按钮、关闭按钮等
 * Contains filename, view mode toggle, edit button, snapshot/history buttons, download button, close button, etc.
 */
// eslint-disable-next-line max-len
const PreviewToolbar: React.FC<PreviewToolbarProps> = ({
  content_type,
  isMarkdown,
  isHTML,
  isEditable,
  isEditMode,
  viewMode,
  isSplitScreenEnabled,
  file_name,
  showOpenInSystemButton,
  historyTarget,
  snapshotSaving,
  onSplitScreenToggle,
  onEditClick,
  onExitEdit,
  onSaveSnapshot,
  onRefreshHistory,
  renderHistoryDropdown,
  onOpenInSystem,
  onDownload,
  onClose,
  showRevealInFolder = false,
  onRevealInFolder,
  leftExtra,
  rightExtra,
}) => {
  const { t } = useTranslation();
  const isDiff = content_type === 'diff';
  const isImage = content_type === 'image';
  const preferActionButtonsInFront = Boolean(leftExtra);
  // Image actions stay on the right: filename left, download + reveal right.
  const actionsInFront = preferActionButtonsInFront && !isImage;

  const toolbarBtn = PREVIEW_TOOLBAR_BTN_CLASS;
  const toolbarBtnActive = PREVIEW_TOOLBAR_BTN_ACTIVE_CLASS;
  const toolbarIconSize = 14;
  const markdownActionIconSize = 16;
  const fileActionIconSize = isMarkdown ? markdownActionIconSize : toolbarIconSize;

  // Snapshot/history are offered only for the types the backend store accepts,
  // and only in the mode where the source text is on screen.
  const snapshotButtonsVisible =
    supportsPreviewHistory(content_type) &&
    (content_type === 'code' ? isEditable && isEditMode : viewMode === 'source' || isSplitScreenEnabled);

  return (
    <div
      className={`flex items-center justify-between h-40px px-10px flex-shrink-0 border-b border-b-solid border-arco-1 ${isMarkdown ? 'preview-toolbar--md' : ''} ${isImage ? 'overflow-hidden' : ''}`}
    >
      <div
        className={`flex items-center justify-between gap-8px w-full min-w-0 ${isImage ? '' : 'overflow-x-auto'}`}
        style={isImage ? undefined : { minWidth: 'max-content' }}
      >
        {/* 左侧只展示文件名，长名称在剩余宽度里省略。 */}
        <div className={`flex items-center h-full gap-8px ${isImage ? 'preview-toolbar__file' : 'min-w-0'}`}>
          {file_name ? (
            <span
              className='block min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-15px text-t-primary'
              title={file_name}
            >
              {file_name}
            </span>
          ) : null}
          {(isMarkdown || isHTML || isDiff) && (
            <>
              {!isDiff && (
                <div
                  className={`flex items-center px-8px py-3px rd-4px cursor-pointer transition-colors duration-150 ${isSplitScreenEnabled ? toolbarBtnActive : 'text-t-secondary hover:bg-3'}`}
                  onClick={() => {
                    try {
                      onSplitScreenToggle();
                    } catch {
                      /* ignore */
                    }
                  }}
                  title={isSplitScreenEnabled ? t('preview.closeSplitScreen') : t('preview.openSplitScreen')}
                >
                  <svg
                    width={fileActionIconSize}
                    height={fileActionIconSize}
                    viewBox='0 0 24 24'
                    fill='none'
                    stroke='currentColor'
                    strokeWidth='2'
                  >
                    <rect x='3' y='3' width='18' height='18' rx='2' />
                    <line x1='12' y1='3' x2='12' y2='21' />
                  </svg>
                </div>
              )}
            </>
          )}

          {content_type === 'code' && isEditable && (
            <div
              className={`${toolbarBtn} ${isEditMode ? toolbarBtnActive : ''}`}
              onClick={() => (isEditMode ? onExitEdit() : onEditClick())}
              title={isEditMode ? t('preview.exitEdit') : t('preview.edit')}
            >
              <svg
                width={fileActionIconSize}
                height={fileActionIconSize}
                viewBox='0 0 24 24'
                fill='none'
                stroke='currentColor'
                strokeWidth='1.8'
                className={isEditMode ? 'text-white' : 'text-t-secondary'}
              >
                <path d='M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7' />
                <path d='M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z' />
              </svg>
              <span>{isEditMode ? t('preview.exitEdit') : t('preview.edit')}</span>
            </div>
          )}

          {isEditable && isEditMode && (
            <div
              className={`flex items-center px-8px py-3px rd-4px cursor-pointer transition-colors duration-150 ${isSplitScreenEnabled ? toolbarBtnActive : 'text-t-secondary hover:bg-3'}`}
              onClick={() => {
                try {
                  onSplitScreenToggle();
                } catch {
                  /* ignore */
                }
              }}
              title={isSplitScreenEnabled ? t('preview.closeSplitScreen') : t('preview.openSplitScreen')}
            >
              <svg
                width={fileActionIconSize}
                height={fileActionIconSize}
                viewBox='0 0 24 24'
                fill='none'
                stroke='currentColor'
                strokeWidth='2'
              >
                <rect x='3' y='3' width='18' height='18' rx='2' />
                <line x1='12' y1='3' x2='12' y2='21' />
              </svg>
            </div>
          )}

          {actionsInFront && showOpenInSystemButton && (
            <button
              type='button'
              className='preview-toolbar__icon-btn text-t-secondary hover:text-t-primary'
              aria-label={t('preview.openInSystemApp')}
              title={t('preview.openInSystemApp')}
              onClick={onOpenInSystem}
            >
              <svg
                width={fileActionIconSize}
                height={fileActionIconSize}
                viewBox='0 0 24 24'
                fill='none'
                stroke='currentColor'
                strokeWidth='2'
                aria-hidden='true'
              >
                <path d='M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6' />
                <polyline points='15 3 21 3 21 9' />
                <line x1='10' y1='14' x2='21' y2='3' />
              </svg>
            </button>
          )}
          {actionsInFront && (
            <button
              type='button'
              className='preview-toolbar__icon-btn text-t-secondary hover:text-t-primary'
              aria-label={t('common.download')}
              title={t('preview.downloadFile')}
              onClick={() => void onDownload()}
            >
              <svg
                width={fileActionIconSize}
                height={fileActionIconSize}
                viewBox='0 0 24 24'
                fill='none'
                stroke='currentColor'
                strokeWidth='2'
                aria-hidden='true'
              >
                <path d='M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4' />
                <polyline points='7 10 12 15 17 10' />
                <line x1='12' y1='15' x2='12' y2='3' />
              </svg>
            </button>
          )}
          {leftExtra}
        </div>

        <div className='preview-toolbar__actions flex items-center gap-4px flex-shrink-0'>
          {rightExtra}

          {snapshotButtonsVisible && (
            <>
              <div
                className={`${toolbarBtn} ${historyTarget ? '' : '!cursor-not-allowed opacity-50'} ${snapshotSaving ? 'opacity-60' : ''}`}
                onClick={historyTarget && !snapshotSaving ? onSaveSnapshot : undefined}
                title={historyTarget ? t('preview.saveSnapshot') : t('preview.snapshotNotSupported')}
              >
                <svg
                  width={toolbarIconSize}
                  height={toolbarIconSize}
                  viewBox='0 0 24 24'
                  fill='none'
                  stroke='currentColor'
                  strokeWidth='1.8'
                  className='text-t-secondary'
                >
                  <path d='M5 7h3l1-2h6l1 2h3a1 1 0 0 1 1 1v9a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a1 1 0 0 1 1-1Z' />
                  <circle cx='12' cy='13' r='3' />
                </svg>
                <span>{t('preview.snapshot')}</span>
              </div>
              {historyTarget ? (
                <Dropdown
                  droplist={renderHistoryDropdown()}
                  trigger={['hover']}
                  position='br'
                  onVisibleChange={(visible) => visible && onRefreshHistory()}
                >
                  <div className={toolbarBtn} title={t('preview.historyVersions')}>
                    <svg
                      width={toolbarIconSize}
                      height={toolbarIconSize}
                      viewBox='0 0 24 24'
                      fill='none'
                      stroke='currentColor'
                      strokeWidth='1.8'
                      className='text-t-secondary'
                    >
                      <path d='M12 8v5l3 2' />
                      <path d='M12 3a9 9 0 1 0 9 9' />
                      <polyline points='21 3 21 9 15 9' />
                    </svg>
                    <span>{t('preview.history')}</span>
                  </div>
                </Dropdown>
              ) : (
                <div
                  className={`${toolbarBtn} !cursor-not-allowed opacity-50`}
                  title={t('preview.historyNotSupported')}
                >
                  <svg
                    width={toolbarIconSize}
                    height={toolbarIconSize}
                    viewBox='0 0 24 24'
                    fill='none'
                    stroke='currentColor'
                    strokeWidth='1.8'
                    className='text-t-secondary'
                  >
                    <path d='M12 8v5l3 2' />
                    <path d='M12 3a9 9 0 1 0 9 9' />
                    <polyline points='21 3 21 9 15 9' />
                  </svg>
                  <span>{t('preview.history')}</span>
                </div>
              )}
            </>
          )}

          {!isImage && !actionsInFront && showOpenInSystemButton && (
            <button
              type='button'
              className='preview-toolbar__icon-btn text-t-secondary hover:text-t-primary'
              aria-label={t('preview.openInSystemApp')}
              title={t('preview.openInSystemApp')}
              onClick={onOpenInSystem}
            >
              <svg
                width={fileActionIconSize}
                height={fileActionIconSize}
                viewBox='0 0 24 24'
                fill='none'
                stroke='currentColor'
                strokeWidth='2'
                aria-hidden='true'
              >
                <path d='M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6' />
                <polyline points='15 3 21 3 21 9' />
                <line x1='10' y1='14' x2='21' y2='3' />
              </svg>
            </button>
          )}

          {isImage ? (
            <button
              type='button'
              className='preview-toolbar__icon-btn text-t-secondary hover:text-t-primary'
              aria-label={t('common.download')}
              title={t('preview.downloadFile')}
              onMouseDown={(event) => event.stopPropagation()}
              onClick={(event) => {
                event.preventDefault();
                event.stopPropagation();
                void onDownload();
              }}
            >
              <svg
                width={fileActionIconSize}
                height={fileActionIconSize}
                viewBox='0 0 24 24'
                fill='none'
                stroke='currentColor'
                strokeWidth='2'
                aria-hidden='true'
              >
                <path d='M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4' />
                <polyline points='7 10 12 15 17 10' />
                <line x1='12' y1='15' x2='12' y2='3' />
              </svg>
            </button>
          ) : (
            !actionsInFront && (
              <button
                type='button'
                className='preview-toolbar__icon-btn text-t-secondary hover:text-t-primary'
                aria-label={t('common.download')}
                title={t('preview.downloadFile')}
                onClick={() => void onDownload()}
              >
                <svg
                  width={fileActionIconSize}
                  height={fileActionIconSize}
                  viewBox='0 0 24 24'
                  fill='none'
                  stroke='currentColor'
                  strokeWidth='2'
                  aria-hidden='true'
                >
                  <path d='M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4' />
                  <polyline points='7 10 12 15 17 10' />
                  <line x1='12' y1='15' x2='12' y2='3' />
                </svg>
              </button>
            )
          )}
          {isImage && showRevealInFolder && (
            <button
              type='button'
              className='preview-toolbar__icon-btn text-t-secondary hover:text-t-primary'
              aria-label={t('preview.openFileLocation')}
              title={t('preview.openFileLocation')}
              onMouseDown={(event) => event.stopPropagation()}
              onClick={(event) => {
                event.preventDefault();
                event.stopPropagation();
                void onRevealInFolder?.();
              }}
            >
              <svg
                width={fileActionIconSize}
                height={fileActionIconSize}
                viewBox='0 0 24 24'
                fill='none'
                stroke='currentColor'
                strokeWidth='2'
                aria-hidden='true'
              >
                <path d='M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z' />
              </svg>
            </button>
          )}
        </div>
      </div>
    </div>
  );
};

export default PreviewToolbar;
