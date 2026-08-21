import { getFileTypeInfo } from '@/renderer/utils/file/fileType';
import {
  FileCode,
  FileExcel,
  FileMusic,
  FilePdf,
  FilePpt,
  FileText,
  FileTxt,
  FileWord,
  FileZip,
  Pic,
  VideoTwo,
  WebPage,
} from '@icon-park/react';
import React from 'react';

const ARCHIVE_EXTENSIONS = new Set(['zip', '7z', 'rar', 'tar', 'gz', 'bz2', 'xz', 'tgz']);
const AUDIO_EXTENSIONS = new Set(['mp3', 'wav', 'flac', 'ogg', 'm4a', 'aac']);
const VIDEO_EXTENSIONS = new Set(['mp4', 'mov', 'webm', 'avi', 'mkv']);
const TEXT_EXTENSIONS = new Set(['txt', 'log']);

export type WorkspaceFileIconKind =
  | 'archive'
  | 'audio'
  | 'code'
  | 'excel'
  | 'html'
  | 'image'
  | 'markdown'
  | 'pdf'
  | 'presentation'
  | 'text'
  | 'video'
  | 'word';

const iconProps = { theme: 'outline', size: 14, strokeWidth: 3, fill: 'currentColor' } as const;

const FILE_ICON_NODE_BY_KIND: Record<WorkspaceFileIconKind, React.ReactNode> = {
  archive: <FileZip {...iconProps} />,
  audio: <FileMusic {...iconProps} />,
  code: <FileCode {...iconProps} />,
  excel: <FileExcel {...iconProps} />,
  html: <WebPage {...iconProps} />,
  image: <Pic {...iconProps} />,
  markdown: <FileText {...iconProps} />,
  pdf: <FilePdf {...iconProps} />,
  presentation: <FilePpt {...iconProps} />,
  text: <FileTxt {...iconProps} />,
  video: <VideoTwo {...iconProps} />,
  word: <FileWord {...iconProps} />,
};

export const getWorkspaceFileIconKind = (fileName: string): WorkspaceFileIconKind => {
  const extension = fileName.toLowerCase().split('.').pop() || '';

  if (ARCHIVE_EXTENSIONS.has(extension)) return 'archive';
  if (AUDIO_EXTENSIONS.has(extension)) return 'audio';
  if (VIDEO_EXTENSIONS.has(extension)) return 'video';
  if (TEXT_EXTENSIONS.has(extension)) return 'text';

  switch (getFileTypeInfo(fileName).contentType) {
    case 'excel':
      return 'excel';
    case 'html':
      return 'html';
    case 'image':
      return 'image';
    case 'markdown':
      return 'markdown';
    case 'pdf':
      return 'pdf';
    case 'ppt':
      return 'presentation';
    case 'word':
      return 'word';
    default:
      return 'code';
  }
};

const WorkspaceFileIcon: React.FC<{ fileName: string }> = ({ fileName }) => {
  const kind = getWorkspaceFileIconKind(fileName);

  return (
    <span className={`workspace-file-type-icon workspace-file-type-icon--${kind}`} aria-hidden='true'>
      {FILE_ICON_NODE_BY_KIND[kind]}
    </span>
  );
};

export default WorkspaceFileIcon;
