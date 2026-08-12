import { ipcBridge } from '@/common';
import CopyIconButton from '@/renderer/components/base/CopyIconButton';
import { Input } from '@arco-design/web-react';
import { Check, Close, Down, FolderClose, FolderOpen } from '@icon-park/react';
import { isDesktopShell } from '@renderer/utils/platform';
import React, { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import WorkspacePickerPopover from './WorkspacePickerPopover';
import { DEFAULT_RECENT_WS_KEY, addRecentWorkspace, getRecentWorkspaces } from './recentWorkspaces';

type WorkspaceFolderSelectProps = {
  value?: string;
  onChange: (value: string) => void;
  onClear?: () => void;
  placeholder: string;
  input_placeholder?: string;
  recentLabel: string;
  chooseDifferentLabel: string;
  recentStorageKey?: string;
  triggerTestId?: string;
  menuTestId?: string;
};

const WorkspaceFolderSelect: React.FC<WorkspaceFolderSelectProps> = ({
  value,
  onChange,
  onClear,
  placeholder,
  input_placeholder,
  recentLabel,
  chooseDifferentLabel,
  recentStorageKey = DEFAULT_RECENT_WS_KEY,
  triggerTestId,
  menuTestId,
}) => {
  const { t } = useTranslation();
  const [menuVisible, setMenuVisible] = useState(false);
  const triggerRef = useRef<HTMLDivElement>(null);
  const isDesktop = isDesktopShell();
  const recentWorkspaces = getRecentWorkspaces(recentStorageKey);

  const handleBrowse = async () => {
    setMenuVisible(false);
    const files = await ipcBridge.dialog.showOpen.invoke({ properties: ['openDirectory', 'createDirectory'] });
    if (files?.[0]) {
      onChange(files[0]);
      addRecentWorkspace(files[0], recentStorageKey);
    }
  };

  const handleSelectRecent = (path: string) => {
    onChange(path);
    addRecentWorkspace(path, recentStorageKey);
    setMenuVisible(false);
  };

  const handleClear = (event: React.MouseEvent) => {
    event.stopPropagation();
    onClear?.();
    if (!onClear) onChange('');
    setMenuVisible(false);
  };

  const handleToggle = () => {
    if (recentWorkspaces.length === 0) {
      void handleBrowse();
      return;
    }
    setMenuVisible((visible) => !visible);
  };

  const folderName = value ? value.split(/[\\/]/).pop() || value : '';

  if (!isDesktop) {
    return <Input placeholder={input_placeholder ?? placeholder} value={value ?? ''} onChange={onChange} />;
  }

  return (
    <div className='relative' ref={triggerRef}>
      <div
        data-testid={triggerTestId}
        role='button'
        tabIndex={0}
        aria-expanded={menuVisible}
        aria-haspopup='menu'
        onClick={handleToggle}
        onKeyDown={(event) => {
          if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault();
            handleToggle();
          }
        }}
        className={`flex items-center gap-10px rounded-10px border px-12px py-10px transition-all ${
          menuVisible
            ? 'border-primary-5 bg-fill-2 shadow-sm'
            : 'border-border-2 bg-fill-1 hover:border-border-1 hover:bg-fill-2'
        }`}
      >
        <FolderOpen
          theme='outline'
          size='16'
          fill='currentColor'
          className='block shrink-0 text-t-secondary'
          style={{ transform: 'translateY(3px)' }}
        />
        {value ? (
          <div className='flex min-w-0 flex-1 flex-col justify-center'>
            <span className='text-sm leading-20px text-t-primary'>{folderName}</span>
            <span className='truncate text-11px leading-16px text-t-tertiary'>{value}</span>
          </div>
        ) : (
          <span className='min-w-0 flex-1 truncate text-sm leading-20px text-t-secondary'>{placeholder}</span>
        )}
        {value ? (
          <>
            <CopyIconButton
              text={value}
              tooltip={t('common.copyPath')}
              size={14}
              className='h-20px w-20px shrink-0 text-t-secondary hover:text-t-primary'
            />
            <span
              className='flex h-20px w-20px shrink-0 cursor-pointer items-center justify-center text-t-secondary transition-colors hover:text-t-primary'
              onClick={handleClear}
            >
              <Close theme='outline' size='14' fill='currentColor' />
            </span>
          </>
        ) : (
          <span className='flex h-20px w-20px shrink-0 items-center justify-center text-t-secondary'>
            <Down size='14' fill='currentColor' />
          </span>
        )}
      </div>

      <WorkspacePickerPopover
        open={menuVisible}
        onOpenChange={setMenuVisible}
        triggerRef={triggerRef}
        preferredPlacement='below'
        minWidth={230}
        maxWidth={480}
        matchTriggerWidth
        testId={menuTestId}
      >
        <div className='overflow-x-hidden overflow-y-auto p-6px'>
          {recentWorkspaces.length > 0 && (
            <>
              <div className='px-10px pb-4px pt-6px text-10px font-500 uppercase tracking-[0.08em] text-t-tertiary'>
                {recentLabel}
              </div>
              {recentWorkspaces.map((path) => {
                const recentName = path.split(/[\\/]/).pop() || path;
                const isSelected = value === path;
                return (
                  <button
                    type='button'
                    key={path}
                    onClick={() => handleSelectRecent(path)}
                    className={`flex w-full items-center gap-10px rounded-8px px-10px py-6px text-left transition-colors ${
                      isSelected ? 'bg-aou-1' : 'hover:bg-fill-2'
                    }`}
                    style={isSelected ? { boxShadow: 'inset 0 0 0 1px var(--aou-6)' } : undefined}
                  >
                    <FolderClose
                      theme='outline'
                      size='16'
                      fill='currentColor'
                      className={`block shrink-0 ${isSelected ? 'text-aou-6' : 'text-t-tertiary'}`}
                      style={{ transform: 'translateY(3px)' }}
                    />
                    <div className='min-w-0 flex-1'>
                      <div className='truncate text-13px leading-18px text-t-primary'>{recentName}</div>
                      <div className='truncate text-11px leading-14px text-t-tertiary'>{path}</div>
                    </div>
                    {isSelected && (
                      <span className='flex h-20px w-20px shrink-0 items-center justify-center text-aou-6'>
                        <Check size='14' fill='currentColor' />
                      </span>
                    )}
                  </button>
                );
              })}
              <div className='mx-2px my-4px h-1px bg-[var(--color-border-1)]' />
            </>
          )}

          <button
            type='button'
            onClick={() => void handleBrowse()}
            className='flex w-full items-center gap-10px rounded-8px px-10px py-6px text-left transition-colors hover:bg-fill-2'
          >
            <FolderOpen
              theme='outline'
              size='16'
              fill='currentColor'
              className='block shrink-0 text-t-tertiary'
              style={{ transform: 'translateY(3px)' }}
            />
            <span className='text-13px text-t-primary'>{chooseDifferentLabel}</span>
          </button>
        </div>
      </WorkspacePickerPopover>
    </div>
  );
};

export default WorkspaceFolderSelect;
