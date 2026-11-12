/** Unified native drag/drop adapter and path normalization utilities. */
import { isTauriRuntime } from './tauriRuntime';
import type { PhysicalPoint, Unsubscribe } from './tauriWindows';

export interface TauriFileMetadata { name: string; path: string; size: number; type: string; lastModified: number }
export interface TauriDragDropEvent { type: 'enter' | 'over' | 'drop' | 'leave' | 'cancel'; paths: string[]; position?: PhysicalPoint }

export const getBaseName = (targetPath: string): string => targetPath.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || targetPath;
export const normalizePath = (raw: string): string => {
  let s = raw.trim();
  if ((s.startsWith('"') && s.endsWith('"')) || (s.startsWith("'") && s.endsWith("'"))) s = s.slice(1, -1);
  if (s.toLowerCase().startsWith('file:')) {
    try { const url = new URL(s); let p = decodeURIComponent(url.pathname); p = p.replace(/^\/([A-Za-z]:)/, '$1'); if (p) return p; } catch {}
  }
  return s;
};
const MIME_BY_EXT: Record<string, string> = { png:'image/png', jpg:'image/jpeg', jpeg:'image/jpeg', gif:'image/gif', webp:'image/webp', bmp:'image/bmp', svg:'image/svg+xml', pdf:'application/pdf', txt:'text/plain', md:'text/markdown', json:'application/json', csv:'text/csv' };
export const inferMimeFromExt = (path: string): string => MIME_BY_EXT[path.split('.').pop()?.toLowerCase() ?? ''] ?? '';
export const pathsToFileMetadata = (paths: string[]): TauriFileMetadata[] => {
  const lastModified = Date.now();
  return paths.map((raw) => { const path = normalizePath(raw); return { path, name: getBaseName(path), size: 0, type: inferMimeFromExt(path), lastModified }; });
};

const dropzones = new Set<HTMLElement>();
export const isPhysicalPointOverElement = (x: number, y: number, element: HTMLElement | null | undefined): boolean => {
  if (!element) return false;
  const dpr = typeof window === 'undefined' ? 1 : window.devicePixelRatio || 1;
  return (typeof document !== 'undefined' ? document.elementsFromPoint(x / dpr, y / dpr) : []).some((el) => el === element || element.contains(el));
};
export const registerDropzone = (element: HTMLElement): Unsubscribe => { dropzones.add(element); return () => dropzones.delete(element); };
export const isPhysicalPointOverAnyDropzone = (x: number, y: number): boolean => [...dropzones].some((el) => isPhysicalPointOverElement(x, y, el));

export const tauriDragDrop = {
  async subscribe(listener: (event: TauriDragDropEvent) => void): Promise<Unsubscribe> {
    if (!isTauriRuntime()) return () => {};
    try {
      const { getCurrentWebview } = await import('@tauri-apps/api/webview');
      return await getCurrentWebview().onDragDropEvent((event) => {
        const payload = event.payload as { type?: string; paths?: string[]; position?: PhysicalPoint };
        if (!['enter', 'over', 'drop', 'leave', 'cancel'].includes(payload.type ?? '')) return;
        listener({
          type: payload.type as TauriDragDropEvent['type'],
          paths: (payload.paths ?? []).map(normalizePath),
          position: payload.position,
        });
      });
    } catch {
      return () => {};
    }
  },
  normalizePath, pathsToFileMetadata, registerDropzone,
  isPointOverElement: (position: PhysicalPoint, element: HTMLElement | null) => isPhysicalPointOverElement(position.x, position.y, element),
  isPointOverAnyDropzone: (position: PhysicalPoint) => isPhysicalPointOverAnyDropzone(position.x, position.y),
};
