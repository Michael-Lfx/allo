import DiffTable, { type DiffTableFile } from '@renderer/components/beautifulUi/diffTable/DiffTable';
import React from 'react';

/**
 * 文件变更项数据 / File change item data
 */
export interface FileChangeItem {
  /** 文件名 / File name */
  file_name: string;
  /** 完整路径 / Full path */
  fullPath: string;
  /** 新增行数 / Number of insertions */
  insertions: number;
  /** 删除行数 / Number of deletions */
  deletions: number;
}

/**
 * 文件变更面板属性 / File changes panel props
 */
export interface FileChangesPanelProps {
  /** 面板标题 / Panel title */
  title: string;
  /** 文件变更列表 / File changes list */
  files: FileChangeItem[];
  /** 默认是否展开 / Default expanded state */
  defaultExpanded?: boolean;
  /** 点击预览按钮的回调 / Callback when preview button is clicked */
  onFileClick?: (file: FileChangeItem) => void;
  /** 点击变更统计的回调（+8/-3 数字触发，打开 diff 对比）/ Callback when change stats are clicked (opens diff view) */
  onDiffClick?: (file: FileChangeItem) => void;
  /** 额外的类名 / Additional class name */
  className?: string;
}

const toDiffTableFile = (file: FileChangeItem): DiffTableFile => ({
  id: file.fullPath,
  title: file.file_name,
  insertions: file.insertions,
  deletions: file.deletions,
});

const resolveFileChange = (file: DiffTableFile, files: FileChangeItem[]): FileChangeItem =>
  files.find((item) => item.fullPath === file.id) ?? {
    file_name: file.title,
    fullPath: file.id,
    insertions: file.insertions,
    deletions: file.deletions,
  };

/**
 * 文件变更面板组件
 * File changes panel component
 *
 * Thin adapter over Beautiful UI DiffTable. Callers keep FileChangeItem.
 */
const FileChangesPanel: React.FC<FileChangesPanelProps> = ({
  title,
  files,
  defaultExpanded = true,
  onFileClick,
  onDiffClick,
  className,
}) => {
  if (files.length === 0) {
    return null;
  }

  return (
    <DiffTable
      title={title}
      files={files.map(toDiffTableFile)}
      defaultExpanded={defaultExpanded}
      className={className}
      onFileClick={onFileClick ? (file) => onFileClick(resolveFileChange(file, files)) : undefined}
      onDiffClick={onDiffClick ? (file) => onDiffClick(resolveFileChange(file, files)) : undefined}
    />
  );
};

export default FileChangesPanel;
