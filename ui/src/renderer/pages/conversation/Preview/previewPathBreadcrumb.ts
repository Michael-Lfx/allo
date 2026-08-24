/**
 * Workspace-relative path segments for the preview breadcrumb
 * (Cursor-style `.config > nextest.toml`), not the leaf filename alone.
 */
export const previewPathSegments = (filePath?: string, workspace?: string): string[] => {
  if (!filePath?.trim()) return [];

  const file = filePath.replace(/\\/g, '/').replace(/\/+$/, '');
  const ws = workspace?.replace(/\\/g, '/').replace(/\/+$/, '') ?? '';
  const fileLower = file.toLowerCase();
  const wsLower = ws.toLowerCase();
  const isAbsolute = file.startsWith('/') || /^[A-Za-z]:\//.test(file);

  let relative = file;
  let insideWorkspace = false;
  if (ws) {
    if (fileLower === wsLower) return [];
    if (fileLower.startsWith(`${wsLower}/`)) {
      relative = file.slice(ws.length + 1);
      insideWorkspace = true;
    }
  }

  const segments = relative.split('/').filter((part) => part.length > 0 && !/^[A-Za-z]:$/.test(part));
  if (insideWorkspace || !isAbsolute) return segments;
  if (segments.length <= 2) return segments;
  return segments.slice(-2);
};
