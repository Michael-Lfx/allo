

import FilePreview from '@/renderer/components/media/FilePreview';
import UploadProgressBar from '@/renderer/components/media/UploadProgressBar';
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
  activeShadow: string;
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
  activeShadow,
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

  return (
    <div
      ref={containerRef}
      className={`${styles.guidInputCardWrap} guid-input-card-shell relative rd-24px flex flex-col ${mentionOpen || slashMenuOpen ? 'overflow-visible' : 'overflow-hidden'} transition-all duration-200 ${isFileDragging ? 'b b-solid border-dashed guid-input-card-shell--dragging' : ''}`}
      style={{
        zIndex: 1,
        transition: 'box-shadow 0.25s ease',
        width: isMobile ? 'calc(100% + 28px)' : undefined,
        marginLeft: isMobile ? -14 : undefined,
        marginRight: isMobile ? -14 : undefined,
        ...(isFileDragging
          ? {
              backgroundColor: 'var(--color-primary-light-1)',
              borderColor: 'rgb(var(--primary-3))',
              borderWidth: '1px',
            }
          : {
              boxShadow: isInputActive && !slashMenuOpen ? activeShadow : 'none',
            }),
      }}
      {...dragHandlers}
    >
      {slashMenuOpen && (
        <div className='absolute left-0 right-0 bottom-[calc(100%+10px)] z-70'>
          {slashMenu}
        </div>
      )}
      {/* inner white card — narrower than outer wrap */}
      <div
        className={`${styles.guidInputInner} p-12px flex flex-col bg-dialog-fill-0`}
        style={{
          transition: 'box-shadow 0.25s ease, border-color 0.25s ease',
          borderColor: isFileDragging ? 'rgb(var(--primary-3))' : borderColor,
          boxShadow: isInputActive && !isFileDragging && !slashMenuOpen ? activeShadow : 'none',
        }}
      >
        {entryStrip}
        {mentionSelectorBadge}
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
        {files.length > 0 && (
          <div className='flex flex-wrap items-center gap-8px mt-12px mb-12px'>
            {files.map((path) => (
              <FilePreview key={path} path={path} onRemove={() => onRemoveFile(path)} />
            ))}
          </div>
        )}
        <UploadProgressBar source='sendbox' />
        {actionRow}
      </div>
      <GuidWorkspaceFootnote
        workspaceDir={workspaceDir}
        onSelectWorkspace={onSelectWorkspace}
        onClearWorkspace={onClearWorkspace}
      />
    </div>
  );
};

export default GuidInputCard;
