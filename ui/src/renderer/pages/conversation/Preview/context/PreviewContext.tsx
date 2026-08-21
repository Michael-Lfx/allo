

import { ipcBridge } from '@/common';
import type { ITerminalSession } from '@/common/adapter/ipcBridge';
import type { ConversationId, TerminalId } from '@/common/types/ids';
import type { PreviewContentType } from '@/common/types/office/preview';
import { emitter } from '@/renderer/utils/emitter';
import React, { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { resolveActiveTabAfterClose } from '../previewTabCloseFallback';
import {
  inferPreviewTabKind,
  type PreviewTabKind,
  type WorkspacePreviewTabDefinition,
} from '../previewTabKind';
import { findWorkspacePreviewTab, upsertMixedPreviewTab } from '../previewTabUpsert';

export type { PreviewTabKind };
export type { WorkspacePreviewTabDefinition };

/** DOM 片段数据结构 / DOM snippet data structure */
export interface DomSnippet {
  /** 唯一 ID / Unique ID */
  id: string;
  /** 简化标签名（用于显示）/ Simplified tag name (for display) */
  tag: string;
  /** 完整 HTML / Full HTML */
  html: string;
}

export interface PreviewMetadata {
  language?: string;
  title?: string;
  diff?: string;
  file_name?: string;
  file_path?: string; // 工作空间文件的绝对路径 / Absolute file path in workspace
  workspace?: string; // 工作空间根目录 / Workspace root directory
  editable?: boolean; // 是否可编辑 / Whether editable
  truncated?: boolean; // 预览内容是否被截断 / Whether preview content was truncated
  /**
   * 打开该预览的会话。预览面板挂在 ConversationProvider 之外，因此需要打开方
   * （`usePreviewLauncher` / 自动预览钩子）把会话身份随元数据带进来。
   * Owning conversation. The panel is mounted OUTSIDE `ConversationProvider`,
   * so openers stamp the identity here for viewers that need it (the mini-app
   * publish action looks up prior publishes by `source_conversation_id`).
   */
  conversation_id?: ConversationId;
}

export interface PreviewTab {
  id: string;
  content: string;
  content_type: PreviewContentType;
  metadata?: PreviewMetadata;
  title: string; // Tab 标题
  isDirty?: boolean; // 是否有未保存的修改 / Whether there are unsaved changes
  originalContent?: string; // 原始内容，用于对比 / Original content for comparison
  kind?: PreviewTabKind;
  terminal_id?: TerminalId;
  /** Stable key for a fixed workspace view such as files or changes. */
  workspaceTabKey?: string;
  /** When true, closing this tab kills and removes the backing PTY session. */
  killOnClose?: boolean;
}

export interface PreviewContextValue {
  // 预览面板状态 / Preview panel state
  isOpen: boolean;
  tabs: PreviewTab[]; // 所有打开的 tabs
  activeTabId: string | null; // 当前激活的 tab ID

  // 获取当前激活的 tab / Get active tab
  activeTab: PreviewTab | null;

  workspacePath?: string;
  // 预览面板操作 / Preview panel operations
  openPreview: (content: string, type: PreviewContentType, metadata?: PreviewMetadata) => void;
  openTerminalTab: (session: ITerminalSession, options?: { killOnClose?: boolean }) => void;
  openBrowserTab: (url: string) => boolean;
  openWorkspaceTab: (definition: WorkspacePreviewTabDefinition) => void;
  closePreview: () => void;
  /** Closes a tab. When the active tab is closed, returns the tab that became active (or null if none). */
  closeTab: (tabId: string) => PreviewTab | null;
  switchTab: (tabId: string) => void;
  updateContent: (content: string) => void;
  saveContent: (tabId?: string) => Promise<boolean>; // 保存内容 / Save content
  findPreviewTab: (type: PreviewContentType, content?: string, metadata?: PreviewMetadata) => PreviewTab | null; // 查找匹配的 tab
  closePreviewByIdentity: (type: PreviewContentType, content?: string, metadata?: PreviewMetadata) => void; // 根据内容关闭指定 tab

  // 发送框集成 / Sendbox integration
  addToSendBox: (text: string) => void;
  setSendBoxHandler: (handler: ((text: string) => void) | null) => void;

  // DOM 片段管理 / DOM snippet management
  domSnippets: DomSnippet[];
  addDomSnippet: (tag: string, html: string) => void;
  removeDomSnippet: (id: string) => void;
  clearDomSnippets: () => void;
}

const PreviewContext = createContext<PreviewContextValue | null>(null);

// Callers pass a generation-scoped, typed browser-storage namespace. No legacy
// preview key is read.
const previewTabsKey = (ns: string) =>
  `nomifun_preview_tabs:${ns}:tabs`;
const previewActiveTabIdKey = (ns: string) =>
  `nomifun_preview_tabs:${ns}:active`;

// 仅持久化小体积文本预览，避免大文本导致 localStorage 写入卡顿
// Persist only lightweight text previews to avoid localStorage jank on large files
const MAX_PERSISTED_TAB_CONTENT_LENGTH = 80_000;
const PERSISTABLE_CONTENT_TYPES = new Set<PreviewContentType>(['markdown', 'html', 'code', 'diff']);

const sanitizeTabsForPersistence = (input: PreviewTab[]): PreviewTab[] => {
  return input
    .filter((tab) => inferPreviewTabKind(tab) === 'file')
    .filter((tab) => PERSISTABLE_CONTENT_TYPES.has(tab.content_type))
    .filter((tab) => tab.content.length <= MAX_PERSISTED_TAB_CONTENT_LENGTH)
    .map((tab) => ({
      ...tab,
      isDirty: false,
      originalContent: tab.content,
    }));
};

const disposeOwnedTerminal = (tab: PreviewTab) => {
  if (inferPreviewTabKind(tab) !== 'terminal' || !tab.killOnClose || !tab.terminal_id) return;
  const terminal_id = tab.terminal_id;
  void ipcBridge.terminal.kill
    .invoke({ terminal_id })
    .catch(() => undefined)
    .finally(() => {
      void ipcBridge.terminal.remove.invoke({ terminal_id }).catch(() => undefined);
    });
};

const parsePersistedTabs = (value: unknown): PreviewTab[] => {
  if (!Array.isArray(value)) return [];

  return value
    .filter((tab): tab is PreviewTab => {
      if (!tab || typeof tab !== 'object') return false;
      const candidate = tab as Partial<PreviewTab>;
      return (
        typeof candidate.id === 'string' &&
        typeof candidate.title === 'string' &&
        typeof candidate.content === 'string' &&
        typeof candidate.content_type === 'string'
      );
    })
    .filter((tab) => inferPreviewTabKind(tab) === 'file')
    .filter((tab) => PERSISTABLE_CONTENT_TYPES.has(tab.content_type))
    .filter((tab) => tab.content.length <= MAX_PERSISTED_TAB_CONTENT_LENGTH)
    .map((tab) => ({
      ...tab,
      originalContent: typeof tab.originalContent === 'string' ? tab.originalContent : tab.content,
      isDirty: false,
    }));
};

// 从 localStorage 恢复状态 / Restore state from localStorage
// 注意：isOpen 不从 localStorage 恢复，新会话时预览面板默认关闭
// Note: isOpen is not restored from localStorage, preview panel is closed by default for new sessions
const loadPersistedState = (ns: string): { isOpen: boolean; tabs: PreviewTab[]; activeTabId: string | null } => {
  try {
    const tabs = parsePersistedTabs(JSON.parse(localStorage.getItem(previewTabsKey(ns)) || '[]'));
    let activeTabId = localStorage.getItem(previewActiveTabIdKey(ns));

    if (activeTabId && !tabs.some((tab) => tab.id === activeTabId)) {
      activeTabId = tabs[0]?.id || null;
    }

    return {
      isOpen: false, // 始终默认关闭 / Always start closed
      tabs,
      activeTabId,
    };
  } catch {
    // 忽略解析错误 / Ignore parsing errors
  }
  return { isOpen: false, tabs: [], activeTabId: null };
};

export const PreviewProvider: React.FC<{
  children: React.ReactNode;
  /**
   * Generation-scoped persistence namespace created by browserStorageKey().
   * Callers must pass the typed owning entity kind and ID rather than composing
   * arbitrary `kind:id` strings.
   */
  persistNamespace: string;
  /**
   * 是否订阅全局 `preview.open` 事件（emitter + IPC），供 agent/MCP 主动打开预览。
   * 仅应有「主」表面（会话）订阅；否则一次打开会被多个 provider 重复处理。
   * 默认 true，保持既有行为不变。
   * Whether to subscribe to the global `preview.open` event (emitter + IPC) used
   * by agents/MCP to open a preview programmatically. Only the "primary" surface
   * (conversation) should subscribe; otherwise a single open would be handled by
   * multiple providers. Defaults to true to preserve existing behavior.
   */
  subscribeGlobalOpen?: boolean;
  /**
   * Workspace root used when the plus menu creates a shell tab or opens a file.
   */
  workspacePath?: string;
}> = ({ children, persistNamespace, subscribeGlobalOpen = true, workspacePath }) => {
  // 从 localStorage 恢复初始状态 / Restore initial state from localStorage
  const persistedState = loadPersistedState(persistNamespace);
  const [isOpen, setIsOpen] = useState(persistedState.isOpen);
  const [tabs, setTabs] = useState<PreviewTab[]>(persistedState.tabs);
  const [activeTabId, setActiveTabId] = useState<string | null>(persistedState.activeTabId);
  // const [sendBoxHandler, setSendBoxHandlerState] = useState<((text: string) => void) | null>(null);
  const sendBoxHandler = useRef<((text: string) => void) | null>(null);
  const [domSnippets, setDomSnippets] = useState<DomSnippet[]>([]);
  /** Last focused tab before the current one — used when closing the active tab. */
  const previousActiveTabIdRef = useRef<string | null>(null);
  const activeTabIdRef = useRef<string | null>(activeTabId);
  activeTabIdRef.current = activeTabId;

  const activateTab = useCallback((nextTabId: string | null, options?: { recordHistory?: boolean }) => {
    const current = activeTabIdRef.current;
    const recordHistory = options?.recordHistory !== false;
    if (recordHistory && current && nextTabId && current !== nextTabId) {
      previousActiveTabIdRef.current = current;
    }
    activeTabIdRef.current = nextTabId;
    setActiveTabId(nextTabId);
  }, []);

  // 持久化 tabs 到 localStorage（仅保存小体积文本 tab）
  // Persist tabs to localStorage (only lightweight text tabs)
  useEffect(() => {
    const timer = setTimeout(() => {
      try {
        localStorage.setItem(previewTabsKey(persistNamespace), JSON.stringify(sanitizeTabsForPersistence(tabs)));
      } catch {
        // 忽略存储错误（如存储空间不足）/ Ignore storage errors (e.g., quota exceeded)
      }
    }, 150);

    return () => clearTimeout(timer);
  }, [tabs, persistNamespace]);

  // 持久化 activeTabId（单独存储，避免切换 tab 时重复序列化大内容）
  // Persist activeTabId separately to avoid re-serializing large tab content on tab switch
  useEffect(() => {
    try {
      if (activeTabId) {
        localStorage.setItem(previewActiveTabIdKey(persistNamespace), activeTabId);
      } else {
        localStorage.removeItem(previewActiveTabIdKey(persistNamespace));
      }
    } catch {
      // 忽略存储错误 / Ignore storage errors
    }
  }, [activeTabId, persistNamespace]);

  // 追踪是否正在保存（避免与流式更新冲突）/ Track if currently saving (to avoid conflicts with streaming updates)
  const savingFilesRef = useRef<Set<string>>(new Set());

  // 获取当前激活的 tab / Get active tab
  const activeTab = useMemo(() => {
    return tabs.find((tab) => tab.id === activeTabId) || null;
  }, [tabs, activeTabId]);

  const normalize = useCallback((value?: string | null) => value?.trim() || '', []);

  // 从可能包含描述的字符串中提取文件名 / Extract filename from string that may contain description
  const extractFileName = useCallback((str?: string): string | undefined => {
    if (!str) return undefined;
    // 匹配 "Writing to xxx.md" 或 "Reading xxx.txt" 等模式，提取文件名 / Match patterns like "Writing to xxx.md" and extract filename
    const match = str.match(/(?:Writing to|Reading|Creating|Updating)\s+(.+)$/i);
    return match ? match[1] : str;
  }, []);

  const findPreviewTabInList = useCallback(
    (tabList: PreviewTab[], type: PreviewContentType, content?: string, meta?: PreviewMetadata) => {
      const normalizedFileName = normalize(meta?.file_name);
      const normalizedTitle = normalize(meta?.title);
      const normalizedFilePath = normalize(meta?.file_path);

      return (
        tabList.find((tab) => {
          if (inferPreviewTabKind(tab) === 'terminal') return false;
          if (tab.content_type !== type) return false;
          const tabFileName = normalize(tab.metadata?.file_name);
          const tabTitle = normalize(tab.metadata?.title);
          const tabFilePath = normalize(tab.metadata?.file_path);

          // 优先通过 file_path 匹配（最可靠）/ Prefer matching by file_path (most reliable)
          if (normalizedFilePath && tabFilePath && normalizedFilePath === tabFilePath) return true;

          // 通过 file_name 匹配时，需要确保路径兼容（避免同名文件在不同目录的冲突）
          // When matching by file_name, ensure path compatibility (avoid conflicts of same-named files in different directories)
          if (normalizedFileName && tabFileName && normalizedFileName === tabFileName) {
            // 如果两边都有 file_path，则必须完全匹配
            // If both have file_path, they must match exactly
            if (normalizedFilePath && tabFilePath) {
              return normalizedFilePath === tabFilePath;
            }
            // 如果只有一边有 file_path，不能仅凭 file_name 匹配
            // If only one side has file_path, cannot match by file_name alone
            if (normalizedFilePath || tabFilePath) {
              return false;
            }
            // 都没有 file_path 时，可以通过 file_name 匹配
            // When neither has file_path, can match by file_name
            return true;
          }

          // 再通过 title 匹配 / Then match by title
          if (!normalizedFileName && normalizedTitle && tabTitle && normalizedTitle === tabTitle) return true;

          // 最后才通过 content 匹配（仅用于小文件）/ Finally match by content (only for small files)
          // 对于大文件（PPT/Excel/Word），不使用 content 比较，避免性能问题
          // For large files (PPT/Excel/Word), skip content comparison to avoid performance issues
          if (!normalizedFileName && !normalizedTitle && !normalizedFilePath && content !== undefined) {
            // 只对小于 100KB 的内容进行比较 / Only compare content smaller than 100KB
            if (content.length < 100000 && tab.content === content) return true;
          }

          return false;
        }) || null
      );
    },
    [normalize]
  );

  const findPreviewTab = useCallback(
    (type: PreviewContentType, content?: string, meta?: PreviewMetadata) => {
      return findPreviewTabInList(tabs, type, content, meta);
    },
    [findPreviewTabInList, tabs]
  );

  const openPreview = useCallback(
    (new_content: string, type: PreviewContentType, meta?: PreviewMetadata) => {
      let nextActiveTabId: string | null = null;
      const kind: PreviewTabKind = type === 'url' ? 'browser' : 'file';

      setTabs((prevTabs) => {
        const existingTab = findPreviewTabInList(prevTabs, type, new_content, meta);
        const reusableFileTab =
          !existingTab && kind === 'file'
            ? prevTabs.find((tab) => inferPreviewTabKind(tab) === 'file')
            : undefined;
        const fallbackTitle = (() => {
          if (type === 'markdown') return 'Markdown';
          if (type === 'diff') return 'Diff';
          if (type === 'code') return `${meta?.language || 'Code'}`;
          if (type === 'image') return 'Image';
          if (type === 'url') return 'Browser';
          return 'Preview';
        })();
        const title = extractFileName(meta?.file_name) || extractFileName(meta?.title) || fallbackTitle;
        const tabId =
          existingTab?.id ??
          reusableFileTab?.id ??
          `${type}-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`;

        const keepDirtyContent = Boolean(existingTab?.isDirty);
        const nextTab: PreviewTab = {
          id: tabId,
          content: keepDirtyContent ? existingTab?.content ?? new_content : new_content,
          content_type: type,
          kind,
          metadata: meta ? { ...existingTab?.metadata, ...meta } : existingTab?.metadata,
          title: keepDirtyContent ? existingTab?.title ?? title : title,
          isDirty: keepDirtyContent,
          originalContent: keepDirtyContent
            ? existingTab?.originalContent ?? new_content
            : new_content,
        };

        const nextTabs = upsertMixedPreviewTab(prevTabs, kind, existingTab ?? undefined, nextTab);
        nextActiveTabId = nextTab.id;
        return nextTabs;
      });

      if (nextActiveTabId) {
        activateTab(nextActiveTabId);
      }
      setIsOpen(true);
    },
    [activateTab, extractFileName, findPreviewTabInList]
  );

  const openTerminalTab = useCallback((session: ITerminalSession, options?: { killOnClose?: boolean }) => {
    let nextActiveTabId: string | null = null;
    setTabs((prevTabs) => {
      const existing = prevTabs.find(
        (tab) => inferPreviewTabKind(tab) === 'terminal' && tab.terminal_id === session.terminal_id
      );
      if (existing) {
        nextActiveTabId = existing.id;
        return prevTabs;
      }
      const tabId = `terminal-${session.terminal_id}`;
      nextActiveTabId = tabId;
      const newTab: PreviewTab = {
        id: tabId,
        content: '',
        content_type: 'code',
        kind: 'terminal',
        title: session.name || 'Terminal',
        terminal_id: session.terminal_id,
        killOnClose: options?.killOnClose ?? true,
        isDirty: false,
        originalContent: '',
      };
      return [...prevTabs, newTab];
    });
    if (nextActiveTabId) {
      activateTab(nextActiveTabId);
    }
    setIsOpen(true);
  }, [activateTab]);

  const openBrowserTab = useCallback(
    (rawUrl: string): boolean => {
      const trimmed = rawUrl.trim();
      if (!trimmed) return false;
      const withProtocol = /^[a-zA-Z][a-zA-Z\d+\-.]*:/.test(trimmed) ? trimmed : `https://${trimmed}`;
      try {
        const parsed = new URL(withProtocol);
        if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return false;
        openPreview(parsed.toString(), 'url', { title: parsed.hostname || parsed.href });
        return true;
      } catch {
        return false;
      }
    },
    [openPreview]
  );

  const openWorkspaceTab = useCallback((definition: WorkspacePreviewTabDefinition) => {
    let nextActiveTabId: string | null = null;
    setTabs((prevTabs) => {
      const existing = findWorkspacePreviewTab(prevTabs, definition.key);
      const nextTab: PreviewTab = {
        id: existing?.id ?? `workspace-${definition.key}`,
        content: '',
        content_type: 'code',
        kind: 'workspace',
        workspaceTabKey: definition.key,
        title: definition.title,
        isDirty: false,
        originalContent: '',
      };
      nextActiveTabId = nextTab.id;
      return upsertMixedPreviewTab(prevTabs, 'workspace', existing, nextTab);
    });
    if (nextActiveTabId) {
      activateTab(nextActiveTabId);
    }
    setIsOpen(true);
  }, [activateTab]);

  const tabsRef = useRef<PreviewTab[]>(tabs);
  tabsRef.current = tabs;

  const closePreview = useCallback(() => {
    tabsRef.current.forEach(disposeOwnedTerminal);
    setIsOpen(false);
    setTabs([]);
    previousActiveTabIdRef.current = null;
    activateTab(null, { recordHistory: false });
    setDomSnippets([]);
  }, [activateTab]);

  // Track last-known mtime per file path for external change detection
  const fileMtimeRef = useRef<Map<string, number>>(new Map());

  const closeTab = useCallback(
    (tabId: string): PreviewTab | null => {
      let nextActiveTab: PreviewTab | null | undefined = undefined;

      setTabs((prevTabs) => {
        const closedIndex = prevTabs.findIndex((tab) => tab.id === tabId);
        const tabToClose = closedIndex >= 0 ? prevTabs[closedIndex] : undefined;
        if (tabToClose) {
          disposeOwnedTerminal(tabToClose);
          if (tabToClose.metadata?.file_path) {
            fileMtimeRef.current.delete(tabToClose.metadata.file_path);
          }
        }

        const newTabs = prevTabs.filter((tab) => tab.id !== tabId);
        // Keep ref in sync so consecutive closeTab calls (close others / close all)
        // see the latest list even before the next render.
        tabsRef.current = newTabs;

        if (previousActiveTabIdRef.current === tabId) {
          previousActiveTabIdRef.current = null;
        }

        if (tabId === activeTabIdRef.current) {
          const nextActiveTabId = resolveActiveTabAfterClose(
            newTabs,
            previousActiveTabIdRef.current
          );
          if (nextActiveTabId && nextActiveTabId === previousActiveTabIdRef.current) {
            previousActiveTabIdRef.current = null;
          }
          nextActiveTab = nextActiveTabId
            ? (newTabs.find((tab) => tab.id === nextActiveTabId) ?? null)
            : null;
          // Preemptively sync so chained closes observe the post-close active id.
          activeTabIdRef.current = nextActiveTabId;
        }

        return newTabs;
      });

      if (nextActiveTab !== undefined) {
        activateTab(nextActiveTab?.id ?? null, { recordHistory: false });
        if (!nextActiveTab) {
          setIsOpen(false);
        }
        return nextActiveTab;
      }

      return tabsRef.current.find((tab) => tab.id === activeTabIdRef.current) ?? null;
    },
    [activateTab]
  );

  const closePreviewByIdentity = useCallback(
    (type: PreviewContentType, content?: string, meta?: PreviewMetadata) => {
      const tab = findPreviewTab(type, content, meta);
      if (tab) {
        closeTab(tab.id);
      }
    },
    [findPreviewTab, closeTab]
  );

  const updateContent = useCallback(
    (new_content: string) => {
      if (!activeTabId) {
        return;
      }

      // 严格的类型检查，防止 Event 对象被错误传递 / Strict type checking to prevent Event object from being passed incorrectly
      if (typeof new_content !== 'string') {
        return;
      }

      try {
        setTabs((prevTabs) => {
          const updated = prevTabs.map((tab) => {
            if (tab.id === activeTabId) {
              // 检查内容是否与原始内容不同 / Check if content differs from original
              const isDirty = new_content !== tab.originalContent;
              return { ...tab, content: new_content, isDirty };
            }
            return tab;
          });
          return updated;
        });
      } catch {
        // Silently ignore errors
      }
    },
    [activeTabId]
  );

  const saveContent = useCallback(
    async (tabId?: string) => {
      const targetTabId = tabId || activeTabId;
      if (!targetTabId) return false;

      const tab = tabs.find((t) => t.id === targetTabId);
      if (!tab) return false;

      // 如果有 file_path 和 workspace，写回工作空间文件 / If file_path and workspace exist, write back to workspace file
      if (tab.metadata?.file_path && tab.metadata?.workspace) {
        try {
          const file_path = tab.metadata.file_path;

          // 标记文件正在保存（避免触发文件监听回调）/ Mark file as being saved (to avoid triggering file watch callback)
          savingFilesRef.current.add(file_path);

          // 使用 IPC 写入文件 / Write file via IPC
          const success = await ipcBridge.fs.writeFile.invoke({
            path: file_path,
            data: tab.content,
          });

          if (success) {
            setTabs((prevTabs) =>
              prevTabs.map((t) => {
                if (t.id === targetTabId) {
                  return { ...t, isDirty: false, originalContent: t.content };
                }
                return t;
              })
            );
          }

          // 延迟移除保存标记（给文件监听一点时间忽略变化）/ Delay removing save flag (give file watch time to ignore change)
          setTimeout(() => {
            savingFilesRef.current.delete(file_path);
          }, 500);

          return success;
        } catch (error) {
          // 发生错误，静默处理（只记录到控制台）/ Error occurred, handle silently (log only)
          // 确保移除保存标记 / Ensure save flag is removed
          if (tab.metadata?.file_path) {
            savingFilesRef.current.delete(tab.metadata.file_path);
          }
          throw error;
        }
      }
      return false;
    },
    [activeTabId, tabs]
  );

  const addToSendBox = useCallback((text: string) => {
    if (sendBoxHandler.current) {
      sendBoxHandler.current(text);
    }
  }, []);

  const setSendBoxHandler = useCallback((handler: ((text: string) => void) | null) => {
    sendBoxHandler.current = handler;
  }, []);

  // DOM 片段管理函数 / DOM snippet management functions
  // 只保留最新的一个片段 / Only keep the latest snippet
  const addDomSnippet = useCallback((tag: string, html: string) => {
    const id = `snippet-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`;
    setDomSnippets([{ id, tag, html }]);
  }, []);

  const removeDomSnippet = useCallback((id: string) => {
    setDomSnippets((prev) => prev.filter((s) => s.id !== id));
  }, []);

  const clearDomSnippets = useCallback(() => {
    setDomSnippets([]);
  }, []);

  // 流式内容订阅：订阅 agent 写入文件时的流式更新（替代文件监听）
  // Streaming content subscription: Subscribe to streaming updates when agent writes files (replaces file watching)
  // 使用防抖优化：等待 agent 完成写入后再更新预览，避免打字动画被频繁中断
  // Use debounce optimization: Wait for agent to finish writing before updating preview, avoiding frequent animation interruptions
  useEffect(() => {
    // 防抖定时器映射：每个文件路径对应一个定时器 / Debounce timer map: one timer per file path
    const debounceTimers = new Map<string, NodeJS.Timeout>();

    const unsubscribe = ipcBridge.fileStream.contentUpdate.on(({ file_path, content, operation }) => {
      // 如果是删除操作，立即处理，不需要防抖 / If delete operation, handle immediately without debounce
      if (operation === 'delete') {
        // 清除该文件的防抖定时器 / Clear debounce timer for this file
        const existingTimer = debounceTimers.get(file_path);
        if (existingTimer) {
          clearTimeout(existingTimer);
          debounceTimers.delete(file_path);
        }

        setTabs((prevTabs) => {
          const tabToClose = prevTabs.find((tab) => tab.metadata?.file_path === file_path);
          if (tabToClose) {
            closeTab(tabToClose.id);
          }
          return prevTabs;
        });
        return;
      }

      // 对写入操作进行防抖：500ms 内没有新的更新才真正更新内容
      // Debounce write operations: Only update content if no new updates within 500ms
      const existingTimer = debounceTimers.get(file_path);
      if (existingTimer) {
        clearTimeout(existingTimer);
      }

      const timer = setTimeout(() => {
        // 使用函数式更新来访问最新的 tabs 状态 / Use functional update to access latest tabs state
        setTabs((prevTabs) => {
          // 查找受影响的 tabs / Find affected tabs
          const affectedTabs = prevTabs.filter((tab) => tab.metadata?.file_path === file_path);

          if (affectedTabs.length === 0) {
            return prevTabs;
          }

          return prevTabs.map((tab) => {
            if (tab.metadata?.file_path !== file_path) return tab;

            // 如果正在保存或用户已编辑，不更新 / Don't update if saving or user has edited
            if (savingFilesRef.current.has(file_path) || tab.isDirty) {
              return tab;
            }

            return {
              ...tab,
              content,
              originalContent: content,
              isDirty: false,
            };
          });
        });

        // 清除定时器 / Clean up timer
        debounceTimers.delete(file_path);
      }, 500); // 500ms 防抖时间 / 500ms debounce delay

      debounceTimers.set(file_path, timer);
    });

    return () => {
      unsubscribe();
      // 清理所有防抖定时器 / Clean up all debounce timers
      debounceTimers.forEach((timer) => clearTimeout(timer));
      debounceTimers.clear();
    };
  }, [closeTab]); // 只依赖 closeTab，不依赖 tabs，避免重复订阅 / Only depend on closeTab, not tabs, to avoid re-subscribing

  // File mtime polling: detect external file changes (Claude Code CLI, Gemini, etc.) by comparing lastModified.
  // Only polls the active tab to minimize IPC overhead; checks other tabs once on tab switch.
  // Uses polling instead of fileWatch IPC events because buildEmitter's main→renderer event delivery
  // is unreliable after the first emission in Electron (only the first event reaches the renderer).
  const checkFileUpdate = useCallback(
    (tab: PreviewTab) => {
      const file_path = tab.metadata?.file_path;
      if (!file_path || tab.isDirty || savingFilesRef.current.has(file_path)) return;

      void ipcBridge.fs.getFileMetadata
        .invoke({ path: file_path, workspace: tab.metadata?.workspace })
        .then((metadata) => {
          if (!metadata) return;
          const prevMtime = fileMtimeRef.current.get(file_path);
          fileMtimeRef.current.set(file_path, metadata.lastModified);
          if (prevMtime === undefined || metadata.lastModified === prevMtime) return;

          const readPromise =
            tab.content_type === 'image'
              ? ipcBridge.fs.getImageBase64.invoke({ path: file_path, workspace: tab.metadata?.workspace })
              : ipcBridge.fs.readFile.invoke({ path: file_path, workspace: tab.metadata?.workspace });

          void readPromise
            .then((content) => {
              if (content == null) return;
              setTabs((latest) =>
                latest.map((t) => {
                  if (t.metadata?.file_path !== file_path) return t;
                  if (savingFilesRef.current.has(file_path) || t.isDirty) return t;
                  return { ...t, content, originalContent: content, isDirty: false };
                })
              );
            })
            .catch((error) => {
              console.error('[PreviewContext] Failed to read file after mtime change:', file_path, error);
            });
        })
        .catch((error) => {
          console.error('[PreviewContext] Failed to get file metadata:', file_path, error);
        });
    },
    [setTabs]
  );

  // Keep a ref to activeTab so the polling interval always sees the latest object
  // without re-running the effect on every tabs state change.
  const activeTabRef = useRef<PreviewTab | null>(null);
  activeTabRef.current = activeTab;

  const activeFilePath = activeTab?.metadata?.file_path;

  // Poll active tab every 1s
  useEffect(() => {
    if (!activeFilePath) return;

    const pollId = setInterval(() => {
      const current = activeTabRef.current;
      if (current) checkFileUpdate(current);
    }, 1000);

    // Check immediately on tab switch
    const current = activeTabRef.current;
    if (current) checkFileUpdate(current);

    return () => {
      clearInterval(pollId);
    };
  }, [activeFilePath, checkFileUpdate]);

  // 监听 preview.open 事件（用于 agent 打开网页预览）/ Listen to preview.open event (for agent to open web preview)
  // 同时监听 IPC 和 renderer emitter 两种方式 / Listen to both IPC and renderer emitter
  // 仅当 subscribeGlobalOpen=true 时注册：全局 open 是单一来源（agent/MCP），只应路由到
  // 「主」表面（会话）的 provider，避免多表面 provider 重复处理同一次打开。
  // Only registered when subscribeGlobalOpen=true: the global open is a single source
  // (agent/MCP) that should route only to the "primary" surface (conversation) provider,
  // avoiding duplicate handling of the same open across multiple-surface providers.
  useEffect(() => {
    if (!subscribeGlobalOpen) return;

    const handleEmitterPreviewOpen = (data: {
      content: string;
      contentType: PreviewContentType;
      metadata?: PreviewMetadata;
    }) => {
      if (data && data.content) {
        openPreview(data.content, data.contentType, data.metadata);
      }
    };

    const handleIpcPreviewOpen = (data: {
      content: string;
      content_type: PreviewContentType;
      metadata?: PreviewMetadata;
    }) => {
      if (data && data.content) {
        openPreview(data.content, data.content_type, data.metadata);
      }
    };

    // 监听 renderer emitter 事件 / Listen to renderer emitter event
    emitter.on('preview.open', handleEmitterPreviewOpen);

    // 监听 IPC 事件（来自主进程，如 chrome-devtools MCP 导航）/ Listen to IPC event (from main process, e.g., chrome-devtools MCP navigation)
    const unsubscribeIpc = ipcBridge.preview.open.on(handleIpcPreviewOpen);

    return () => {
      emitter.off('preview.open', handleEmitterPreviewOpen);
      unsubscribeIpc();
    };
  }, [openPreview, subscribeGlobalOpen]);

  const previewContextValue = useMemo(() => {
    return {
      isOpen,
      tabs,
      activeTabId,
      activeTab,
      workspacePath,
      openPreview,
      openTerminalTab,
      openBrowserTab,
      openWorkspaceTab,
      closePreview,
      closeTab,
      switchTab: activateTab,
      updateContent,
      saveContent,
      findPreviewTab,
      closePreviewByIdentity,
      addToSendBox,
      setSendBoxHandler,
      domSnippets,
      addDomSnippet,
      removeDomSnippet,
      clearDomSnippets,
    };
  }, [
    isOpen,
    tabs,
    activeTabId,
    activeTab,
    workspacePath,
    openPreview,
    openTerminalTab,
    openBrowserTab,
    openWorkspaceTab,
    closePreview,
    closeTab,
    activateTab,
    updateContent,
    saveContent,
    findPreviewTab,
    closePreviewByIdentity,
    addToSendBox,
    setSendBoxHandler,
    domSnippets,
    addDomSnippet,
    removeDomSnippet,
    clearDomSnippets,
  ]);

  return <PreviewContext.Provider value={previewContextValue}>{children}</PreviewContext.Provider>;
};

export const usePreviewContext = () => {
  const context = useContext(PreviewContext);
  if (!context) {
    throw new Error('usePreviewContext must be used within PreviewProvider');
  }
  return context;
};

/**
 * 非抛错版本：在没有 PreviewProvider 时返回 null，而不是抛异常。
 * 供可能渲染在 ChatLayout（预览 provider）之外的消费者使用（如 Markdown 里的
 * mermaid 块出现在桌宠/知识库/设定等表面时），避免白屏崩溃。
 * Non-throwing variant: returns null when there is no PreviewProvider instead of
 * throwing. For consumers that may render outside ChatLayout's preview provider
 * (e.g. a mermaid block inside Markdown shown on companion/knowledge/preset
 * surfaces), so they don't white-screen the window.
 */
export const usePreviewContextOptional = (): PreviewContextValue | null => {
  return useContext(PreviewContext);
};
