/**
 * Compatibility re-export for the native drag/drop adapter.
 * New code should import from `@/common/adapter/tauriDragDrop`.
 */
export {
  getBaseName,
  normalizePath,
  inferMimeFromExt,
  pathsToFileMetadata,
  isPhysicalPointOverElement,
  registerDropzone,
  isPhysicalPointOverAnyDropzone,
  tauriDragDrop,
} from '@/common/adapter/tauriDragDrop';
export type { TauriDragDropEvent, TauriFileMetadata } from '@/common/adapter/tauriDragDrop';