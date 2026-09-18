

import React from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@arco-design/web-react';
import { Plus } from '@icon-park/react';
import classNames from 'classnames';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';
import styles from '../Sider.module.css';

type SiderNewConversationEntryProps = {
  isMobile: boolean;
  collapsed: boolean;
  siderTooltipProps: SiderTooltipProps;
  onClick: () => void;
  onNewTerminal?: () => void;
};

const SiderNewConversationEntry: React.FC<SiderNewConversationEntryProps> = ({
  isMobile,
  collapsed,
  siderTooltipProps,
  onClick,
}) => {
  const { t } = useTranslation();
  const label = t('common.tray.newChat');

  if (collapsed) {
    return (
      <Tooltip {...siderTooltipProps} content={label} position='right'>
        <button
          type='button'
          data-testid='sider-new-conversation-entry'
          className={classNames(
            'w-full h-34px flex items-center justify-center cursor-pointer shrink-0 transition-colors text-t-primary rd-8px hover:bg-fill-3 active:bg-fill-4 bg-transparent border-none p-0 focus:outline-none',
            styles.newChatTrigger
          )}
          onClick={onClick}
          aria-label={label}
        >
          <Plus
            theme='outline'
            size='16'
            fill='currentColor'
            className='block leading-none'
            style={{ lineHeight: 0 }}
          />
        </button>
      </Tooltip>
    );
  }

  // Expanded: 单独的新建对话全宽按钮，与搜索按钮完全统一对齐（左对齐、size-22px 图标盒、统一内边距与字体）
  return (
    <Tooltip {...siderTooltipProps} content={label} position='right'>
      <button
        type='button'
        data-testid='sider-new-conversation-entry'
        className={classNames(
          styles.newChatTrigger,
          'h-34px w-full flex items-center justify-start gap-8px pl-10px pr-8px shrink-0 rd-0.5rem border border-solid border-[var(--color-border-2)] bg-transparent text-t-primary cursor-pointer hover:bg-fill-2 active:bg-fill-3 transition-colors focus:outline-none focus-visible:outline-none select-none',
          isMobile && 'sider-action-btn-mobile'
        )}
        onClick={onClick}
        aria-label={label}
      >
        <span className='size-22px flex items-center justify-center shrink-0 text-t-primary'>
          <Plus
            theme='outline'
            size='16'
            fill='currentColor'
            className={classNames('block leading-none shrink-0', styles.newChatIcon)}
            style={{ lineHeight: 0 }}
          />
        </span>
        <span className='collapsed-hidden text-t-primary text-14px font-[500] leading-24px truncate'>
          {label}
        </span>
      </button>
    </Tooltip>
  );
};

export default SiderNewConversationEntry;
