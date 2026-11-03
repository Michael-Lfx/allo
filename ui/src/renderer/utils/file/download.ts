

import { ipcBridge } from '@/common';
import { base64ToBlob, BINARY_MIME_MAP } from './base64';

export function downloadBlob(blob: Blob, file_name: string): void {
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = file_name;
  link.rel = 'noopener';
  document.body.appendChild(link);
  link.click();
  // Revoking in the same turn cancels the download: Chromium / WebView2 has
  // not started reading the blob yet. Keep the URL until the save finishes.
  window.setTimeout(() => {
    link.remove();
    URL.revokeObjectURL(url);
  }, 60_000);
}

/** Save an in-memory data URL without a second disk read or a data: fetch. */
export function downloadDataUrl(dataUrl: string, file_name: string): void {
  const mimeType = /^data:([^;,]+)/.exec(dataUrl)?.[1] || 'application/octet-stream';
  downloadBlob(base64ToBlob(dataUrl, mimeType), file_name);
}

/**
 * Download a file by reading its raw bytes from disk (works in both Electron and WebUI).
 * Uses getImageBase64 + in-memory atob decode to bypass CSP connect-src restrictions.
 */
export async function downloadFileFromPath(file_path: string, file_name: string, workspace?: string): Promise<void> {
  const dataUrl = await ipcBridge.fs.getImageBase64.invoke({ path: file_path, workspace });
  if (!dataUrl) {
    throw new Error('File data not found');
  }
  const ext = file_name.split('.').pop()?.toLowerCase() ?? '';
  const mimeType = BINARY_MIME_MAP[ext] ?? 'application/octet-stream';
  const blob = base64ToBlob(dataUrl, mimeType);
  downloadBlob(blob, file_name);
}

/**
 * Download in-memory text content as a file.
 */
export function downloadTextContent(content: string, file_name: string, mimeType: string): void {
  const blob = new Blob([content], { type: mimeType });
  downloadBlob(blob, file_name);
}
