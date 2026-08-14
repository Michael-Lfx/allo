export type ContextCardSourceKind = 'pdf' | 'csv' | 'md' | 'code' | 'other';

export type ContextCardItem = {
  id: string;
  title: string;
  snippet?: string;
  sourceKind: ContextCardSourceKind;
  sourceLabel: string;
  onOpen?: () => void;
};

export const sourceKindFromPath = (path: string): ContextCardSourceKind => {
  const lower = path.toLowerCase();
  if (lower.endsWith('.pdf')) return 'pdf';
  if (lower.endsWith('.csv') || lower.endsWith('.xlsx') || lower.endsWith('.xls')) return 'csv';
  if (lower.endsWith('.md') || lower.endsWith('.markdown')) return 'md';
  if (/\.(ts|tsx|js|jsx|rs|py|go|json|toml)$/.test(lower)) return 'code';
  return 'other';
};
