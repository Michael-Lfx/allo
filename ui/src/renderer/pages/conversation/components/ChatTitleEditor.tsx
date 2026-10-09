import MarqueeText from '@/renderer/components/base/MarqueeText';
import classNames from 'classnames';
import React from 'react';
import { useTranslation } from 'react-i18next';
import styles from './ChatTitleEditor.module.css';

type ChatTitleEditorProps = {
  editingTitle: boolean;
  titleDraft: string;
  setTitleDraft: (value: string) => void;
  setEditingTitle: (value: boolean) => void;
  renameLoading: boolean;
  canRenameTitle: boolean;
  submitTitleRename: () => Promise<void>;
  titleAreaMaxWidth: number;
  title: React.ReactNode;
  /** Optional read-only context rendered below the editable conversation title. */
  subtitle?: React.ReactNode;
  /** Optional leading icon (e.g. agent logo) rendered inside the hover region, just before the title */
  leading?: React.ReactNode;
};

// Inline title display with double-click-to-edit rename support
const ChatTitleEditor: React.FC<ChatTitleEditorProps> = ({
  editingTitle,
  titleDraft,
  setTitleDraft,
  setEditingTitle,
  renameLoading,
  canRenameTitle,
  submitTitleRename,
  titleAreaMaxWidth,
  title,
  subtitle,
  leading,
}) => {
  const { t } = useTranslation();
  const titleClassName = classNames(
    styles.title,
    'block min-w-0 overflow-hidden text-ellipsis whitespace-nowrap transition-colors duration-150',
    canRenameTitle && 'cursor-text focus:outline-none'
  );

  const handleTitleClick = () => {
    if (!canRenameTitle) return;
    setEditingTitle(true);
  };

  const handleTitleKeyDown = (event: React.KeyboardEvent<HTMLSpanElement>) => {
    if (!canRenameTitle) return;
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      setEditingTitle(true);
    }
  };

  return (
    <div
      className='flex min-w-0 max-w-full items-center gap-4px'
      style={{ width: '100%', maxWidth: `${titleAreaMaxWidth}px` }}
    >
      <div
        data-chat-title-label
        className={classNames(styles.surface, 'group flex min-w-0 flex-1 items-center rounded-8px')}
      >
        {leading && <div className='shrink-0 flex items-center pl-8px'>{leading}</div>}
        <div className='min-w-0 flex-1 flex flex-col justify-center px-8px py-5px'>
          {editingTitle && canRenameTitle ? (
            <input
              autoFocus
              value={titleDraft}
              disabled={renameLoading}
              className={styles.renameInput}
              maxLength={120}
              aria-label={t('conversation.history.renamePlaceholder')}
              onChange={(event) => setTitleDraft(event.target.value)}
              onFocus={(event) => {
                event.target.select();
              }}
              onBlur={() => {
                void submitTitleRename();
              }}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  void submitTitleRename();
                } else if (event.key === 'Escape') {
                  setTitleDraft(typeof title === 'string' ? title : '');
                  setEditingTitle(false);
                }
              }}
            />
          ) : typeof title === 'string' ? (
            <MarqueeText
              text={title}
              trigger='hoverOrFocus'
              role={canRenameTitle ? 'button' : undefined}
              tabIndex={canRenameTitle ? 0 : undefined}
              className={titleClassName}
              onClick={handleTitleClick}
              onKeyDown={handleTitleKeyDown}
            />
          ) : (
            <span
              role={canRenameTitle ? 'button' : undefined}
              tabIndex={canRenameTitle ? 0 : undefined}
              className={titleClassName}
              onClick={handleTitleClick}
              onKeyDown={handleTitleKeyDown}
            >
              {title}
            </span>
          )}
          {subtitle && (
            <div className={classNames(styles.subtitle, 'min-w-0')}>
              {subtitle}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default ChatTitleEditor;
