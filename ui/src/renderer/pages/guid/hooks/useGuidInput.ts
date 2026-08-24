

import { useDragUpload } from '@/renderer/hooks/file/useDragUpload';
import { usePasteService } from '@/renderer/hooks/file/usePasteService';
import { allSupportedExts, type FileMetadata } from '@/renderer/services/FileService';
import { MAX_IMAGE_ATTACHMENTS, admitImageAttachments } from '@/renderer/utils/file/imageAttachments';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

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
 */
export const useGuidInput = ({ locationState, containerRef }: UseGuidInputOptions): GuidInputResult => {
  const { t } = useTranslation();
  const [input, setInput] = useState('');
  const [files, setFiles] = useState<string[]>([]);
  const [dir, setDir] = useState<string>('');
  const [isInputFocused, setIsInputFocused] = useState(false);
  const [loading, setLoading] = useState(false);

  // Read workspace from location.state (passed from tabs add button)
  useEffect(() => {
    if (locationState?.workspace) {
      setDir(locationState.workspace);
    }
  }, [locationState]);

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
