import { isBackendHttpError } from '@/common/adapter/httpBridge';
import type { IExpertListResponse, IExpertSummary, IExpertText } from '@/common/adapter/ipcBridge';
import type { I18nKey } from '@/renderer/services/i18n/i18n-keys';

export const EXPERT_PAGE_SIZE = 100;
export const EXPERT_MAX_PAGES = 30;

/** Catalog categories. Ids match Flowy `categoryId`; labels live in i18n. */
export const EXPERT_CATEGORIES = [
  { id: '01-ProductDesign', labelKey: 'settings.experts.categories.productDesign' },
  { id: '02-Engineering', labelKey: 'settings.experts.categories.engineering' },
  { id: '03-GameSpatial', labelKey: 'settings.experts.categories.gameSpatial' },
  { id: '04-DataAI', labelKey: 'settings.experts.categories.dataAi' },
  { id: '05-MarketingGrowth', labelKey: 'settings.experts.categories.marketingGrowth' },
  { id: '06-ContentCreative', labelKey: 'settings.experts.categories.contentCreative' },
  { id: '07-SalesCommerce', labelKey: 'settings.experts.categories.salesCommerce' },
  { id: '08-FinanceInvestment', labelKey: 'settings.experts.categories.financeInvestment' },
  { id: '09-OperationsHR', labelKey: 'settings.experts.categories.operationsHr' },
  { id: '10-ProjectQuality', labelKey: 'settings.experts.categories.projectQuality' },
  { id: '11-SecurityCompliance', labelKey: 'settings.experts.categories.securityCompliance' },
  { id: '12-IndustryConsultant', labelKey: 'settings.experts.categories.industryConsultant' },
  { id: '13-TencentZone', labelKey: 'settings.experts.categories.tencentZone' },
  { id: '14-GlobalDevelopment', labelKey: 'settings.experts.categories.globalDevelopment' },
  { id: '15-BackToSchool', labelKey: 'settings.experts.categories.backToSchool' },
] as const satisfies readonly { id: string; labelKey: I18nKey }[];

export type ExpertCategoryId = (typeof EXPERT_CATEGORIES)[number]['id'];
export type ExpertCategoryFilter = 'all' | ExpertCategoryId;

export const expertCategoryParam = (filter: ExpertCategoryFilter): ExpertCategoryId | undefined => {
  if (filter === 'all') return undefined;
  return filter;
};

export type ExpertKindFilter = 'all' | 'agent' | 'team';
export type ExpertListErrorKind = 'login' | 'failed';
export type ExpertDetailErrorKind = 'login' | 'notFound' | 'failed';

export type ExpertPeople = {
  lead: string[];
  members: string[];
};

const prefersChinese = (language: string): boolean => language.toLowerCase().startsWith('zh');

export const localizedPair = (
  language: string,
  zh?: string | null,
  en?: string | null
): string => {
  const primary = (prefersChinese(language) ? zh : en)?.trim() ?? '';
  const secondary = (prefersChinese(language) ? en : zh)?.trim() ?? '';
  return primary || secondary;
};

export const localizedTags = (language: string, tags?: IExpertText[] | null): string[] => {
  const labels = (tags ?? [])
    .map((tag) => localizedPair(language, tag?.zh, tag?.en))
    .filter((label) => label.length > 0);
  return [...new Set(labels)];
};

export const expertTypeParam = (filter: ExpertKindFilter): 'agent' | 'team' | undefined => {
  switch (filter) {
    case 'all':
      return undefined;
    case 'agent':
      return 'agent';
    case 'team':
      return 'team';
    default: {
      const unreachable: never = filter;
      return unreachable;
    }
  }
};

export const formatZipSize = (bytes?: number | null): string => {
  if (bytes == null || !Number.isFinite(bytes) || bytes <= 0) return '';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(bytes < 10 * 1024 ? 1 : 0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(bytes < 10 * 1024 * 1024 ? 1 : 0)} MB`;
};

const asRecord = (value: unknown): Record<string, unknown> | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  return value as Record<string, unknown>;
};

const readString = (record: Record<string, unknown>, key: string): string => {
  const value = record[key];
  return typeof value === 'string' ? value : '';
};

export const readPromptText = (value: unknown, language: string): string => {
  if (typeof value === 'string') return value.trim();
  const record = asRecord(value);
  if (!record) return '';
  const localized = localizedPair(language, readString(record, 'zh'), readString(record, 'en'));
  if (localized) return localized;
  for (const key of ['prompt', 'text', 'title', 'name', 'content']) {
    const field = readString(record, key).trim();
    if (field) return field;
  }
  return '';
};

const readMemberLabel = (value: unknown, language: string): string => {
  if (typeof value === 'string') return value.trim();
  const record = asRecord(value);
  if (!record) return '';
  const name = localizedPair(
    language,
    readString(record, 'name') || readString(record, 'zh'),
    readString(record, 'nameEn') || readString(record, 'en')
  );
  const role = localizedPair(
    language,
    readString(record, 'role') || readString(record, 'profession'),
    readString(record, 'roleEn') || readString(record, 'professionEn')
  );
  if (name && role) return `${name} · ${role}`;
  return name || role || readString(record, 'slug').trim();
};

export const readExpertPeople = (
  detail: { teamInfo?: unknown; members?: unknown[] | null },
  language: string
): ExpertPeople => {
  const lead: string[] = [];
  const members: string[] = [];
  const team = asRecord(detail.teamInfo);
  if (team) {
    const leadLabel = readMemberLabel(team.lead, language);
    if (leadLabel) lead.push(leadLabel);
    const teamMembers = Array.isArray(team.members) ? team.members : [];
    for (const member of teamMembers) {
      const label = readMemberLabel(member, language);
      if (label) members.push(label);
    }
  }
  for (const member of detail.members ?? []) {
    const label = readMemberLabel(member, language);
    if (label && !lead.includes(label) && !members.includes(label)) members.push(label);
  }
  return { lead, members };
};

export const expertListErrorKind = (error: unknown): ExpertListErrorKind => {
  if (!isBackendHttpError(error)) return 'failed';
  if (error.status === 401 || error.status === 403 || error.code === 'UNAUTHORIZED') return 'login';
  return 'failed';
};

export const expertDetailErrorKind = (error: unknown): ExpertDetailErrorKind => {
  if (!isBackendHttpError(error)) return 'failed';
  const message = `${error.code} ${error.backendMessage}`.toLowerCase();
  if (error.status === 404 || message.includes('expert_not_found')) return 'notFound';
  if (error.status === 401 || error.status === 403 || error.code === 'UNAUTHORIZED') return 'login';
  return 'failed';
};

export async function loadAllExperts(
  fetchPage: (page: number) => Promise<IExpertListResponse>,
  maxPages = EXPERT_MAX_PAGES
): Promise<{ items: IExpertSummary[]; truncated: boolean }> {
  const items: IExpertSummary[] = [];
  const seen = new Set<string>();
  for (let page = 1; page <= maxPages; page += 1) {
    const data = await fetchPage(page);
    const list = data.list ?? [];
    for (const item of list) {
      const slug = item.slug?.trim();
      if (!slug || seen.has(slug)) continue;
      seen.add(slug);
      items.push({ ...item, slug });
    }
    const total = typeof data.total === 'number' ? data.total : null;
    if (list.length < EXPERT_PAGE_SIZE || (total != null && seen.size >= total)) {
      return { items, truncated: false };
    }
    if (page === maxPages) return { items, truncated: true };
  }
  return { items, truncated: false };
}
