const LANGUAGE_DISPLAY_NAMES: Record<string, string> = {
  bash: 'Bash',
  c: 'C',
  cpp: 'C++',
  csharp: 'C#',
  css: 'CSS',
  diff: 'Diff',
  dockerfile: 'Dockerfile',
  go: 'Go',
  gql: 'GraphQL',
  graphql: 'GraphQL',
  html: 'HTML',
  http: 'HTTP',
  ini: 'INI',
  java: 'Java',
  javascript: 'JavaScript',
  js: 'JavaScript',
  json: 'JSON',
  jsx: 'JSX',
  kotlin: 'Kotlin',
  latex: 'LaTeX',
  lua: 'Lua',
  makefile: 'Makefile',
  markdown: 'Markdown',
  nginx: 'Nginx',
  md: 'Markdown',
  php: 'PHP',
  powershell: 'PowerShell',
  proto: 'Protobuf',
  protobuf: 'Protobuf',
  python: 'Python',
  py: 'Python',
  ruby: 'Ruby',
  rust: 'Rust',
  scss: 'SCSS',
  shell: 'Shell',
  sql: 'SQL',
  swift: 'Swift',
  toml: 'TOML',
  text: 'Text',
  ts: 'TypeScript',
  tsx: 'TSX',
  typescript: 'TypeScript',
  vue: 'Vue',
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
