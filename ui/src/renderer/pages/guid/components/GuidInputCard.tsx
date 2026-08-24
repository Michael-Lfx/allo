

import FilePreview from '@/renderer/components/media/FilePreview';
import HorizontalFileList from '@/renderer/components/media/HorizontalFileList';
import UploadProgressBar from '@/renderer/components/media/UploadProgressBar';
import ComposerSurface from '@/renderer/components/chat/ComposerSurface';
import ComposerSkillTokenInput, {
  type ComposerSkillTokenInputHandle,
  type ComposerTokenInputState,
} from '@/renderer/components/chat/ComposerSkillTokenInput';
import type { ComposerSkillChip } from '@/renderer/components/chat/composerSkill';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { useCompositionInput } from '@/renderer/hooks/chat/useCompositionInput';
import React from 'react';
import styles from '../index.module.css';
import GuidWorkspaceFootnote from './GuidWorkspaceFootnote';

const COMPOSER_MENU_BORDER_COLOR = 'color-mix(in srgb, var(--color-border-2) 68%, var(--color-bg-1))';

type GuidInputCardProps = {
  // Input state
  input: string;
  onInputChange: (value: string) => void;
  onKeyDown: (event: React.KeyboardEvent) => void;
  onPaste: React.ClipboardEventHandler;
  onFocus: () => void;
  onBlur: () => void;
  placeholder: string;

  // Styling
  isInputActive: boolean;
  isFileDragging: boolean;
  activeBorderColor: string;
  inactiveBorderColor: string;
  dragHandlers: React.HTMLAttributes<HTMLDivElement>;
  /**
   * Ref bound to the card's root div — used by the host for Tauri native
   * drag-drop hit-testing (desktop). Optional; omitted by non-desktop callers.
   */
  containerRef?: React.Ref<HTMLDivElement>;

  // Mention state
  mentionOpen: boolean;
  mentionSelectorBadge: React.ReactNode;
  mentionDropdown: React.ReactNode;

  // Slash command menu
  slashMenuOpen?: boolean;
  slashMenu?: React.ReactNode;

  // Explicit Skill selections for the first conversation turn.
  skillChips?: ComposerSkillChip[];
  onSkillChipsChange?: (skills: ComposerSkillChip[]) => void;
  onTokenInputStateChange?: (state: ComposerTokenInputState) => void;
  tokenInputRef?: React.Ref<ComposerSkillTokenInputHandle>;

  // Files
  files: string[];
  onRemoveFile: (path: string) => void;

  // Entry strip (slot rendered at top of inner card)
  entryStrip?: React.ReactNode;

  // Action row
  actionRow: React.ReactNode;

  // Workspace
  workspaceDir: string;
  onSelectWorkspace: (dir: string) => void;
  onClearWorkspace: () => void;
};

const GuidInputCard: React.FC<GuidInputCardProps> = ({
  input,
  onInputChange,
  onKeyDown,
  onPaste,
  onFocus,
  onBlur,
  placeholder,
  isInputActive,
  isFileDragging,
  activeBorderColor,
  inactiveBorderColor,
  dragHandlers,
  containerRef,
  mentionOpen,
  mentionSelectorBadge,
  mentionDropdown,
  slashMenuOpen = false,
  slashMenu,
  skillChips = [],
  onSkillChipsChange,
  onTokenInputStateChange,
  tokenInputRef,
  files,
  onRemoveFile,
  entryStrip,
  actionRow,
  workspaceDir,
  onSelectWorkspace,
  onClearWorkspace,
}) => {
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const { compositionHandlers, isImeActive } = useCompositionInput();
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (isImeActive(e)) return;
    onKeyDown(e);
  };

  const borderColor = isFileDragging
    ? 'rgb(var(--primary-3))'
    : slashMenuOpen
      ? COMPOSER_MENU_BORDER_COLOR
      : isInputActive
        ? activeBorderColor
        : inactiveBorderColor;

  // The home composer is centered content, not a bottom-anchored obstruction.
  return (
    <ComposerSurface
      registerNotificationBlocker={false}
      outerRef={containerRef}
      dragHandlers={dragHandlers}
      isOverlayOpen={mentionOpen || slashMenuOpen}
      className={`${styles.guidInputCardWrap} ${isInputActive ? styles.guidInputCardWrapActive : ''} guid-input-card-shell ${isFileDragging ? 'b b-solid border-dashed guid-input-card-shell--dragging' : ''}`}
      style={{
        zIndex: 1,
        width: isMobile ? 'calc(100% + 28px)' : undefined,
        marginLeft: isMobile ? -14 : undefined,
        marginRight: isMobile ? -14 : undefined,
        ...(isFileDragging
          ? {
              backgroundColor: 'var(--color-primary-light-1)',
              borderColor: 'rgb(var(--primary-3))',
              borderWidth: '1px',
            }
          : undefined),
      }}
      panelClassName={`${styles.guidInputInner} p-12px bg-dialog-fill-0`}
      panelStyle={{
        borderColor: isFileDragging ? 'rgb(var(--primary-3))' : borderColor,
        boxShadow: 'none',
      }}
      beforePanel={
        slashMenuOpen ? (
          <div className='absolute left-0 right-0 bottom-[calc(100%+10px)] z-70'>
            {slashMenu}
          </div>
        ) : null
      }
      afterPanel={
        <GuidWorkspaceFootnote
          workspaceDir={workspaceDir}
          onSelectWorkspace={onSelectWorkspace}
          onClearWorkspace={onClearWorkspace}
        />
      }
    >
        {entryStrip}
        {mentionSelectorBadge}
        {files.length > 0 && (
          <HorizontalFileList>
            {files.map((path) => (
              <FilePreview key={path} path={path} onRemove={() => onRemoveFile(path)} />
            ))}
          </HorizontalFileList>
        )}
        <ComposerSkillTokenInput
          ref={tokenInputRef}
          autoFocus
          className={`text-14px rounded-xl !bg-transparent ${styles.lightPlaceholder}`}
          style={{
            minHeight: isMobile ? '40px' : '40px',
            maxHeight: isMobile ? '160px' : '400px',
            overflowY: 'auto',
            paddingLeft: '7px',
            paddingRight: 0,
          }}
          placeholder={placeholder}
          value={input}
          skills={skillChips}
          onChange={onInputChange}
          onSkillsChange={onSkillChipsChange}
          onDraftStateChange={onTokenInputStateChange}
          onPaste={onPaste}
          onFocus={onFocus}
          onBlur={onBlur}
          {...compositionHandlers}
          onKeyDown={handleKeyDown}
          dataTestId='guid-input'
        />
        <div style={{ height: 12, flexShrink: 0 }} aria-hidden='true' />
        {mentionOpen && (
          <div className='absolute z-50' style={{ left: 16, top: 44 }}>
            {mentionDropdown}
          </div>
        )}
        <UploadProgressBar source='sendbox' />
        {actionRow}
    </ComposerSurface>
  );
};

export default GuidInputCard;
