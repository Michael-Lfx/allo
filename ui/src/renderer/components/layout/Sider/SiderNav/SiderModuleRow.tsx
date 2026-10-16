import React from 'react';
import { Tooltip } from '@arco-design/web-react';
import classNames from 'classnames';
import type { SiderTooltipProps } from '@renderer/utils/ui/siderTooltip';
import styles from '../Sider.module.css';

type SiderModuleRowProps = {
  collapsed: boolean;
  active: boolean;
  label: string;
  testId: string;
  siderTooltipProps: SiderTooltipProps;
  onClick: () => void;
  onPointerEnter?: () => void;
  icon: React.ReactNode;
};

/** Full-width sider row matching the new-task button: icon on the left, label beside it. */
const SiderModuleRow: React.FC<SiderModuleRowProps> = ({
  collapsed,
  active,
  label,
  testId,
  siderTooltipProps,
  onClick,
  onPointerEnter,
  icon,
}) => {
  if (collapsed) {
    return (
      <Tooltip {...siderTooltipProps} content={label} position='right'>
        <button
          type='button'
          role='tab'
          data-testid={testId}
          title={label}
          aria-selected={active}
          aria-label={label}
          className={classNames(
            'w-full h-34px flex items-center justify-center cursor-pointer shrink-0 transition-colors text-t-primary rd-8px border-none p-0 focus:outline-none',
            active ? 'bg-fill-3' : 'bg-transparent hover:bg-fill-3 active:bg-fill-4'
          )}
          onClick={onClick}
          onPointerEnter={onPointerEnter}
        >
          {icon}
        </button>
      </Tooltip>
    );
  }

  return (
    <Tooltip {...siderTooltipProps} content={label} position='right'>
      <button
        type='button'
        role='tab'
        data-testid={testId}
        title={label}
        aria-selected={active}
        aria-label={label}
        className={classNames(
          styles.newChatTrigger,
          'h-34px w-full flex items-center justify-start gap-8px pl-10px pr-8px shrink-0 rd-0.5rem border-none text-t-primary cursor-pointer transition-colors focus:outline-none focus-visible:outline-none select-none',
          active ? 'bg-fill-2' : 'bg-transparent hover:bg-fill-2 active:bg-fill-3'
        )}
        onClick={onClick}
        onPointerEnter={onPointerEnter}
      >
        <span className='size-22px flex items-center justify-center shrink-0 text-t-primary'>{icon}</span>
        <span className='collapsed-hidden text-t-primary text-14px font-[500] leading-24px truncate'>{label}</span>
      </button>
    </Tooltip>
  );
};

export default SiderModuleRow;
