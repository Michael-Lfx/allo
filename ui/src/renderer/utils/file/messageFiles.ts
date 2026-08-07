import { NOMIFUN_FILES_MARKER, NOMIFUN_TIMESTAMP_REGEX } from '@/common/config/constants';
import type { FileOrFolderItem } from '@/renderer/utils/file/fileTypes';

export const collectSelectedFiles = (uploadFile: string[], atPath: Array<string | FileOrFolderItem>): string[] => {
  const atPathFiles = atPath.map((item) => (typeof item === 'string' ? item : item.path)).filter(Boolean);
  return Array.from(new Set([...uploadFile, ...atPathFiles]));
};

/**
 * 附件集合差：从当前选择中精确移除本次已提交的附件（按路径匹配）。
 * 路径提取与 collectSelectedFiles 一致（string 即路径，对象取 .path）。
 * 提交 X 飞行中新增 Y → 仅剩 Y；飞行中删 X → 幂等；无新增 → 清空。
 *
 * Attachment set-difference: remove exactly the submitted attachments (matched
 * by path) from the current selection. Path extraction mirrors collectSelectedFiles
 * (a bare string is a path; an object yields .path). Submit X, add Y mid-flight →
 * keep only Y; delete X mid-flight → idempotent; nothing added → emptied.
 */
export const removeSubmittedAttachments = <T extends string | FileOrFolderItem>(
  current: readonly T[],
  submittedAttachmentIds: ReadonlySet<string>
): T[] => current.filter((item) => !submittedAttachmentIds.has(typeof item === 'string' ? item : item.path));

export const buildDisplayMessage = (input: string, files: string[], workspacePath: string): string => {
  if (!files.length) return input;
  const normalizedWorkspace = workspacePath?.replace(/[\\/]+$/, '');
  const displayPaths = files.map((file_path) => {
    const sanitizedPath = file_path.replace(NOMIFUN_TIMESTAMP_REGEX, '$1');
    if (!normalizedWorkspace) {
      return sanitizedPath;
    }

    const isAbsolute = file_path.startsWith('/') || /^[A-Za-z]:/.test(file_path);
    if (isAbsolute) {
      // If file is inside workspace, preserve relative path (including subdirectories like uploads/)
      const normalizedFile = file_path.replace(/\\/g, '/');
      const normalizedWorkspaceWithForwardSlash = normalizedWorkspace.replace(/\\/g, '/');
      if (normalizedFile.startsWith(normalizedWorkspaceWithForwardSlash + '/')) {
        const relativePath = normalizedFile.slice(normalizedWorkspaceWithForwardSlash.length + 1);
        return `${normalizedWorkspace}/${relativePath.replace(NOMIFUN_TIMESTAMP_REGEX, '$1')}`;
      }
      // Keep external absolute paths unchanged so preview and metadata lookups
      // continue to read the real file instead of a non-existent workspace path.
      return sanitizedPath;
    }
    return `${normalizedWorkspace}/${sanitizedPath}`;
  });
  return `${input}\n\n${NOMIFUN_FILES_MARKER}\n${displayPaths.join('\n')}`;
};
