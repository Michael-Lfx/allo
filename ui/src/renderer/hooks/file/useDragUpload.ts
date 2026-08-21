

import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AppMessage as Message } from '@/renderer/components/notifications';
import type { FileMetadata } from '@renderer/services/FileService';
import { FileService } from '@renderer/services/FileService';
import type { UploadSource } from '@renderer/hooks/file/useUploadState';
import type { ConversationId } from '@/common/types/ids';
import { isTauriRuntime } from '@/common/adapter/tauriRuntime';
import { isPhysicalPointOverElement, pathsToFileMetadata, registerDropzone } from '@renderer/utils/tauriDragDrop';
import { useLatestRef } from '@renderer/hooks/ui/useLatestRef';

export interface UseDragUploadOptions {
  supportedExts?: string[];
  onFilesAdded?: (files: FileMetadata[]) => void;
  /** Conversation ID for WebUI file uploads */
  conversation_id?: ConversationId;
  /** Upload surface used for progress scoping (defaults to 'sendbox') */
  source?: UploadSource;
  /**
   * Container ref used to hit-test Tauri native drag-drop events (desktop only).
   * When omitted, the Tauri native path stays inactive and only HTML5 drop works
   * (the WebUI/browser behavior is unchanged).
   */
  containerRef?: React.RefObject<HTMLElement | null>;
}

export const useDragUpload = ({
  supportedExts: _supportedExts = [],
  onFilesAdded,
  conversation_id,
  source = 'sendbox',
  containerRef,
}: UseDragUploadOptions) => {  const { t } = useTranslation();
  const [isFileDragging, setIsFileDragging] = useState(false);

  // 拖拽计数器，防止状态闪烁
  const dragCounter = useRef(0);

  // Latest-callback ref: the Tauri subscription reads onFilesAdded through this
  // so the listener is set up once per container and doesn't churn when callers
  // rebuild their handler (e.g. AttachmentsField, whose handler depends on the
  // current attachment list).
  const onFilesAddedLatest = useLatestRef(onFilesAdded);

  const handleDragOver = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (!isFileDragging) {
        setIsFileDragging(true);
        dragCounter.current += 1;
      }
    },
    [isFileDragging]
  );

  const handleDragEnter = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();

    dragCounter.current += 1;
    setIsFileDragging(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();

    dragCounter.current -= 1;

    if (dragCounter.current <= 0) {
      dragCounter.current = 0;
      setIsFileDragging(false);
    }
  }, []);

  const handleDrop = useCallback(
    async (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();

      // 重置状态
      dragCounter.current = 0;
      setIsFileDragging(false);

      if (!onFilesAdded) return;

      try {
        const droppedFiles = e.nativeEvent.dataTransfer!.files;

        if (droppedFiles.length > 0) {
          const processedFiles = await FileService.processDroppedFiles(droppedFiles, conversation_id, source);

          if (processedFiles.length > 0) {
            onFilesAdded(processedFiles);
          }
        }
      } catch (err) {
        console.error('Failed to process dropped files:', err);
        Message.error(t('conversation.workspace.dragFailed', 'Failed to process dropped files'));
      }
    },
    [conversation_id, onFilesAdded, source, t]
  );

  /**
   * Tauri 桌面端：订阅原生拖拽事件。
   * webview 的 HTML5 `drop` 收不到 OS 文件拖放（dataTransfer.files 为空），因此改从
   * `onDragDropEvent` 读取主机绝对路径。通过命中测试 containerRef，只处理落在自身区域的
   * 拖放；其他区域（如工作区栏）由各自的全局监听处理。模式参考 useWorkspaceDragImport.ts。
   *
   * Tauri: the webview HTML5 drop never receives OS file drops, so host paths
   * come from onDragDropEvent instead. We hit-test containerRef so this hook only
   * reacts to drops on its own surface — the workspace sidebar handles the rest.
   *
   * The effect depends only on `containerRef` (a stable ref object): `onFilesAdded`
   * is read through `onFilesAddedLatest` so the subscription is set up once and
   * survives callback identity churn. The container is also registered in the
   * shared dropzone registry so the workspace catch-all listener can yield.
   */
  useEffect(() => {
    if (!containerRef) return;
    if (!isTauriRuntime()) return;

    let unlisten: (() => void) | undefined;
    let unregisterDropzone: (() => void) | undefined;
    let cancelled = false;
    let rafId: number | null = null;

    const cancelRaf = () => {
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
    };

    void (async () => {
      try {
        // Register so the workspace catch-all import listener yields drops that
        // land on this surface. (Best-effort at mount time; SendBox's wrapper is
        // always mounted so it always registers. Conditionally-rendered zones
        // like AttachmentsField's collapsed dropzone simply won't register until
        // remount — fine, since they don't co-exist with the workspace listener.)
        if (containerRef.current) {
          unregisterDropzone = registerDropzone(containerRef.current);
        }

        const { getCurrentWebview } = await import('@tauri-apps/api/webview');
        const fn = await getCurrentWebview().onDragDropEvent((event) => {
          const payload = event.payload as {
            type: string;
            paths?: string[];
            position?: { x: number; y: number };
          };
          const target = containerRef.current;

          if (payload.type === 'enter' || payload.type === 'over') {
            if (payload.position) {
              const { x, y } = payload.position;
              // Coalesce rapid enter/over ticks into one animation frame so we
              // don't force a layout flush (elementsFromPoint) on every drag-move.
              cancelRaf();
              rafId = requestAnimationFrame(() => {
                rafId = null;
                setIsFileDragging(isPhysicalPointOverElement(x, y, target));
              });
            }
          } else if (payload.type === 'leave' || payload.type === 'cancel') {
            // leave 不带 position；触发即说明拖拽已离开窗口，直接清高亮。
            cancelRaf();
            setIsFileDragging(false);
          } else if (payload.type === 'drop') {
            cancelRaf();
            setIsFileDragging(false);
            if (
              payload.position &&
              Array.isArray(payload.paths) &&
              payload.paths.length > 0 &&
              isPhysicalPointOverElement(payload.position.x, payload.position.y, target)
            ) {
              onFilesAddedLatest.current?.(pathsToFileMetadata(payload.paths));
            }
          }
        });
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      } catch (error) {
        console.warn('[useDragUpload] Failed to attach Tauri drag-drop listener:', error);
      }
    })();

    return () => {
      cancelled = true;
      cancelRaf();
      if (unlisten) unlisten();
      if (unregisterDropzone) unregisterDropzone();
    };
  }, [containerRef]);

  const dragHandlers = {
    onDragOver: handleDragOver,
    onDragEnter: handleDragEnter,
    onDragLeave: handleDragLeave,
    onDrop: handleDrop,
  };

  return {
    isFileDragging,
    dragHandlers,
  };
};
