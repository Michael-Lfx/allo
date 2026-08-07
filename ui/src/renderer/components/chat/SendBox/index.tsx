

import type { ConversationId, MessageId } from '@/common/types/ids';
import { ipcBridge } from '@/common';
import AtFileMenu from '@/renderer/components/chat/AtFileMenu';
import BtwOverlay from '@/renderer/components/chat/BtwOverlay';
import { useInputFocusRing } from '@/renderer/hooks/chat/useInputFocusRing';
import SlashCommandMenu, { type SlashCommandMenuItem } from '@/renderer/components/chat/SlashCommandMenu';
import { useBtwCommand } from '@/renderer/components/chat/BtwOverlay/useBtwCommand';
import { parseGoalSlashCommand } from '@/common/chat/slash/goalCommand';
import { useGoalCommand } from '@/renderer/hooks/chat/useGoalCommand';
import {
  useSlashLauncherController,
  type SlashLauncherSelectionContext,
} from '@/renderer/hooks/chat/useSlashLauncherController';
import { useSkillCatalog } from '@/renderer/hooks/skills/useSkillCatalog';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { useConversationContextSafe } from '@/renderer/hooks/context/ConversationContext';
import { usePreviewContext } from '@/renderer/pages/conversation/Preview';
import { warmupConversation } from '@/renderer/pages/conversation/utils/warmupConversation';
import { buildAtFileInsertion, getActiveAtFileQuery, getAllAtFileQueries } from '@/renderer/utils/chat/atFileQuery';
import { getLastAssistantText } from '@/renderer/utils/chat/getLastAssistantText';
import { emitter, type ReplyQuote, useAddEventListener } from '@/renderer/utils/emitter';
import { mergeFileSelectionItems, type FileSelectionItem } from '@/renderer/utils/file/fileSelection';
import type { FileOrFolderItem } from '@/renderer/utils/file/fileTypes';
import { filterWorkspaceMentionItems } from '@/renderer/utils/file/workspaceMentions';
import { copyText } from '@/renderer/utils/ui/clipboard';
import { blurActiveElement, shouldBlockMobileInputFocus } from '@/renderer/utils/ui/focus';
import { Button, Input, Message, Tag } from '@arco-design/web-react';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';
import { Aiming, CloseSmall, Paperclip, Plus, Quote } from '@icon-park/react';
import type { SlashCommandItem } from '@/common/chat/slash/types';
import {
  groupSlashLauncherItems,
  replaceActiveSlashToken,
  type SlashLauncherItem,
} from '@/common/chat/slash/launcher';
import type { TFunction } from 'i18next';
import React, { useCallback, useDeferredValue, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useCompositionInput } from '@renderer/hooks/chat/useCompositionInput';
import { useConfig } from '@renderer/hooks/config/useConfig';
import { useConversationExport } from '@renderer/hooks/file/useConversationExport';
import { useDragUpload } from '@renderer/hooks/file/useDragUpload';
import { useLatestRef } from '@renderer/hooks/ui/useLatestRef';
import { usePasteService } from '@renderer/hooks/file/usePasteService';
import { useMessageList } from '@renderer/pages/conversation/Messages/hooks';
import type { FileMetadata } from '@renderer/services/FileService';
import { useUploadState } from '@renderer/hooks/file/useUploadState';
import { useAbortUploadsOnConversationChange } from '@renderer/hooks/file/useAbortUploadsOnConversationChange';
import UploadProgressBar from '@renderer/components/media/UploadProgressBar';
import { allSupportedExts } from '@renderer/services/FileService';
import ComposerSubmitCluster from '@/renderer/components/chat/ComposerSubmitCluster';
import ComposerSurface from '@/renderer/components/chat/ComposerSurface';
import type { ComposerSkillChip } from '@/renderer/components/chat/composerSkill';
import ComposerSkillTokenInput, {
  type ComposerSkillTokenInputHandle,
  type ComposerTokenInputState,
} from '@/renderer/components/chat/ComposerSkillTokenInput';
import type { ComposerDraft } from '@/renderer/components/chat/composerDraft';
import { appendSpeechTranscript } from '@/renderer/hooks/system/useSpeechInput';
import { getConversationInputHistory } from '@/renderer/utils/chat/messageHistory';
import { uuid } from '@/common/utils';
import { resolveEditResubmitOutcome } from '@/renderer/components/chat/SendBox/editResubmitOutcome';
import {
  clearEditingMessage,
  setEditingMessage,
  updateEditingMessage,
} from '@/renderer/pages/conversation/Messages/editingMessageStore';
import { markRetrySucceeded } from '@/renderer/utils/analytics/productFunnel';
import PinnedPlan from '@renderer/pages/conversation/Messages/components/PinnedPlan';
import { derivePinnedPlan } from '@renderer/pages/conversation/Messages/components/pinnedPlanModel';
import './sendbox.css';

const constVoid = (): void => undefined;
// 临界值：超过该字符数直接切换至多行模式，避免为超长文本做昂贵的宽度测量
// Threshold: switch to multi-line mode directly when character count exceeds this value to avoid heavy layout work
const MAX_SINGLE_LINE_CHARACTERS = 800;
const BTW_COMMAND_RE = /^\/btw(?:\s+([\s\S]*))?$/i;
const COMPOSER_MENU_BORDER_COLOR = 'color-mix(in srgb, var(--color-border-2) 68%, var(--color-bg-1))';

const getSelectedItemMatchKeys = (item: FileSelectionItem): string[] => {
  if (typeof item === 'string') {
    return [item];
  }
  return [item.relativePath, item.path].filter((value): value is string => Boolean(value));
};

const getSelectedItemPath = (item: FileSelectionItem): string | undefined => {
  if (typeof item === 'string') {
    return item;
  }
  return item.path;
};

const getSelectedItemDisplayLabel = (item: FileSelectionItem): string => {
  if (typeof item === 'string') {
    return item.split(/[\\/]/).pop() || item;
  }
  return item.relativePath || item.name || item.path;
};

const rememberSelectedItem = (itemsByPath: Map<string, FileSelectionItem>, item: FileSelectionItem): void => {
  const path = getSelectedItemPath(item);
  if (!path) {
    return;
  }

  const existing = itemsByPath.get(path);
  if (typeof existing === 'string' && typeof item !== 'string') {
    itemsByPath.set(path, item);
    return;
  }

  if (!existing) {
    itemsByPath.set(path, item);
  }
};

const areSelectionItemsEquivalent = (left: FileSelectionItem[], right: FileSelectionItem[]): boolean => {
  if (left.length !== right.length) {
    return false;
  }

  for (let index = 0; index < left.length; index += 1) {
    const leftItem = left[index];
    const rightItem = right[index];
    if (leftItem === rightItem) {
      continue;
    }

    if (typeof leftItem !== typeof rightItem) {
      return false;
    }

    if (getSelectedItemPath(leftItem) !== getSelectedItemPath(rightItem)) {
      return false;
    }
  }

  return true;
};

const buildOwnedSelectionItems = (
  currentItems: FileSelectionItem[],
  mentionOwnedPaths: Set<string>,
  externalOwnedPaths: Set<string>,
  itemsByPath: Map<string, FileSelectionItem>
): FileSelectionItem[] => {
  const ownedPaths = new Set([...mentionOwnedPaths, ...externalOwnedPaths]);
  const nextItems: FileSelectionItem[] = [];
  const seenPaths = new Set<string>();

  for (const item of currentItems) {
    const path = getSelectedItemPath(item);
    if (!path || seenPaths.has(path) || !ownedPaths.has(path)) {
      continue;
    }

    nextItems.push(item);
    seenPaths.add(path);
  }

  for (const path of ownedPaths) {
    if (seenPaths.has(path)) {
      continue;
    }

    nextItems.push(itemsByPath.get(path) ?? path);
    seenPaths.add(path);
  }

  return nextItems;
};

function extractBtwQuestion(value: string): string | null {
  const match = value.trim().match(BTW_COMMAND_RE);
  return match ? match[1] || '' : null;
}

function getSlashCommandDescription(command: SlashCommandItem, t: TFunction): string {
  if (command.source !== 'builtin' && command.name === 'compact') {
    return t('conversation.slashCommands.compact.description', { defaultValue: command.description });
  }
  return command.description;
}

function getSkillSourceLabel(source: string, t: TFunction): string {
  return t(`conversation.skills.sources.${source}`, { defaultValue: source });
}

