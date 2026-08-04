

import type { IConversationMcpStatus, IConversationMcpStatusKind } from '@/common/config/storage';
import { Button, Message, Trigger } from '@arco-design/web-react';
import { FolderOpen, Paperclip, Plus, Right, Shield } from '@icon-park/react';
import { useConversationContextSafe } from '@/renderer/hooks/context/ConversationContext';
import { isDesktopShell } from '@/renderer/utils/platform';
import { FileService } from '@/renderer/services/FileService';
import type { FileMetadata } from '@/renderer/services/FileService';
import { emitter } from '@/renderer/utils/emitter';
import React, { useCallback, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

interface FileAttachButtonProps {
  openFileSelector: () => void;
  onLocalFilesAdded?: (files: FileMetadata[]) => void;
  loadedMcpStatuses?: IConversationMcpStatus[];
}

const MenuItem: React.FC<{
  icon: React.ReactNode;
  label: React.ReactNode;
  description?: React.ReactNode;
  suffix?: React.ReactNode;
  onClick?: () => void;
  className?: string;
  title?: string;
}> = ({ icon, label, description, suffix, onClick, className = '', title }) => (
  <div
    className={`flex items-center gap-10px px-12px py-9px rounded-8px cursor-pointer hover:bg-fill-2 transition-colors text-14px text-t-primary select-none ${className}`}
    onClick={onClick}
    title={title}
  >
    <span className='flex-shrink-0 inline-flex items-center justify-center color-#86909c w-18px leading-none'>
      {icon}
    </span>
    <span className='min-w-0 flex-1'>
      <span className='block leading-none'>{label}</span>
      {description ? <span className='mt-4px block text-12px leading-16px text-t-secondary'>{description}</span> : null}
    </span>
    {suffix}
  </div>
);

const MCP_STATUS_CLASS_NAME: Record<IConversationMcpStatusKind, string> = {
  loaded: 'text-[var(--color-success-6)]',
  failed: 'text-[var(--color-danger-6)]',
  unsupported: 'text-[var(--color-warning-6)]',
};

const FileAttachButton: React.FC<FileAttachButtonProps> = ({
  openFileSelector,
  onLocalFilesAdded,
  loadedMcpStatuses,
}) => {
  const conversationContext = useConversationContextSafe();
  const { t } = useTranslation();
  const navigate = useNavigate();
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [uploading, setUploading] = useState(false);
  const [open, setOpen] = useState(false);
  const [mcpOpen, setMcpOpen] = useState(false);

  const mcpStatuses = loadedMcpStatuses ?? conversationContext?.loadedMcpStatuses ?? [];
  const handleOpenMcpSettings = useCallback(() => {
    setOpen(false);
    setMcpOpen(false);
    void navigate('/mcp');
  }, [navigate]);

  const handleLocalFileChange = useCallback(
    async (e: React.ChangeEvent<HTMLInputElement>) => {
      const fileList = e.target.files;
      if (!fileList || fileList.length === 0 || !onLocalFilesAdded) return;
      setUploading(true);
      try {
        const processed = await FileService.processDroppedFiles(
          fileList,
          conversationContext?.conversation_id != null ? conversationContext.conversation_id : undefined
        );
        if (processed.length > 0) onLocalFilesAdded(processed);
      } catch {
        Message.error(t('common.fileAttach.failed'));
      } finally {
        setUploading(false);
      }
      e.target.value = '';
    },
    [conversationContext?.conversation_id, onLocalFilesAdded, t]
  );

  const isDesktop = isDesktopShell();
  const hasMcpServers = mcpStatuses.length > 0;
  const plusIcon = <Plus theme='outline' size='16' strokeWidth={3} fill='currentColor' />;

  const cardStyle: React.CSSProperties = {
    backgroundColor: 'var(--color-bg-2, #fff)',
    borderRadius: 12,
    boxShadow: '0 4px 24px rgba(0,0,0,0.13)',
    border: '1px solid var(--color-border-1, #e5e6eb)',
    padding: '6px 0',
    minWidth: 220,
    zIndex: 1050,
  };

  if (isDesktop) {
    return (
      <Button
        type='secondary'
        shape='circle'
        size='small'
        className='sendbox-composer-plus-btn conversation-attach-plus-btn'
        icon={plusIcon}
        loading={uploading}
        disabled={uploading}
        onClick={() => emitter.emit('sendbox.open-add')}
        data-testid='nomi-attach-folder-btn'
      />
    );
  }

  const mcpPanel = (
    <div
      style={{
        ...cardStyle,
        minWidth: 220,
        width: 'min(320px, calc(100vw - 96px))',
        maxWidth: 320,
      }}
      onClick={(e) => e.stopPropagation()}
    >
      {mcpStatuses.map((item) => (
        <MenuItem
          key={`${item.mcp_server_id}-${item.name}-${item.status}`}
          icon={<Shield theme='outline' size={15} strokeWidth={2.5} />}
          label={item.name}
          suffix={
            item.status === 'loaded' ? undefined : (
              <span className={`text-12px leading-none ${MCP_STATUS_CLASS_NAME[item.status]}`}>
                {t(`conversation.mcp.status.${item.status}` as const)}
              </span>
            )
          }
          className='mx-6px cursor-default hover:bg-transparent'
          title={item.reason}
        />
      ))}
      <div style={{ margin: '4px 12px', height: 1, backgroundColor: 'var(--color-border-1, #e5e6eb)' }} />
      <div className='px-12px py-8px'>
        <div className='text-12px leading-16px text-t-secondary whitespace-normal break-words'>
          {t('conversation.mcp.managementHint', {
            defaultValue:
              'If an MCP looks abnormal, review its JSON configuration and run an availability check in MCP management.',
          })}
        </div>
        <Button
          type='text'
          size='mini'
          className='mt-6px h-auto! px-0! text-12px! inline-flex! items-center! gap-4px!'
          onClick={handleOpenMcpSettings}
        >
          <span className='leading-none'>
            {t('conversation.mcp.openSettings', {
              defaultValue: 'Open MCP management',
            })}
          </span>
          <span className='inline-flex h-12px w-12px flex-shrink-0 items-center justify-center'>
            <Right theme='outline' size={12} strokeWidth={3} className='block' />
          </span>
        </Button>
      </div>
    </div>
  );

  const menu = (
    <div style={cardStyle} onClick={(e) => e.stopPropagation()}>
      {hasMcpServers && (
        <>
          <div className='px-6px'>
            <Trigger
              popup={() => mcpPanel}
              trigger='hover'
              position='right'
              popupVisible={mcpOpen}
              onVisibleChange={setMcpOpen}
              mouseEnterDelay={100}
              mouseLeaveDelay={150}
            >
              <div>
                <MenuItem
                  icon={<Shield theme='outline' size={15} strokeWidth={2.5} />}
                  label={`${t('conversation.mcp.loaded', { defaultValue: 'Loaded MCP' })} · ${mcpStatuses.length}`}
                  suffix={<Right theme='outline' size={12} strokeWidth={3} style={{ color: '#c9cdd4' }} />}
                />
              </div>
            </Trigger>
          </div>
          <div style={{ margin: '4px 12px', height: 1, backgroundColor: 'var(--color-border-1, #e5e6eb)' }} />
        </>
      )}

      {/* 文件操作最常用，在最下（离 + 最近） */}
      <div className='px-6px'>
        {!isDesktop && (
          <MenuItem
            icon={<FolderOpen theme='outline' size={15} strokeWidth={2.5} />}
            label={t('common.fileAttach.myDevice', { defaultValue: 'Upload from device' })}
            onClick={() => {
              fileInputRef.current?.click();
              setOpen(false);
            }}
          />
        )}
        <MenuItem
          icon={<Paperclip theme='outline' size={15} strokeWidth={2.5} />}
          label={t('common.fileAttach.addFiles', { defaultValue: 'Add files' })}
          onClick={() => {
            openFileSelector();
            setOpen(false);
          }}
        />
      </div>
    </div>
  );

  return (
    <>
      <Trigger
        popup={() => menu}
        trigger='click'
        position='tl'
        popupVisible={open}
        onVisibleChange={setOpen}
        clickToClose
        popupAlign={{ bottom: 8 }}
      >
        <Button
          type='secondary'
          shape='circle'
          size='small'
          className='sendbox-composer-plus-btn conversation-attach-plus-btn'
          icon={plusIcon}
          loading={uploading}
          disabled={uploading}
          data-testid='nomi-attach-folder-btn'
        />
      </Trigger>
      <input
        ref={fileInputRef}
        type='file'
        multiple
        style={{ display: 'none' }}
        onChange={handleLocalFileChange}
        data-testid='nomi-file-upload-input'
      />
    </>
  );
};

export default FileAttachButton;
