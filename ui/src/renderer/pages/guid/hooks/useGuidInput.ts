
/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useDragUpload } from '@/renderer/hooks/file/useDragUpload';
import { usePasteService } from '@/renderer/hooks/file/usePasteService';
import { allSupportedExts, type FileMetadata } from '@/renderer/services/FileService';
import { MAX_IMAGE_ATTACHMENTS, admitImageAttachments } from '@/renderer/utils/file/imageAttachments';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { GUID_DRAFT_KEY, useComposerDraftStore } from '@/renderer/stores/composerDraftStore';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

/** Debounce window for persisting the Guid composer draft while typing. */
const DRAFT_PERSIST_DEBOUNCE_MS = 500;

export type GuidInputResult = {
  input: string;
  setInput: React.Dispatch<React.SetStateAction<string>>;
  files: string[];
  setFiles: React.Dispatch<React.SetStateAction<string[]>>;
  dir: string;
  setDir: React.Dispatch<React.SetStateAction<string>>;
  isInputFocused: boolean;
  loading: boolean;
  setLoading: React.Dispatch<React.SetStateAction<boolean>>;
  handleFilesPasted: (pastedFiles: FileMetadata[]) => void;
  handleFilesUploaded: (uploadedPaths: string[]) => void;
  handleRemoveFile: (targetPath: string) => void;
  handleTextareaFocus: () => void;
  handleTextareaBlur: () => void;
  onPaste: ReturnType<typeof usePasteService>['onPaste'];
  isFileDragging: boolean;
  dragHandlers: ReturnType<typeof useDragUpload>['dragHandlers'];
};

type UseGuidInputOptions = {
  locationState: { workspace?: string } | null;
  /**
   * Container ref for Tauri native drag-drop hit-testing (desktop only).
   * When omitted, the Tauri native path stays inactive and only HTML5 drop works.
   */
  containerRef?: React.RefObject<HTMLElement | null>;
};

/**
 * Hook that manages input state, file handling, and drag/paste for the Guid page.
 *
 * The composer snapshot (text + attachments + workspace) is persisted to the
 * app-global composer draft store so it survives navigation between modules and
 * app restarts. It is cleared only after an accepted send (see useGuidSend); a
 * failed send keeps the draft intact so the user can retry.
 */
export const useGuidInput = ({ locationState, containerRef }: UseGuidInputOptions): GuidInputResult => {
  const { t } = useTranslation();
  const setDraft = useComposerDraftStore((state) => state.setDraft);
  const [input, setInput] = useState(() => useComposerDraftStore.getState().drafts[GUID_DRAFT_KEY]?.text ?? '');
  const [files, setFiles] = useState<string[]>(() => useComposerDraftStore.getState().drafts[GUID_DRAFT_KEY]?.files ?? []);
  const [dir, setDir] = useState<string>(() => useComposerDraftStore.getState().drafts[GUID_DRAFT_KEY]?.dir ?? '');
  const [isInputFocused, setIsInputFocused] = useState(false);
  const [loading, setLoading] = useState(false);

  // Read workspace from location.state (passed from tabs add button)
  useEffect(() => {
    if (locationState?.workspace) {
      setDir(locationState.workspace);
    }
  }, [locationState]);

  // Mirror the live snapshot for debounced + unmount persistence. The local
  // state stays the single source of truth for the controlled inputs so the
  // existing setInput(prev => …) call sites keep working unchanged.
  const latestDraftRef = useRef({ input, files, dir });
  latestDraftRef.current = { input, files, dir };

  useEffect(() => {
    const timer = setTimeout(() => {
      setDraft(GUID_DRAFT_KEY, { text: input, files, dir });
    }, DRAFT_PERSIST_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [input, files, dir, setDraft]);

  // Flush any pending change on unmount (route change / navigate to a session)
  // so recent typing is not lost between the debounce windows. An empty
  // snapshot normalizes to a delete, which is how an accepted send clears it.
  useEffect(
    () => () => {
      const snapshot = latestDraftRef.current;
      useComposerDraftStore.getState().setDraft(GUID_DRAFT_KEY, snapshot);
    },
    []
  );

  const appendFilesWithinImageLimit = useCallback(
    (candidatePaths: string[]) => {
      const admission = admitImageAttachments(files, candidatePaths);
      if (admission.rejectedImageCount > 0) {
        Message.warning(t('conversation.chat.imageAttachmentLimit', { limit: MAX_IMAGE_ATTACHMENTS }));
      }
      if (admission.acceptedPaths.length > 0) {
        setFiles((prevFiles) => Array.from(new Set([...prevFiles, ...admission.acceptedPaths])));
      }
    },
    [files, t]
  );

  // Paste, drag, and file selection all use the same message-image admission rule.
  // Do NOT clear dir here: attached files coexist with a selected workspace.
  const handleFilesPasted = useCallback(
    (pastedFiles: FileMetadata[]) => appendFilesWithinImageLimit(pastedFiles.map((file) => file.path)),
    [appendFilesWithinImageLimit]
  );

  const handleFilesUploaded = useCallback(
    (uploadedPaths: string[]) => appendFilesWithinImageLimit(uploadedPaths),
    [appendFilesWithinImageLimit]
  );

  const handleRemoveFile = useCallback((targetPath: string) => {
    setFiles((prevFiles) => prevFiles.filter((file) => file !== targetPath));
  }, []);

  // Use drag upload hook (drag treated like paste, appends to existing files)
  const { isFileDragging, dragHandlers } = useDragUpload({
    onFilesAdded: handleFilesPasted,
    containerRef,
  });

  // Use shared PasteService integration (paste appends to existing files)
  const { onPaste, onFocus } = usePasteService({
    supportedExts: allSupportedExts,
    onFilesAdded: handleFilesPasted,
  });

  const handleTextareaFocus = useCallback(() => {
    onFocus();
    setIsInputFocused(true);
  }, [onFocus]);

  const handleTextareaBlur = useCallback(() => {
    setIsInputFocused(false);
  }, []);

  return {
    input,
    setInput,
    files,
    setFiles,
    dir,
    setDir,
    isInputFocused,
    loading,
    setLoading,
    handleFilesPasted,
    handleFilesUploaded,
    handleRemoveFile,
    handleTextareaFocus,
    handleTextareaBlur,
    onPaste,
    isFileDragging,
    dragHandlers,
  };
};