const SendBox: React.FC<{
  value?: string;
  onChange?: (value: string) => void;
  onSend: (message: string) => Promise<void>;
  /** Atomic Skill loading for runtimes that support the structured load plan. */
  onSendWithSkills?: (message: string, skillIds: string[]) => Promise<void>;
  skillChips?: ComposerSkillChip[];
  onSkillChipsChange?: (skills: ComposerSkillChip[]) => void;
  onStop?: () => Promise<void>;
  /** When provided AND a turn is running AND there is a draft, a secondary
   *  "steer now" action is offered alongside the (enqueue) send button. */
  onSteer?: (message: string) => Promise<void>;
  /** Gate the steer affordance (e.g. only for the Nomi native engine). */
  steerAvailable?: boolean;
  /** When provided (Nomi only), enables "edit a sent message" mode: the message text is
   *  recalled into the composer via the `sendbox.edit` event and submitting calls this
   *  instead of onSend, which truncates the conversation and re-runs from that message. */
  onEditResubmit?: (msgId: MessageId, createdAt: number, message: string) => Promise<void>;
  /** Clear the agent's conversation context (release model context). When set, a `/clear` builtin appears. */
  onClearContext?: () => void | Promise<void>;
  disabled?: boolean;
  loading?: boolean;
  className?: string;
  tools?: React.ReactNode;
  rightTools?: React.ReactNode;
  /** Conversation-only: compact status control rendered inside the composer header row. */
  topRightTools?: React.ReactNode;
  prefix?: React.ReactNode;
  placeholder?: string;
  onFilesAdded?: (files: FileMetadata[]) => void;
  supportedExts?: string[];
  defaultMultiLine?: boolean;
  lockMultiLine?: boolean;
  sendButtonPrefix?: React.ReactNode;
  slash_commands?: SlashCommandItem[];
  onSlashBuiltinCommand?: (name: string) => void;
  onAddFiles?: () => void;
  enableGoalMenu?: boolean;
  /** Conversation-only: the next regular message will become the goal objective. */
  goalModeArmed?: boolean;
  onGoalModeChange?: (enabled: boolean) => void;
  hasPendingAttachments?: boolean;
  enableBtw?: boolean;
  allowSendWhileLoading?: boolean;
  compactActions?: boolean;
  selectedWorkspaceItems?: FileSelectionItem[];
  onSelectedWorkspaceItemsChange?: (items: FileSelectionItem[]) => void;
  bottomHint?: React.ReactNode;
  /** Conversation-only: render the compact plan strip inside the input panel status row. */
  showPinnedPlan?: boolean;
  /**
   * Mobile-only: open a parent-supplied action sheet via the `+` button.
   * When provided, mobile renders a single `+` button (left) and send/stop button (right);
   * `tools` and `rightTools` are not rendered inline on mobile.
   */
  onMobilePlusClick?: () => void;
}> = ({
  onSend,
  onSendWithSkills,
  skillChips = [],
  onSkillChipsChange,
  onStop,
  onSteer,
  steerAvailable,
  onEditResubmit,
  onClearContext,
  prefix,
  className,
  loading,
  tools,
  rightTools,
  topRightTools,
  disabled,
  placeholder,
  value: input = '',
  onChange: setInput = constVoid,
  onFilesAdded,
  supportedExts = allSupportedExts,
  defaultMultiLine = false,
  lockMultiLine = false,
  sendButtonPrefix,
  slash_commands = [],
  onSlashBuiltinCommand,
  onAddFiles,
  enableGoalMenu = false,
  goalModeArmed = false,
  onGoalModeChange,
  hasPendingAttachments = false,
  enableBtw = false,
  allowSendWhileLoading = false,
  compactActions = false,
  selectedWorkspaceItems,
  onSelectedWorkspaceItemsChange,
  bottomHint,
  showPinnedPlan = false,
  onMobilePlusClick,
}) => {
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  // Mobile compact mode: parent supplies the `+` action sheet, which collapses
  // tools/rightTools into a single launcher and lets the textarea start as a single line.
  const isMobileCompact = isMobile && Boolean(onMobilePlusClick);
  const effectiveLockMultiLine = lockMultiLine && !isMobileCompact;
  const effectiveDefaultMultiLine = defaultMultiLine && !isMobileCompact;
  const conversationContext = useConversationContextSafe();
  const { t, i18n } = useTranslation();
  const [isLoading, setIsLoading] = useState(false);
  const [isStopping, setIsStopping] = useState(false);
  const isStoppingRef = useRef(false);
  const [isSingleLine, setIsSingleLine] = useState(!effectiveDefaultMultiLine);
  const [isInputFocused, setIsInputFocused] = useState(false);
  const [isAddMenuOpen, setIsAddMenuOpen] = useState(false);
  const [addMenuActiveIndex, setAddMenuActiveIndex] = useState(0);
  const isInputActive = isInputFocused;
  const { activeBorderColor, inactiveBorderColor } = useInputFocusRing();
  const containerRef = useRef<HTMLDivElement>(null);
  // Outer wrapper ref (the `relative` shell around `.sendbox-panel`). Used as the
  // Tauri native drag-drop hit-test scope so the catch includes floating children
  // that live OUTSIDE the panel box — notably PinnedPlan, rendered as a sibling
  // above `.sendbox-panel` — letting drops on it attach instead of being imported
  // by the workspace catch-all. `containerRef` (the panel) stays anchored to
  // BtwOverlay / textarea queries; this ref is drag-drop only.
  const dropzoneRef = useRef<HTMLDivElement>(null);
  const tokenInputRef = useRef<ComposerSkillTokenInputHandle>(null);
  const lastSubmittedDraftRef = useRef<ComposerDraft | null>(null);
  const [tokenInputState, setTokenInputState] = useState<ComposerTokenInputState>({
    projection: input,
    selection: { start: input.length, end: input.length },
    textSelection: { start: input.length, end: input.length },
  });
  const [singleLineWidth, setSingleLineWidth] = useState(0);
  const measurementCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const mobileUserFocusIntentUntilRef = useRef(0);
  const warmedConversationRef = useRef<ConversationId | undefined>(undefined);
  const warmupTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const latestInputRef = useLatestRef(input);
  const setInputRef = useLatestRef(setInput);
  const messageList = useMessageList();
  const pinnedPlan = useMemo(() => (showPinnedPlan ? derivePinnedPlan(messageList) : null), [messageList, showPinnedPlan]);
  const hasInternalStatusRow = Boolean(topRightTools);
  const [historyNavigationIndex, setHistoryNavigationIndex] = useState<number | null>(null);
  const historyDraftRef = useRef<string | null>(null);
  const [replyQuote, setReplyQuote] = useState<ReplyQuote | null>(null);
  const [editingMsgId, setEditingMsgId] = useState<MessageId | null>(null);
  const editingCreatedAtRef = useRef<number>(0);
  const editPrevDraftRef = useRef<string | null>(null);
  // C3: 编辑态 store 的 owner 标识（本实例唯一、仅 owner 可清，双实例护栏）与
  // 最新会话 id（事件监听器闭包可能捕获旧 conversationContext，写 store 前取 ref）。
  // C3: owner identity in the editing store (unique per instance, only the owner
  // may clear — dual-instance guard) plus the latest conversation id (event
  // listener closures may capture a stale conversationContext; read the ref).
  const editingOwnerIdRef = useRef<string | undefined>(undefined);
  const conversationIdRef = useLatestRef(conversationContext?.conversation_id);
  const editingOwnerId = (): string => {
    if (editingOwnerIdRef.current === undefined) {
      editingOwnerIdRef.current = uuid();
    }
    return editingOwnerIdRef.current;
  };
  // Edit-resubmit operation token: every UI side effect in the submit
  // promise chain (.then/.catch/.finally) is gated on this so a stale
  // operation can never clobber a newer one's input/edit/loading state.
  const activeEditOperationRef = useRef<string | null>(null);
  // Monotonic input revision (bumped on every `input` change) so the success
  // callback can tell "user typed during the request" from "string happened to
  // match" — only clear input when the user never touched it post-submit.
  const inputRevisionRef = useRef(0);
  const [caretPosition, setCaretPosition] = useState(0);
  const [workspaceMentionItems, setWorkspaceMentionItems] = useState<FileOrFolderItem[]>([]);
  const [workspaceMentionLoading, setWorkspaceMentionLoading] = useState(false);
  const [atFileMenuActiveIndex, setAtFileMenuActiveIndex] = useState(0);
  const [dismissedAtFileToken, setDismissedAtFileToken] = useState<string | null>(null);
  const mentionOwnedPathsRef = useRef<Set<string>>(new Set());
  const everMentionOwnedPathsRef = useRef<Set<string>>(new Set());
  const externalOwnedPathsRef = useRef<Set<string>>(new Set());
  const selectedItemByPathRef = useRef<Map<string, FileSelectionItem>>(new Map());
  const suppressedExternalAppendPathsRef = useRef<Set<string>>(new Set());
  const fetchedAtFileSessionKeyRef = useRef<string | null>(null);

  // Listen for reply events from message actions
  useAddEventListener('sendbox.reply', (quote) => setReplyQuote(quote), []);
  useAddEventListener('sendbox.reply.clear', () => setReplyQuote(null), []);
  useAddEventListener(
    'sendbox.open-add',
    () => {
      setAddMenuActiveIndex(0);
      setIsAddMenuOpen(true);
    },
    []
  );

  // 编辑已发送消息：把原文回填输入框，进入"编辑模式"，提交即截断重跑（仅 Nomi 提供 onEditResubmit）
  useAddEventListener(
    'sendbox.edit',
    (payload) => {
      if (!onEditResubmit) return;
      editPrevDraftRef.current = latestInputRef.current;
      editingCreatedAtRef.current = payload.createdAt;
      setEditingMsgId(payload.msgId);
      setReplyQuote(null);
      onSkillChipsChange?.([]);
      setInputRef.current(payload.content);
      requestAnimationFrame(() => {
        tokenInputRef.current?.focusAtTextOffset(payload.content.length);
      });
      // C3: 登记编辑中气泡（待提交，pending=false）。
      // C3: register the bubble being edited (recalled, not yet submitted).
      const cid = conversationIdRef.current;
      if (cid) {
        setEditingMessage(cid, { ownerId: editingOwnerId(), msgId: payload.msgId, pending: false });
      }
    },
    [onEditResubmit, onSkillChipsChange]
  );

  // C3: 卸载时清理本实例在编辑 store 中的登记（owner-guarded，双实例互不干扰）。
  // C3: clear this instance's editing-store registration on unmount (owner-guarded;
  // a sibling SendBox's registration is untouched).
  useEffect(() => {
    return () => {
      const owner = editingOwnerIdRef.current;
      const cid = conversationIdRef.current;
      if (owner !== undefined && cid) clearEditingMessage(cid, owner);
    };
  }, []);

  useAddEventListener(
    'sendbox.retry',
    (payload) => {
      if (disabled || loading || isLoading) return;
      const content = payload.content?.trim();
      if (!content) return;
      void (async () => {
        // Snapshot the input revision so a retry failure restores the retried
        // text only when the user never touched the composer mid-flight — never
        // overwriting new typing (same protection as the edit-resubmit path).
        const submittedInputRevision = inputRevisionRef.current;
        setIsLoading(true);
        try {
          if (onEditResubmit && payload.msgId && payload.createdAt) {
            await onEditResubmit(payload.msgId, payload.createdAt, content);
          } else {
            setInputRef.current(content);
            await onSend(content);
          }
          markRetrySucceeded(payload.msgId ?? `retry-${Date.now()}`, {
            conversation_type: conversationContext?.type,
          });
        } catch {
          if (inputRevisionRef.current === submittedInputRevision) {
            setInput(content);
          }
        } finally {
          setIsLoading(false);
        }
      })();
    },
    [conversationContext?.type, disabled, loading, isLoading, onEditResubmit, onSend]
  );

  // Bump the input revision on every composer change so the edit-resubmit
  // success/failure callbacks can detect mid-flight user edits by revision
  // drift rather than brittle string equality.
  useEffect(() => {
    inputRevisionRef.current += 1;
  }, [input]);

  // Invalidate any in-flight edit-resubmit operation on unmount so its late
  // callbacks cannot touch state (the token check makes them no-ops).
  useEffect(() => {
    return () => {
      activeEditOperationRef.current = null;
    };
  }, []);

  // 集成预览面板的"添加到聊天"功能 / Integrate preview panel's "Add to chat" functionality
  const { setSendBoxHandler, domSnippets, removeDomSnippet, clearDomSnippets } = usePreviewContext();

  // 注册处理器以接收来自预览面板的文本 / Register handler to receive text from preview panel
  useEffect(() => {
    const handler = (text: string) => {
      const base = latestInputRef.current;
      const insertion = base ? `\n\n${text}` : text;
      if (tokenInputRef.current) {
        tokenInputRef.current.focusAtTextOffset(base.length);
        tokenInputRef.current.insertTextAtSelection(insertion);
      } else {
        setInputRef.current(`${base}${insertion}`);
      }
    };
    setSendBoxHandler(handler);
    return () => {
      setSendBoxHandler(null);
    };
  }, [setSendBoxHandler]);

  // Track the single-line input width. Inline Skill chips can change this
  // without changing the draft, so a mount-only measurement is insufficient.
  useLayoutEffect(() => {
    if (!isSingleLine) {
      return;
    }

    const inputElement = containerRef.current?.querySelector<HTMLElement>('[data-testid="sendbox-input"]');
    if (!inputElement) {
      return;
    }

    const updateWidth = () => {
      const width = Math.floor(inputElement.getBoundingClientRect().width);
      if (width > 0) {
        setSingleLineWidth((currentWidth) => (currentWidth === width ? currentWidth : width));
      }
    };

    updateWidth();
    if (typeof ResizeObserver === 'undefined') {
      return;
    }

    const observer = new ResizeObserver(updateWidth);
    observer.observe(inputElement);
    return () => observer.disconnect();
  }, [isSingleLine, skillChips.length]);

  // 移动端挂载后主动清除焦点，拦截路由切换导致的非用户触发聚焦
  useEffect(() => {
    if (!isMobile) return;
    const timer = setTimeout(() => {
      blurActiveElement();
    }, 0);
    return () => clearTimeout(timer);
  }, [isMobile]);

  // 检测是否单行
  // Detect whether to use single-line or multi-line mode
  useEffect(() => {
    // 有换行符直接多行
    // Switch to multi-line mode if newline character exists
    if (input.includes('\n')) {
      setIsSingleLine(false);
      return;
    }

    // 还没获取到基准宽度时不做判断
    // Skip detection if baseline width is not yet obtained
    if (singleLineWidth === 0) {
      return;
    }

    // 长文本无需测量，直接切换多行，防止创建超宽 DOM 触发长时间布局计算
    // Skip measurement for long text and switch to multi-line immediately to avoid expensive layout caused by extra-wide DOM
    if (input.length >= MAX_SINGLE_LINE_CHARACTERS) {
      setIsSingleLine(false);
      return;
    }

    // 检测内容宽度
    // Detect content width
    const frame = requestAnimationFrame(() => {
      const inputElement = containerRef.current?.querySelector<HTMLElement>('[data-testid="sendbox-input"]');
      if (!inputElement) {
        return;
      }

      // 复用单个离屏 canvas，防止持续创建/销毁元素
      // Reuse a single offscreen canvas to avoid creating/destroying DOM nodes repeatedly
      const canvas = measurementCanvasRef.current ?? document.createElement('canvas');
      if (!measurementCanvasRef.current) {
        measurementCanvasRef.current = canvas;
      }
      const context = canvas.getContext('2d');
      if (!context) {
        return;
      }

      const inputStyle = getComputedStyle(inputElement);
      const fallbackFontSize = inputStyle.fontSize || '14px';
      const fallbackFontFamily = inputStyle.fontFamily || 'sans-serif';
      context.font = inputStyle.font || `${fallbackFontSize} ${fallbackFontFamily}`.trim();

      const textWidth = Math.max(context.measureText(input || '').width, inputElement.scrollWidth);

      // 使用初始化时保存的固定宽度作为判断基准
      // Use the fixed baseline width saved during initialization
      const baseWidth = singleLineWidth;

      // 文本宽度超过基准宽度时切换到多行
      // Switch to multi-line when text width exceeds baseline width
      if (textWidth >= baseWidth) {
        setIsSingleLine(false);
      } else if (textWidth < baseWidth - 30 && !effectiveLockMultiLine) {
        // 文本宽度小于基准宽度减30px时切回单行，留出小缓冲区避免临界点抖动
        // 如果 lockMultiLine 为 true，则不切换回单行
        // Switch back to single-line when text width is less than baseline minus 30px, leaving a small buffer to avoid flickering at the threshold
        // If lockMultiLine is true, do not switch back to single-line
        setIsSingleLine(true);
      }
      // 在 (baseWidth-30) 到 baseWidth 之间保持当前状态
      // Maintain current state between (baseWidth-30) and baseWidth
    });

    return () => cancelAnimationFrame(frame);
  }, [effectiveLockMultiLine, input, singleLineWidth]);

  // 使用拖拽 hook
  const { isFileDragging, dragHandlers } = useDragUpload({
    onFilesAdded,
    containerRef: dropzoneRef,
    conversation_id:
      conversationContext?.conversation_id != null ? conversationContext.conversation_id : undefined,
  });

  const { isUploading } = useUploadState('sendbox');
  // Bind sendbox uploads to the current conversation's lifecycle: switching
  // conversations or unmounting the SendBox aborts anything still in flight.
  useAbortUploadsOnConversationChange(
    conversationContext?.conversation_id != null ? conversationContext.conversation_id : undefined,
    'sendbox'
  );
  const [message, context] = useArcoMessage();
  const conversationExport = useConversationExport({
    conversation_id: conversationContext?.conversation_id,
    workspace: conversationContext?.workspace,
    t,
    messageApi: message,
  });
  const btwCommand = useBtwCommand(conversationContext?.conversation_id, enableBtw);
  const btwQuestion = useMemo(() => extractBtwQuestion(input), [input]);
  // /goal 与 /subgoal 家族命令为 host-resolved：提交时拦截并映射为 goal API 调用，
  // 不作为普通消息发给 agent。仅 nomi 会话启用——后端 goal 端点只支持 nomi runtime。
  const goalCommand = useGoalCommand(
    conversationContext?.conversation_id != null ? conversationContext.conversation_id : undefined,
    conversationContext?.type === 'nomi'
  );
  const activeAtFileQuery = useMemo(() => {
    if (!conversationContext?.workspace) {
      return null;
    }
    return getActiveAtFileQuery(input, caretPosition);
  }, [caretPosition, conversationContext?.workspace, input]);
  const activeAtFileTokenKey = useMemo(() => {
    if (!activeAtFileQuery) {
      return null;
    }
    return `${activeAtFileQuery.start}:${activeAtFileQuery.rawQuery}`;
  }, [activeAtFileQuery]);
  const atFileSessionKey = useMemo(() => {
    if (!conversationContext?.workspace || !activeAtFileQuery) {
      return null;
    }
    return `${conversationContext.workspace}:${activeAtFileQuery.start}`;
  }, [activeAtFileQuery, conversationContext?.workspace]);
  const allAtFileQueries = useMemo(() => getAllAtFileQueries(input), [input]);
  const deferredAtFileQuery = useDeferredValue(activeAtFileQuery?.query ?? '');
  const inputHistory = useMemo(
    () => getConversationInputHistory(messageList, conversationContext?.conversation_id),
    [conversationContext?.conversation_id, messageList]
  );
  const unmatchedSelectedWorkspaceItems = useMemo(() => {
    if (!selectedWorkspaceItems?.length) {
      return [];
    }

    const mentionQueries = new Set(allAtFileQueries.map((item) => item.query));
    return selectedWorkspaceItems.filter((item) => {
      if (typeof item !== 'string' && !item.isFile) {
        return false;
      }
      return !getSelectedItemMatchKeys(item).some((key) => mentionQueries.has(key));
    });
  }, [allAtFileQueries, selectedWorkspaceItems]);

  const builtinSlashCommands = useMemo<SlashCommandItem[]>(() => {
    const commands: SlashCommandItem[] = [];
    if (enableBtw) {
      commands.push({
        name: 'btw',
        description: t('conversation.sideQuestion.description'),
        kind: 'builtin',
        source: 'builtin',
        selectionBehavior: 'insert',
      });
    }
    if (onSlashBuiltinCommand) {
      commands.push({
        name: 'open',
        description: t('conversation.workspace.addFile', { defaultValue: 'Add File' }),
        kind: 'builtin',
        source: 'builtin',
      });
    }
    if (conversationContext?.conversation_id) {
      commands.push({
        name: 'copy',
        description: t('messages.copy', { defaultValue: 'Copy' }),
        kind: 'builtin',
        source: 'builtin',
      });
      commands.push({
        name: 'export',
        description: t('messages.export.commandDescription'),
        kind: 'builtin',
        source: 'builtin',
      });
    }
    if (conversationContext?.conversation_id && onClearContext) {
      commands.push({
        name: 'clear',
        description: t('conversation.clearContext.description', { defaultValue: 'Clear conversation context' }),
        kind: 'builtin',
        source: 'builtin',
      });
    }
    return commands;
  }, [conversationContext?.conversation_id, enableBtw, onClearContext, onSlashBuiltinCommand, t]);

  const { skills: catalogSkills } = useSkillCatalog(Boolean(onSendWithSkills && onSkillChipsChange));
  const launcherItems = useMemo<SlashLauncherItem[]>(
    () => [
      ...builtinSlashCommands.map((command) => ({
        id: `system:${command.name}`,
        kind: 'system' as const,
        name: command.name,
        description: getSlashCommandDescription(command, t),
      })),
      ...slash_commands.map((command) => ({
        id: `${command.source === 'builtin' ? 'system' : 'agent'}:${command.source}:${command.name}`,
        kind: command.source === 'builtin' ? ('system' as const) : ('agent' as const),
        name: command.name,
        description: getSlashCommandDescription(command, t),
      })),
      ...catalogSkills.map((skill) => ({
        id: skill.skillId,
        kind: 'skill' as const,
        name: skill.name,
        description: skill.description,
        source: getSkillSourceLabel(skill.source, t),
      })),
    ],
    [builtinSlashCommands, catalogSkills, slash_commands, t],
  );

  const orderedLauncherItems = useMemo(
    () => groupSlashLauncherItems(launcherItems).flatMap(({ items }) => items),
    [launcherItems]
  );

  const goalLauncherItem = useMemo(
    () => launcherItems.find((item) => item.kind === 'system' && item.name === 'goal'),
    [launcherItems]
  );

  const insertSlashCommandAtSelection = useCallback(
    (name: string) => {
      const beforeCaret = tokenInputState.projection.slice(0, tokenInputState.selection.end);
      const separator = beforeCaret.length > 0 && /[a-zA-Z0-9_./:-]$/.test(beforeCaret) ? ' ' : '';
      tokenInputRef.current?.focus();
      tokenInputRef.current?.insertTextAtSelection(`${separator}/${name} `);
    },
    [tokenInputState.projection, tokenInputState.selection.end]
  );

  const slashController = useSlashLauncherController({
    input: tokenInputState.projection,
    caretPosition: tokenInputState.selection.end,
    items: orderedLauncherItems,
    onExecuteSystem: (item, context: SlashLauncherSelectionContext) => {
      const goalInvocation = parseGoalSlashCommand(`/${item.name}`);
      if (goalInvocation) {
        if (goalInvocation.action === 'start') {
          onGoalModeChange?.(true);
          if (!context.manual && !tokenInputRef.current?.replaceActiveSlashToken()) {
            setInput(replaceActiveSlashToken(input, '', tokenInputState.textSelection.end));
          }
          return;
        }
        if (context.manual) {
          insertSlashCommandAtSelection(item.name);
          return;
        }
        if (!goalCommand.enabled) {
          if (!tokenInputRef.current?.replaceActiveSlashToken(`/${item.name} `)) {
            setInput(replaceActiveSlashToken(input, `/${item.name} `, tokenInputState.textSelection.end));
          }
          return;
        }
        setInput('');
        void goalCommand.run(goalInvocation);
        return;
      }
      if (item.name === 'btw') {
        if (context.manual) {
          insertSlashCommandAtSelection(item.name);
          return;
        }
        if (!tokenInputRef.current?.replaceActiveSlashToken('/btw ')) {
          setInput(replaceActiveSlashToken(input, '/btw ', tokenInputState.textSelection.end));
        }
        return;
      }
      if (item.name === 'copy') {
        const lastAssistantText = getLastAssistantText(messageList, Boolean(loading));
        if (!lastAssistantText) {
          Message.warning(t('messages.copyLastOutput.empty'));
        } else {
          void copyText(lastAssistantText)
            .then(() => {
              Message.success(t('messages.copySuccess'));
            })
            .catch(() => {
              Message.error(t('messages.copyFailed'));
            });
        }
      } else if (item.name === 'export') {
        void conversationExport.openExportFlow();
      } else if (item.name === 'clear') {
        void Promise.resolve(onClearContext?.());
      } else {
        onSlashBuiltinCommand?.(item.name);
      }
      if (!context.manual) {
        tokenInputRef.current?.replaceActiveSlashToken();
      }
    },
    onSelectSkill: (item) => {
      const skill = catalogSkills.find((candidate) => candidate.skillId === item.id);
      if (!skill || !onSkillChipsChange) {
        return;
      }
      tokenInputRef.current?.insertSkillAtActiveSlash({
        skillId: skill.skillId,
        name: skill.name,
        source: getSkillSourceLabel(skill.source, t),
      });
    },
    onSelectAgent: (item, context) => {
      if (context.manual) {
        insertSlashCommandAtSelection(item.name);
        return;
      }
      if (!tokenInputRef.current?.replaceActiveSlashToken(`/${item.name} `)) {
        setInput(replaceActiveSlashToken(input, `/${item.name} `, tokenInputState.textSelection.end));
      }
    },
  });

  const slashMenuItems = useMemo<SlashCommandMenuItem[]>(
    () =>
      slashController.filteredItems.map((item) => ({
        key: item.id,
        label: item.kind === 'skill' ? item.name : `/${item.name}`,
        description: item.description,
        badge: item.source,
        section:
          item.kind === 'system'
            ? t('conversation.slashLauncher.system', { defaultValue: 'System commands' })
            : item.kind === 'skill'
              ? t('conversation.slashLauncher.skills', { defaultValue: 'Skills' })
              : t('conversation.slashLauncher.agent', { defaultValue: 'Agent commands' }),
      })),
    [slashController.filteredItems, t]
  );

  const addMenuItems = useMemo<SlashCommandMenuItem[]>(
    () => [
      {
        key: 'files',
        label: t('common.fileAttach.filesAndFolders', { defaultValue: 'Files and folders' }),
        section: t('common.add'),
        icon: <Paperclip theme='outline' size='17' />,
      },
      ...(enableGoalMenu
        ? [
            {
              key: 'goal',
              label: t('conversation.goal.chip.label', { defaultValue: 'Goal' }),
              description:
                goalLauncherItem?.description ??
                t('conversation.goal.menu.description', {
                  defaultValue: 'Set a goal to keep pursuing',
                }),
              icon: <Aiming theme='outline' size='17' />,
            },
          ]
        : []),
    ],
    [enableGoalMenu, goalLauncherItem, t]
  );

  const handleAddMenuSelect = useCallback(
    (item: SlashCommandMenuItem) => {
      setIsAddMenuOpen(false);
      if (item.key === 'files') {
        onAddFiles?.();
      } else if (item.key === 'goal') {
        const goalItem =
          goalLauncherItem ??
          ({
            id: 'system:goal',
            kind: 'system' as const,
            name: 'goal',
            description: '',
          } satisfies SlashLauncherItem);
        slashController.onSelectItem(goalItem);
      }
    },
    [goalLauncherItem, onAddFiles, slashController]
  );

  const handleAddMenuKeyDown = useCallback(
    (event: React.KeyboardEvent) => {
      if (!isAddMenuOpen || addMenuItems.length === 0) {
        return false;
      }
      if (event.key === 'Escape') {
        event.preventDefault();
        setIsAddMenuOpen(false);
        return true;
      }
      if (event.key === 'ArrowDown') {
        event.preventDefault();
        setAddMenuActiveIndex((current) => (current + 1) % addMenuItems.length);
        return true;
      }
      if (event.key === 'ArrowUp') {
        event.preventDefault();
        setAddMenuActiveIndex((current) => (current - 1 + addMenuItems.length) % addMenuItems.length);
        return true;
      }
      if (event.key === 'Enter' && !event.shiftKey) {
        event.preventDefault();
        const activeItem = addMenuItems[addMenuActiveIndex];
        if (activeItem) {
          handleAddMenuSelect(activeItem);
        }
        return true;
      }
      return false;
    },
    [addMenuActiveIndex, addMenuItems, handleAddMenuSelect, isAddMenuOpen]
  );

  const isCommandMenuOpen = conversationExport.isOpen || slashController.isOpen;
  const isAtFileMenuOpen =
    Boolean(conversationContext?.workspace) &&
    Boolean(activeAtFileQuery) &&
    activeAtFileTokenKey !== dismissedAtFileToken &&
    !isCommandMenuOpen;
  const isComposerMenuOpen = isCommandMenuOpen || isAddMenuOpen || isAtFileMenuOpen;
  const visibleAtFileMenuItems = useMemo(
    () => filterWorkspaceMentionItems(workspaceMentionItems, deferredAtFileQuery),
    [deferredAtFileQuery, workspaceMentionItems]
  );
  const isOverlayOpen = isCommandMenuOpen || isAddMenuOpen || btwCommand.isOpen || isAtFileMenuOpen;

  const handleTokenInputChange = (value: string) => {
    if (historyNavigationIndex !== null) {
      historyDraftRef.current = null;
      setHistoryNavigationIndex(null);
    }
    if (conversationExport.isOpen && value) {
      conversationExport.closeExportFlow();
    }
    setInput(value);
  };

  const handleOverlayKeyDown = (event: React.KeyboardEvent) => {
    return handleAddMenuKeyDown(event) || conversationExport.handleKeyDown(event) || slashController.onKeyDown(event);
  };

  const renderExportFileNamePanel = () => {
    return (
      <div
        className='rounded-14px border border-solid overflow-hidden p-12px flex flex-col gap-10px'
        style={{
          borderColor: 'var(--color-border-2)',
          background: 'color-mix(in srgb, var(--color-bg-1) 88%, transparent)',
          backdropFilter: 'blur(14px) saturate(1.1)',
          WebkitBackdropFilter: 'blur(14px) saturate(1.1)',
        }}
      >
        <div className='text-13px font-semibold text-t-primary'>{t('messages.export.file_nameLabel')}</div>
        <Input
          autoFocus
          value={conversationExport.filename}
          onChange={conversationExport.setFilename}
          placeholder={t('messages.export.file_namePlaceholder')}
          disabled={conversationExport.loading}
          onKeyDown={(event) => {
            conversationExport.handleKeyDown(event);
          }}
        />
        <div className='text-12px text-t-secondary break-all'>
          {t('messages.export.pathLabel')}: {conversationExport.pathPreview}
        </div>
        <div className='flex items-center justify-end gap-8px'>
          <Button
            size='small'
            type='secondary'
            disabled={conversationExport.loading}
            onClick={() => {
              conversationExport.closeExportFlow();
            }}
          >
            {t('common.cancel')}
          </Button>
          <Button
            size='small'
            type='secondary'
            disabled={conversationExport.loading}
            onClick={() => {
              conversationExport.showMenu();
            }}
          >
            {t('common.back')}
          </Button>
          <Button
            size='small'
            type='primary'
            loading={conversationExport.loading}
            onClick={() => {
              void conversationExport.submitFilename();
            }}
          >
            {t('common.save')}
          </Button>
        </div>
      </div>
    );
  };

  useEffect(() => {
    if (!isAtFileMenuOpen || !conversationContext?.workspace || !atFileSessionKey) {
      fetchedAtFileSessionKeyRef.current = null;
      setWorkspaceMentionItems([]);
      setWorkspaceMentionLoading(false);
      return;
    }

    if (fetchedAtFileSessionKeyRef.current === atFileSessionKey) {
      return;
    }

    let cancelled = false;
    fetchedAtFileSessionKeyRef.current = atFileSessionKey;
    setWorkspaceMentionLoading(true);

    void ipcBridge.fs.listWorkspaceFiles
      .invoke({ root: conversationContext.workspace })
      .then((result) => {
        if (cancelled) {
          return;
        }
        const files = result.map((item) => ({
          path: item.fullPath,
          name: item.name,
          isFile: true,
          relativePath: item.relativePath || undefined,
        }));
        setWorkspaceMentionItems(files);
      })
      .catch((error) => {
        if (!cancelled) {
          fetchedAtFileSessionKeyRef.current = null;
          console.warn('[SendBox] Failed to load workspace file mentions:', error);
          setWorkspaceMentionItems([]);
        }
      })
      .finally(() => {
        if (!cancelled) {
          setWorkspaceMentionLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [atFileSessionKey, conversationContext?.workspace, isAtFileMenuOpen]);

  useEffect(() => {
    if (!activeAtFileTokenKey) {
      setAtFileMenuActiveIndex(0);
      return;
    }
    setAtFileMenuActiveIndex(0);
  }, [activeAtFileTokenKey]);

  useEffect(() => {
    if (!visibleAtFileMenuItems.length) {
      setAtFileMenuActiveIndex(0);
      return;
    }
    setAtFileMenuActiveIndex((previous) => Math.min(previous, visibleAtFileMenuItems.length - 1));
  }, [visibleAtFileMenuItems]);

  useEffect(() => {
    if (!selectedWorkspaceItems || !onSelectedWorkspaceItemsChange) {
      return;
    }

    const mentionQueries = new Set(allAtFileQueries.map((item) => item.query));
    selectedWorkspaceItems.forEach((item) => rememberSelectedItem(selectedItemByPathRef.current, item));

    const nextMentionOwnedPaths = new Set<string>();
    for (const path of mentionOwnedPathsRef.current) {
      const item = selectedItemByPathRef.current.get(path);
      if (!item) {
        continue;
      }

      if (getSelectedItemMatchKeys(item).some((key) => mentionQueries.has(key))) {
        nextMentionOwnedPaths.add(path);
      }
    }

    for (const item of selectedWorkspaceItems) {
      const path = getSelectedItemPath(item);
      if (!path) {
        continue;
      }

      if (getSelectedItemMatchKeys(item).some((key) => mentionQueries.has(key))) {
        nextMentionOwnedPaths.add(path);
      }
    }

    const incomingPaths = new Set<string>();
    for (const item of selectedWorkspaceItems) {
      const path = getSelectedItemPath(item);
      if (path) {
        incomingPaths.add(path);
      }
    }

    const nextExternalOwnedPaths = new Set(
      Array.from(externalOwnedPathsRef.current).filter((path) => incomingPaths.has(path))
    );
    for (const path of incomingPaths) {
      if (!nextMentionOwnedPaths.has(path) && !everMentionOwnedPathsRef.current.has(path)) {
        nextExternalOwnedPaths.add(path);
      }
    }

    mentionOwnedPathsRef.current = nextMentionOwnedPaths;
    nextMentionOwnedPaths.forEach((path) => {
      everMentionOwnedPathsRef.current.add(path);
    });
    externalOwnedPathsRef.current = nextExternalOwnedPaths;

    const nextItems = buildOwnedSelectionItems(
      selectedWorkspaceItems,
      mentionOwnedPathsRef.current,
      externalOwnedPathsRef.current,
      selectedItemByPathRef.current
    );

    if (!areSelectionItemsEquivalent(selectedWorkspaceItems, nextItems)) {
      onSelectedWorkspaceItemsChange(nextItems);
    }
  }, [allAtFileQueries, onSelectedWorkspaceItemsChange, selectedWorkspaceItems]);

  const handleExternalSelectionAppend = useCallback((items: FileSelectionItem[]) => {
    for (const item of items) {
      const path = getSelectedItemPath(item);
      if (!path) {
        continue;
      }

      if (suppressedExternalAppendPathsRef.current.has(path)) {
        suppressedExternalAppendPathsRef.current.delete(path);
        continue;
      }

      rememberSelectedItem(selectedItemByPathRef.current, item);
      externalOwnedPathsRef.current.add(path);
    }
  }, []);

  useAddEventListener(
    'nomi.selected.file.append',
    (items: FileSelectionItem[]) => {
      if (conversationContext?.type === 'nomi') {
        handleExternalSelectionAppend(items);
      }
    },
    [conversationContext?.type, handleExternalSelectionAppend]
  );
  useAddEventListener(
    'acp.selected.file.append',
    (items: FileSelectionItem[]) => {
      if (conversationContext?.type === 'acp') {
        handleExternalSelectionAppend(items);
      }
    },
    [conversationContext?.type, handleExternalSelectionAppend]
  );
  useAddEventListener(
    'remote.selected.file.append',
    (items: FileSelectionItem[]) => {
      if (conversationContext?.type === 'remote') {
        handleExternalSelectionAppend(items);
      }
    },
    [conversationContext?.type, handleExternalSelectionAppend]
  );
  useAddEventListener(
    'openclaw-gateway.selected.file.append',
    (items: FileSelectionItem[]) => {
      if (conversationContext?.type === 'openclaw-gateway') {
        handleExternalSelectionAppend(items);
      }
    },
    [conversationContext?.type, handleExternalSelectionAppend]
  );
  useAddEventListener(
    'nanobot.selected.file.append',
    (items: FileSelectionItem[]) => {
      if (conversationContext?.type === 'nanobot') {
        handleExternalSelectionAppend(items);
      }
    },
    [conversationContext?.type, handleExternalSelectionAppend]
  );

  const emitSelectedFileAppend = useCallback(
    (item: FileOrFolderItem) => {
      switch (conversationContext?.type) {
        case 'nomi':
          emitter.emit('nomi.selected.file.append', [item]);
          break;
        case 'acp':
          emitter.emit('acp.selected.file.append', [item]);
          break;
        case 'remote':
          emitter.emit('remote.selected.file.append', [item]);
          break;
        case 'openclaw-gateway':
          emitter.emit('openclaw-gateway.selected.file.append', [item]);
          break;
        case 'nanobot':
          emitter.emit('nanobot.selected.file.append', [item]);
          break;
        default:
          break;
      }
    },
    [conversationContext?.type]
  );

  const insertSelectedAtFile = useCallback(
    (item: FileOrFolderItem) => {
      if (!activeAtFileQuery) {
        return;
      }

      const nextInsertion = buildAtFileInsertion(item);
      if (!nextInsertion) {
        return;
      }
      const nextCaret = activeAtFileQuery.start + nextInsertion.length;
      const insertedTokenKey = `${activeAtFileQuery.start}:${nextInsertion.slice(1)}`;
      const path = getSelectedItemPath(item);

      setDismissedAtFileToken(insertedTokenKey);
      tokenInputRef.current?.replaceTextRange(activeAtFileQuery, nextInsertion);
      if (path) {
        rememberSelectedItem(selectedItemByPathRef.current, item);
        mentionOwnedPathsRef.current.add(path);
        everMentionOwnedPathsRef.current.add(path);
        suppressedExternalAppendPathsRef.current.add(path);
      }
      if (selectedWorkspaceItems && onSelectedWorkspaceItemsChange) {
        const mergedItems = mergeFileSelectionItems(selectedWorkspaceItems, [item]);
        const nextItems = buildOwnedSelectionItems(
          mergedItems,
          mentionOwnedPathsRef.current,
          externalOwnedPathsRef.current,
          selectedItemByPathRef.current
        );
        onSelectedWorkspaceItemsChange(nextItems);
      }
      emitSelectedFileAppend(item);

      requestAnimationFrame(() => {
        tokenInputRef.current?.focusAtTextOffset(nextCaret);
        setCaretPosition(nextCaret);
      });
    },
    [
      activeAtFileQuery,
      emitSelectedFileAppend,
      onSelectedWorkspaceItemsChange,
      selectedWorkspaceItems,
    ]
  );

  // 使用共享的输入法合成处理
  const { compositionHandlers, createKeyDownHandler } = useCompositionInput();
  const [sendKeyPref] = useConfig('chat.sendKey');
  const sendKey = sendKeyPref ?? 'enter';

  // 使用共享的PasteService集成
  const { onPaste, onFocus: handlePasteFocus } = usePasteService({
    supportedExts,
    onFilesAdded,
    conversation_id:
      conversationContext?.conversation_id != null ? conversationContext.conversation_id : undefined,
  });
  const markMobileFocusIntent = useCallback(() => {
    if (!isMobile) return;
    mobileUserFocusIntentUntilRef.current = Date.now() + 1500;
  }, [isMobile]);

  const handleInputFocus = useCallback(() => {
    if (isMobile && Date.now() > mobileUserFocusIntentUntilRef.current) {
      blurActiveElement();
      return;
    }
    if (isMobile && shouldBlockMobileInputFocus()) {
      blurActiveElement();
      return;
    }
    mobileUserFocusIntentUntilRef.current = 0;
    handlePasteFocus();
    setIsInputFocused(true);

    // Pre-warm worker bootstrap shortly after focus (short debounce). Focusing
    // the input is a strong "about to send" intent, so warm eagerly to hide the
    // session build (MCP connect / skill load / prompt build) behind the user's
    // typing. The debounce only guards against warming while focus flickers
    // during rapid switching; kept short so the build starts well before submit.
    // (Idempotent: single-flight in warmupConversation + warmedConversationRef +
    // the backend's per-conversation OnceCell, so a redundant call is a no-op.)
    const cid = conversationContext?.conversation_id;
    if (cid && warmedConversationRef.current !== cid) {
      if (warmupTimerRef.current) clearTimeout(warmupTimerRef.current);
      warmupTimerRef.current = setTimeout(() => {
        warmedConversationRef.current = cid;
        warmupConversation(cid).catch(() => {});
      }, 300);
    }
  }, [handlePasteFocus, isMobile, conversationContext?.conversation_id]);
  const handleInputBlur = useCallback(() => {
    if (warmupTimerRef.current) {
      clearTimeout(warmupTimerRef.current);
      warmupTimerRef.current = null;
    }
    setIsInputFocused(false);
  }, []);

  useEffect(() => {
    historyDraftRef.current = null;
    setHistoryNavigationIndex(null);
    mentionOwnedPathsRef.current = new Set();
    everMentionOwnedPathsRef.current = new Set();
    externalOwnedPathsRef.current = new Set();
    selectedItemByPathRef.current = new Map();
    suppressedExternalAppendPathsRef.current = new Set();
  }, [conversationContext?.conversation_id]);

  const applyHistoryInput = useCallback(
    (value: string) => {
      setInputRef.current(value);
      requestAnimationFrame(() => {
        tokenInputRef.current?.focusAtTextOffset(value.length);
      });
    },
    [setInputRef]
  );

  const exitHistoryNavigation = useCallback(
    (restoreDraft: boolean) => {
      const draft = historyDraftRef.current;
      historyDraftRef.current = null;
      setHistoryNavigationIndex(null);
      if (restoreDraft && draft !== null) {
        applyHistoryInput(draft);
      }
    },
    [applyHistoryInput]
  );

  const handleHistoryKeyDown = useCallback(
    (event: React.KeyboardEvent) => {
      if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) {
        return false;
      }

      if (!(event.currentTarget instanceof HTMLElement)) {
        return false;
      }

      if (event.key === 'Escape' && historyNavigationIndex !== null) {
        event.preventDefault();
        exitHistoryNavigation(true);
        return true;
      }

      if (!inputHistory.length) {
        return false;
      }

      if (event.key === 'ArrowUp') {
        if (historyNavigationIndex === null && input.slice(0, tokenInputState.textSelection.start).includes('\n')) {
          return false;
        }

        const nextIndex =
          historyNavigationIndex === null ? 0 : Math.min(historyNavigationIndex + 1, inputHistory.length - 1);
        const nextValue = inputHistory[nextIndex];
        if (nextValue === undefined) {
          return false;
        }

        if (historyNavigationIndex === null) {
          historyDraftRef.current = latestInputRef.current;
        }

        event.preventDefault();
        setHistoryNavigationIndex(nextIndex);
        applyHistoryInput(nextValue);
        return true;
      }

      if (event.key === 'ArrowDown' && historyNavigationIndex !== null) {
        event.preventDefault();
        if (historyNavigationIndex === 0) {
          exitHistoryNavigation(true);
          return true;
        }

        const nextIndex = historyNavigationIndex - 1;
        const nextValue = inputHistory[nextIndex];
        if (nextValue === undefined) {
          exitHistoryNavigation(true);
          return true;
        }

        setHistoryNavigationIndex(nextIndex);
        applyHistoryInput(nextValue);
        return true;
      }

      return false;
    },
    [applyHistoryInput, exitHistoryNavigation, historyNavigationIndex, input, inputHistory, latestInputRef, tokenInputState.textSelection.start]
  );

  const handleAtFileMenuKeyDown = useCallback(
    (event: React.KeyboardEvent) => {
      if (!isAtFileMenuOpen || !activeAtFileTokenKey) {
        return false;
      }

      if (event.key === 'Escape') {
        event.preventDefault();
        setDismissedAtFileToken(activeAtFileTokenKey);
        return true;
      }

      if (!visibleAtFileMenuItems.length) {
        return false;
      }

      if (event.key === 'ArrowDown') {
        event.preventDefault();
        setAtFileMenuActiveIndex((previous) => (previous + 1) % visibleAtFileMenuItems.length);
        return true;
      }

      if (event.key === 'ArrowUp') {
        event.preventDefault();
        setAtFileMenuActiveIndex((previous) => (previous === 0 ? visibleAtFileMenuItems.length - 1 : previous - 1));
        return true;
      }

      if (event.key === 'Enter') {
        const selectedItem = visibleAtFileMenuItems[atFileMenuActiveIndex];
        if (!selectedItem) {
          return false;
        }
        event.preventDefault();
        insertSelectedAtFile(selectedItem);
        return true;
      }

      return false;
    },
    [activeAtFileTokenKey, atFileMenuActiveIndex, insertSelectedAtFile, isAtFileMenuOpen, visibleAtFileMenuItems]
  );

  // Builds the final message from the current draft and CLEARS the input.
  // Returns null when there's nothing to send. Mirrors the compose half of
  // sendMessageHandler so steer can reuse it.
  const composeAndClear = (allowEmpty = false): string | null => {
    if (!input.trim() && domSnippets.length === 0 && !allowEmpty) return null;

    historyDraftRef.current = null;
    setHistoryNavigationIndex(null);

    // 构建消息内容 / Build message content
    let finalMessage = input;

    // Prepend reply quote as blockquote
    if (replyQuote) {
      const quotedLines = replyQuote.content
        .split('\n')
        .map((line) => `> ${line}`)
        .join('\n');
      finalMessage = `${quotedLines}\n\n${finalMessage}`;
    }

    // 如果有 DOM 片段，附加完整 HTML / If has DOM snippets, append full HTML
    if (domSnippets.length > 0) {
      const snippetsHtml = domSnippets
        .map((s) => `\n\n---\nDOM Snippet (${s.tag}):\n\`\`\`html\n${s.html}\n\`\`\``)
        .join('');
      finalMessage = input + snippetsHtml;
    }

    // Clear the complete token document so a failed send can restore the exact
    // Skill positions instead of rebuilding the old prefix layout.
    lastSubmittedDraftRef.current = tokenInputRef.current?.getDraft() ?? null;
    if (tokenInputRef.current) {
      tokenInputRef.current.clear();
    } else {
      setInput('');
    }
    clearDomSnippets();
    setReplyQuote(null);

    return finalMessage;
  };

  const cancelEdit = () => {
    // A submit is already in flight — its promise chain owns the edit state
    // until the backend accepts/rejects; canceling mid-flight would strand
    // isLoading. Ignore the cancel click while a resubmit is pending.
    if (isLoading) return;
    setEditingMsgId(null);
    const prev = editPrevDraftRef.current ?? '';
    editPrevDraftRef.current = null;
    setInput(prev);
    // C3: 取消编辑 → 清除编辑中徽章（仅 owner 匹配才生效）。
    // C3: cancel edit → clear the editing badge (owner-guarded).
    const cid = conversationIdRef.current;
    if (cid) clearEditingMessage(cid, editingOwnerId());
  };

  const submitGoalObjective = (objective: string) => {
    const busy = !allowSendWhileLoading && (isLoading || loading);
    void goalCommand.run({ action: 'set', objective }, { setToast: busy ? 'deferred' : 'started' }).then((ok) => {
      if (!ok) {
        return;
      }
      onGoalModeChange?.(false);
      if (busy) {
        return;
      }
      setIsLoading(true);
      onSend(objective)
        .catch(() => {
          setInput(objective);
        })
        .finally(() => {
          setIsLoading(false);
        });
    });
  };

  const sendMessageHandler = () => {
    if (isUploading || isStopping) return;
    // 编辑模式：提交走截断重跑而非普通发送。
    if (editingMsgId && onEditResubmit) {
      if (isLoading || !input.trim()) return;
      const finalMessage = input;
      const targetId = editingMsgId;
      const targetCreatedAt = editingCreatedAtRef.current;
      // Stamp this resubmit with an operation token and snapshot the input
      // revision. Edit state + input are cleared ONLY after backend acceptance
      // (the .then below); a stale operation or a user who typed mid-flight must
      // not have its input/edit/loading state touched.
      const operationId = uuid();
      activeEditOperationRef.current = operationId;
      const submittedInputRevision = inputRevisionRef.current;
      const isCurrentOperation = () => activeEditOperationRef.current === operationId;
      const ownerId = editingOwnerId();
      const cid = conversationIdRef.current;
      setIsLoading(true);
      // C3: 重发在飞 → 徽章转「重发中」（仅 owner 匹配生效）。
      // C3: resubmit in flight → badge flips to "resubmitting" (owner-guarded).
      if (cid) updateEditingMessage(cid, ownerId, { pending: true, operationId });
      onEditResubmit(targetId, targetCreatedAt, finalMessage)
        .then(() => {
          // Backend accepted: exit edit mode. Only clear the composer when the
          // user never modified it after submit (revision unchanged); otherwise
          // preserve whatever they are now typing.
          const outcome = resolveEditResubmitOutcome({
            isCurrentOperation: isCurrentOperation(),
            revisionUnchanged: inputRevisionRef.current === submittedInputRevision,
            status: 'success',
          });
          if (outcome.stale) return;
          if (outcome.clearInput) setInput('');
          if (outcome.exitEditMode) {
            setEditingMsgId(null);
            editPrevDraftRef.current = null;
          }
          // C3: 接受后消息已被替换，清除编辑中徽章（owner-guarded）。
          // C3: after acceptance the message is replaced — clear the badge.
          if (cid) clearEditingMessage(cid, ownerId);
        })
        .catch(() => {
          // Backend rejected: stay in edit mode so the user can adjust and retry.
          // Restore the submitted text only if the user didn't change it mid-flight.
          const outcome = resolveEditResubmitOutcome({
            isCurrentOperation: isCurrentOperation(),
            revisionUnchanged: inputRevisionRef.current === submittedInputRevision,
            status: 'failure',
          });
          if (outcome.stale) return;
          if (outcome.restoreSubmittedInput) setInput(finalMessage);
          // C3: 失败后仍处编辑态，徽章回落为「编辑中」。
          // C3: still editing after a failure — badge drops back to "editing".
          if (cid) updateEditingMessage(cid, ownerId, { pending: false });
        })
        .finally(() => {
          // Lowering isLoading depends only on the operation token, not the
          // outcome status — a stale operation's promise must not toggle it.
          if (isCurrentOperation()) {
            setIsLoading(false);
          }
        });
      return;
    }
    // Cancel any pending warmup: once the user actually submits, the
    // forthcoming /messages request will build the agent on its own.
    // Without this, a focus-triggered warmup timer still fires ~1s later
    // and races the real send over the same conversation.
    if (warmupTimerRef.current) {
      clearTimeout(warmupTimerRef.current);
      warmupTimerRef.current = null;
    }
    const activeCid = conversationContext?.conversation_id;
    if (activeCid) {
      warmedConversationRef.current = activeCid;
    }
    if (enableBtw && btwQuestion !== null) {
      const normalizedQuestion = btwQuestion.trim();
      if (!normalizedQuestion) {
        message.warning(t('conversation.sideQuestion.emptyQuestion'));
        return;
      }
      if (btwCommand.isLoading) {
        message.warning(t('conversation.sideQuestion.alreadyRunning'));
        return;
      }
      if (hasPendingAttachments || domSnippets.length > 0) {
        message.warning(t('conversation.sideQuestion.attachmentsNotAllowed'));
        return;
      }
      historyDraftRef.current = null;
      setHistoryNavigationIndex(null);
      setInput('');
      void btwCommand.ask(normalizedQuestion);
      return;
    }

    // 拦截 /goal 与 /subgoal 家族命令（在忙碌门控之前：暂停/恢复/清除在 turn 运行中也应可用）
    if (goalCommand.enabled) {
      const goalInvocation = parseGoalSlashCommand(input);
      if (goalInvocation) {
        if (goalInvocation.action === 'start') {
          historyDraftRef.current = null;
          setHistoryNavigationIndex(null);
          setInput('');
          onGoalModeChange?.(true);
          requestAnimationFrame(() => {
            tokenInputRef.current?.focusAtTextOffset(0);
          });
          return;
        }
        historyDraftRef.current = null;
        setHistoryNavigationIndex(null);
        setInput('');
        // `/goal <text>`（set）需立即启动首轮：设定成功后把 objective 原文
        // 作为普通用户消息走既有发送管线。set 分支遵守忙碌门控——turn
        // 进行中只设定目标（当前回合结束后由 judge 接管续作），不强行插消息；
        // set 失败（API 报错）则不发送。
        if (goalInvocation.action === 'set') {
          const objective = goalInvocation.objective;
          submitGoalObjective(objective);
          return;
        }
        void goalCommand.run(goalInvocation);
        return;
      }
    }

    if (goalModeArmed && input.trim()) {
      historyDraftRef.current = null;
      setHistoryNavigationIndex(null);
      setInput('');
      submitGoalObjective(input);
      return;
    }

    if (!allowSendWhileLoading && (isLoading || loading)) {
      console.info('[sendbox]', {
        event: 'blocked-while-loading',
        allowSendWhileLoading,
        isLoading,
        loading,
      });
      message.warning(t('messages.conversationInProgress'));
      return;
    }
    const hasSkillLoadPlan = Boolean(onSendWithSkills && skillChips.length > 0);
    if (!input.trim() && domSnippets.length === 0 && !hasSkillLoadPlan) {
      return;
    }
    console.info('[sendbox]', {
      event: 'submit',
      allowSendWhileLoading,
      isLoading,
      loading,
      inputLength: input.length,
      domSnippetCount: domSnippets.length,
    });
    setIsLoading(true);
    const submittedSkills = skillChips;
    const finalMessage = composeAndClear(hasSkillLoadPlan);
    if (finalMessage == null) return;

    const send = hasSkillLoadPlan && onSendWithSkills
      ? onSendWithSkills(finalMessage, submittedSkills.map((skill) => skill.skillId))
      : onSend(finalMessage);
    send
      .then(() => {
        if (hasSkillLoadPlan) {
          onSkillChipsChange?.([]);
        }
      })
      .catch(() => {
        const submittedDraft = lastSubmittedDraftRef.current;
        if (submittedDraft) {
          tokenInputRef.current?.restoreDraft(submittedDraft);
        } else {
          setInput(finalMessage);
          if (hasSkillLoadPlan) {
            onSkillChipsChange?.(submittedSkills);
          }
        }
      })
      .finally(() => {
        setIsLoading(false);
      });
  };

  const steerMessageHandler = () => {
    if (!onSteer || isUploading || isStopping) return;
    const finalMessage = composeAndClear();
    if (finalMessage == null) return;
    setIsLoading(true);
    onSteer(finalMessage)
      .catch(() => {
        const submittedDraft = lastSubmittedDraftRef.current;
        if (submittedDraft) {
          tokenInputRef.current?.restoreDraft(submittedDraft);
        } else {
          setInput(finalMessage);
        }
      })
      .finally(() => {
        setIsLoading(false);
      });
  };

  const stopHandler = async () => {
    if (!onStop || isStoppingRef.current) return;
    isStoppingRef.current = true;
    setIsStopping(true);
    try {
      await onStop();
    } finally {
      isStoppingRef.current = false;
      setIsStopping(false);
      setIsLoading(false);
    }
  };

  const handleSpeechTranscript = useCallback(
    (transcript: string) => {
      const current_value = latestInputRef.current;
      const nextValue = appendSpeechTranscript(current_value, transcript);
      const insertion = nextValue.slice(current_value.length);
      if (tokenInputRef.current) {
        tokenInputRef.current.focusAtTextOffset(current_value.length);
        tokenInputRef.current.insertTextAtSelection(insertion);
      } else {
        setInputRef.current(nextValue);
      }
    },
    [latestInputRef, setInputRef]
  );
  const speechLocale = i18n?.language || 'en-US';

  const hasInlineSkillChips = skillChips.length > 0;
  const hasDraftToSend = input.trim().length > 0 || domSnippets.length > 0 || hasInlineSkillChips;

  const isProcessing = isLoading || loading;
  const showStopOnly =
    isProcessing &&
    (!allowSendWhileLoading || compactActions || !hasDraftToSend || disabled || isUploading);

  const composerSubmitCluster = (
    <ComposerSubmitCluster
      hasDraft={hasDraftToSend}
      loading={isProcessing}
      disabled={disabled}
      isUploading={isUploading}
      speechLocale={speechLocale}
      onSend={sendMessageHandler}
      onSpeechTranscript={handleSpeechTranscript}
      showStop={isProcessing}
      onStop={stopHandler}
      showSteer={Boolean(onSteer) && allowSendWhileLoading && isProcessing && !showStopOnly}
      steerAvailable={steerAvailable}
      onSteer={steerMessageHandler}
      speechHidden={isMobileCompact}
      sendTestId='sendbox-send-btn'
    />
  );

  const mobilePlusButton = isMobileCompact ? (
    <Button
      shape='circle'
      type='secondary'
      size='small'
      className='sendbox-composer-plus-btn sendbox-mobile-plus-btn'
      icon={<Plus theme='outline' size='16' strokeWidth={3} fill='currentColor' />}
      onClick={onMobilePlusClick}
      data-testid='sendbox-mobile-plus-btn'
      aria-label={t('common.more', { defaultValue: 'More' })}
    />
  ) : null;

  // On mobile compact mode, the parent supplies the action sheet — collapse
  // tools/rightTools into the `+` launcher and skip the inline speech button.
  const renderedTools = isMobileCompact ? mobilePlusButton : tools;
  const renderedRightTools = isMobileCompact ? null : rightTools;

  return (
    <ComposerSurface
      outerRef={dropzoneRef}
      panelRef={containerRef}
      dragHandlers={dragHandlers}
      dragHandlersTarget='panel'
      isOverlayOpen={isOverlayOpen}
      overflowTarget='panel'
      className={className}
      panelClassName={`sendbox-panel p-16px border-3 b bg-dialog-fill-0 b-solid rd-20px ${
        isFileDragging ? 'b-dashed sendbox-panel--dragging' : ''
      }`}
      before={
        pinnedPlan ? (
          <div
            className='absolute left-1/2 bottom-[calc(100%+8px)] -translate-x-1/2 z-30'
            data-testid='sendbox-plan-anchor'
          >
            <PinnedPlan plan={pinnedPlan} active={Boolean(loading || isLoading)} />
          </div>
        ) : null
      }
      panelStyle={{
        ...(isFileDragging
          ? {
              backgroundColor: 'var(--color-primary-light-1)',
              borderColor: 'rgb(var(--primary-3))',
              borderWidth: '1px',
            }
          : {
              borderWidth: '1px',
              borderColor: isComposerMenuOpen
                ? COMPOSER_MENU_BORDER_COLOR
                : isInputActive
                  ? activeBorderColor
                  : inactiveBorderColor,
              boxShadow: 'none',
            }),
      }}
    >
      <BtwOverlay
        answer={btwCommand.answer}
        anchorEl={containerRef.current}
        isLoading={btwCommand.isLoading}
        isOpen={btwCommand.isOpen}
        onDismiss={btwCommand.dismiss}
        parentTaskRunning={Boolean(loading || isLoading)}
        question={btwCommand.question}
      />
      {isAtFileMenuOpen && (
        <div className='absolute left-0 right-0 bottom-[calc(100%+10px)] z-70'>
          <AtFileMenu
            activeIndex={atFileMenuActiveIndex}
            emptyText={
              deferredAtFileQuery
                ? t('conversation.workspace.search.empty', { defaultValue: 'No files found' })
                : t('messages.atFile.hint', { defaultValue: 'Type to search for files' })
            }
            items={visibleAtFileMenuItems}
            label={t('messages.atFile.menuLabel', { defaultValue: 'File mentions' })}
            loading={workspaceMentionLoading}
            loadingText={t('messages.atFile.loading', { defaultValue: 'Loading...' })}
            onHoverItem={setAtFileMenuActiveIndex}
            onSelectItem={insertSelectedAtFile}
          />
        </div>
      )}
      {(isAddMenuOpen || isCommandMenuOpen) && (
        <div className='absolute left-0 right-0 bottom-[calc(100%+10px)] z-70'>
          {isAddMenuOpen ? (
            <SlashCommandMenu
              title={t('common.add')}
              compact
              items={addMenuItems}
              activeIndex={addMenuActiveIndex}
              onHoverItem={setAddMenuActiveIndex}
              onSelectItem={handleAddMenuSelect}
              emptyText={t('messages.slash.empty', { defaultValue: 'No commands found' })}
            />
          ) : conversationExport.step === 'menu' ? (
            <SlashCommandMenu
              title={t('messages.export.menuTitle')}
              hint={t('messages.export.menuHint')}
              items={conversationExport.menuItems}
              activeIndex={conversationExport.activeIndex}
              loading={conversationExport.loading}
              onHoverItem={conversationExport.setActiveIndex}
              onSelectItem={(item) => {
                conversationExport.onSelectMenuItem(item.key);
              }}
              emptyText={t('messages.slash.empty', { defaultValue: 'No commands found' })}
            />
          ) : conversationExport.step === 'filename' ? (
            renderExportFileNamePanel()
          ) : (
            <SlashCommandMenu
              title={t('messages.slash.title', { defaultValue: 'Commands' })}
              hint={t('messages.slash.hint', { defaultValue: 'Type / to open command menu' })}
              compact
              items={slashMenuItems}
              activeIndex={slashController.activeIndex}
              loading={false}
              onHoverItem={slashController.setActiveIndex}
              onSelectItem={(item) => {
                const targetIndex = slashController.filteredItems.findIndex((launcherItem) => launcherItem.id === item.key);
                if (targetIndex >= 0) {
                  slashController.onSelectByIndex(targetIndex);
                }
              }}
              emptyText={t('messages.slash.empty', { defaultValue: 'No commands found' })}
            />
          )}
        </div>
      )}
        {hasInternalStatusRow && (
          <div
            className='sendbox-internal-status-row mb-8px flex w-full flex-wrap items-start gap-8px'
            data-testid='sendbox-internal-status-row'
          >
            {topRightTools && (
              <div className='ml-auto flex h-28px flex-shrink-0 items-center' data-testid='sendbox-internal-context-tools'>
                {topRightTools}
              </div>
            )}
          </div>
        )}
        <div style={{ width: '100%' }}>
          {prefix}
          {context}
          {/* 编辑消息提示条 / Editing message banner */}
          {editingMsgId && (
            <div className='flex items-center gap-10px mb-8px px-12px py-8px rd-10px bg-fill-1 b-1 b-solid b-border-2'>
              <span className='text-13px text-t-primary'>{t('conversation.editMessage.banner')}</span>
              <div
                className='ml-auto flex-shrink-0 p-2px rd-full cursor-pointer hover:bg-fill-3 transition-colors'
                onClick={cancelEdit}
                style={{ lineHeight: 0 }}
              >
                <CloseSmall theme='outline' size='14' />
              </div>
            </div>
          )}
          {/* Reply quote preview */}
          {replyQuote && (
            <div className='flex items-start gap-10px mb-8px px-12px py-10px rd-10px bg-fill-1 b-1 b-solid b-border-2'>
              <div className='flex-shrink-0 mt-2px' style={{ lineHeight: 0 }}>
                <Quote theme='filled' size='16' fill='rgb(var(--primary-6))' />
              </div>
              <div className='flex-1 min-w-0 text-13px text-t-primary line-clamp-3 lh-20px whitespace-pre-wrap break-all'>
                {replyQuote.content}
              </div>
              <div
                className='flex-shrink-0 mt-2px p-2px rd-full cursor-pointer hover:bg-fill-3 transition-colors'
                onClick={() => setReplyQuote(null)}
                style={{ lineHeight: 0 }}
              >
                <CloseSmall theme='outline' size='14' />
              </div>
            </div>
          )}
          {/* DOM 片段标签 / DOM snippet tags */}
          {domSnippets.length > 0 && (
            <div className='flex flex-wrap gap-6px mb-8px'>
              {domSnippets.map((snippet) => (
                <Tag
                  key={snippet.id}
                  closable
                  closeIcon={<CloseSmall theme='outline' size='12' />}
                  onClose={() => removeDomSnippet(snippet.id)}
                  className='text-12px bg-fill-2 b-1 b-solid b-border-2 rd-4px'
                >
                  {snippet.tag}
                </Tag>
              ))}
            </div>
          )}
          {unmatchedSelectedWorkspaceItems.length > 0 && onSelectedWorkspaceItemsChange && (
            <div className='flex flex-wrap gap-6px mb-8px'>
              {unmatchedSelectedWorkspaceItems.map((item) => (
                <Tag
                  key={typeof item === 'string' ? item : item.path}
                  closable
                  closeIcon={<CloseSmall theme='outline' size='12' />}
                  onClose={() => {
                    const path = getSelectedItemPath(item);
                    if (!path) {
                      return;
                    }
                    externalOwnedPathsRef.current.delete(path);
                    const nextItems = buildOwnedSelectionItems(
                      selectedWorkspaceItems ?? [],
                      mentionOwnedPathsRef.current,
                      externalOwnedPathsRef.current,
                      selectedItemByPathRef.current
                    );
                    onSelectedWorkspaceItemsChange(nextItems);
                  }}
                  className='text-12px bg-fill-2 b-1 b-solid b-border-2 rd-4px'
                >
                  {getSelectedItemDisplayLabel(item)}
                </Tag>
              ))}
            </div>
          )}
        </div>
        <UploadProgressBar source='sendbox' />
        <div
          className={
            isSingleLine
              ? 'flex items-center gap-2 w-full min-w-0 overflow-hidden'
              : 'w-full overflow-hidden'
          }
        >
          {isSingleLine && (
            <div
              className={
                isMobileCompact
                  ? 'flex-shrink-0 sendbox-tools sendbox-tools-mobile-compact'
                  : isMobile
                    ? 'sendbox-tools sendbox-tools-scroll-mobile'
                    : 'flex-shrink-0 sendbox-tools'
              }
            >
              {renderedTools}
            </div>
          )}
          <div className={isSingleLine ? 'flex-1 min-w-0' : 'w-full'}>
            <ComposerSkillTokenInput
              ref={tokenInputRef}
              autoFocus={!isMobile}
              disabled={disabled}
              value={input}
              skills={skillChips}
              onChange={handleTokenInputChange}
              onSkillsChange={onSkillChipsChange}
              onDraftStateChange={(state) => {
                setTokenInputState(state);
                setCaretPosition(state.textSelection.end);
              }}
              placeholder={
                isMobileCompact
                  ? (placeholder ??
                    (bottomHint as string | undefined) ??
                    t('conversation.sendbox.hint', { defaultValue: 'Type / for commands, @ to reference files' }))
                  : placeholder
                    ? `${placeholder}  ${bottomHint ?? t('conversation.sendbox.hint', { defaultValue: 'Type / for commands, @ to reference files' })}`
                    : ((bottomHint as string | undefined) ??
                      t('conversation.sendbox.hint', { defaultValue: 'Type / for commands, @ to reference files' }))
              }
              className={`pl-0 pr-0 focus:shadow-none m-0 !bg-transparent lh-[20px] text-14px ${isMobile ? 'sendbox-input--mobile' : ''}`}
              dataTestId='sendbox-input'
              singleLine={isSingleLine}
              style={{
                minHeight: isSingleLine ? (isMobile ? '22px' : '20px') : '40px',
                maxHeight: isSingleLine ? (isMobile ? '22px' : '20px') : '200px',
                overflowY: isSingleLine ? 'hidden' : 'auto',
                marginBottom: isSingleLine ? 0 : '8px',
              }}
              onPaste={onPaste}
              onMouseDown={markMobileFocusIntent}
              onFocus={handleInputFocus}
              onBlur={handleInputBlur}
              {...compositionHandlers}
              onKeyDown={createKeyDownHandler(
                sendMessageHandler,
                (event) => {
                  if (handleAtFileMenuKeyDown(event) || handleOverlayKeyDown(event) || handleHistoryKeyDown(event)) {
                    return true;
                  }
                  // Mod(Ctrl/Cmd)+Enter steers the draft into the running turn
                  // instead of enqueuing it. Only in 'enter' mode — in 'mod-enter'
                  // mode Mod+Enter IS the submit gesture. Opt-in via onSteer + steerAvailable.
                  if (
                    sendKey === 'enter' &&
                    onSteer &&
                    steerAvailable &&
                    event.key === 'Enter' &&
                    !event.shiftKey &&
                    (event.metaKey || event.ctrlKey)
                  ) {
                    event.preventDefault();
                    steerMessageHandler();
                    return true;
                  }
                  return false;
                },
                sendKey
              )}
            />
          </div>
          {isSingleLine && (
            <div className='flex items-center gap-2'>
              {sendButtonPrefix}
              {composerSubmitCluster}
            </div>
          )}
        </div>
        {!isSingleLine && (
          <div className='flex items-center justify-between gap-2 w-full'>
            <div
              className={
                isMobileCompact
                  ? 'flex-shrink-0 sendbox-tools sendbox-tools-mobile-compact'
                  : isMobile
                    ? 'sendbox-tools sendbox-tools-scroll-mobile'
                    : 'sendbox-tools'
              }
            >
              {renderedTools}
            </div>
            <div
              className={`sendbox-actions flex items-center ${
                conversationContext?.type === 'nomi' ? 'sendbox-actions--nomi' : 'gap-2'
              }`}
            >
              {renderedRightTools}
              {sendButtonPrefix}
              {composerSubmitCluster}
            </div>
          </div>
        )}
    </ComposerSurface>
  );
};

export default SendBox;
