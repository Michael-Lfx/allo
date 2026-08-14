const LANGUAGE_DISPLAY_NAMES: Record<string, string> = {
  bash: 'Bash',
  c: 'C',
  cpp: 'C++',
  csharp: 'C#',
  css: 'CSS',
  diff: 'Diff',
  dockerfile: 'Dockerfile',
  go: 'Go',
  html: 'HTML',
  ini: 'INI',
  java: 'Java',
  javascript: 'JavaScript',
  js: 'JavaScript',
  json: 'JSON',
  jsx: 'JavaScript',
  kotlin: 'Kotlin',
  latex: 'LaTeX',
  lua: 'Lua',
  makefile: 'Makefile',
  markdown: 'Markdown',
  md: 'Markdown',
  php: 'PHP',
  powershell: 'PowerShell',
  python: 'Python',
  py: 'Python',
  ruby: 'Ruby',
  rust: 'Rust',
  scss: 'SCSS',
  shell: 'Shell',
  sql: 'SQL',
  swift: 'Swift',
  text: 'Text',
  ts: 'TypeScript',
  tsx: 'TypeScript',
  typescript: 'TypeScript',
  xml: 'XML',
  yaml: 'YAML',
  yml: 'YAML',
};

export const displayNameForCodeLanguage = (language?: string): string => {
  const trimmed = language?.trim();
  if (!trimmed) return '';
  const mapped = LANGUAGE_DISPLAY_NAMES[trimmed.toLowerCase()];
  if (mapped) return mapped;
  return `${trimmed.charAt(0).toUpperCase()}${trimmed.slice(1)}`;
};

export const filenameFromFenceNode = (node: unknown): string | undefined => {
  if (!node || typeof node !== 'object') return undefined;
  const record = node as {
    properties?: { meta?: unknown };
    data?: { meta?: unknown };
  };
  const metaCandidates = [record.data?.meta, record.properties?.meta];
  for (const meta of metaCandidates) {
    if (typeof meta !== 'string' || !meta.trim()) continue;
    const assigned = meta.match(/(?:file(?:name)?)\s*=\s*["']?([^\s"']+)/i);
    if (assigned?.[1]) return assigned[1];
    const dotted = meta
      .trim()
      .split(/\s+/)
      .find((token) => token.includes('.') && !token.startsWith('{'));
    if (dotted) return dotted.replace(/^["']|["']$/g, '');
  }
  return undefined;
};
