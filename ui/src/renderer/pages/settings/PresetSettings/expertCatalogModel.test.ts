import { describe, expect, test } from 'bun:test';
import type { IExpertListResponse } from '@/common/adapter/ipcBridge';
import {
  EXPERT_CATEGORIES,
  EXPERT_MAX_PAGES,
  EXPERT_PAGE_SIZE,
  expertCategoryParam,
  expertDetailErrorKind,
  expertListErrorKind,
  expertTypeParam,
  formatZipSize,
  displayableExpertAvatar,
  expertListHasMore,
  loadAllExperts,
  mergeExpertPage,
  localizedPair,
  localizedTags,
  readExpertPeople,
  readPromptText,
} from './expertCatalogModel';

const page = (list: IExpertListResponse['list'], total?: number): IExpertListResponse => ({
  list,
  total,
});

describe('expert catalog model', () => {
  test('picks the active language and falls back to the other name', () => {
    expect(localizedPair('zh-CN', '演示', 'Demo')).toBe('演示');
    expect(localizedPair('en-US', '', 'Demo')).toBe('Demo');
    expect(localizedPair('en-US', '演示', '  ')).toBe('演示');
    expect(localizedTags('zh-CN', [{ zh: '后端', en: 'Backend' }, { zh: '后端', en: 'Backend' }])).toEqual([
      '后端',
    ]);
  });

  test('keeps only an http(s) catalog avatar url', () => {
    expect(displayableExpertAvatar(' https://cdn.example.com/a.png ')).toBe('https://cdn.example.com/a.png');
    expect(displayableExpertAvatar('http://download.flowyaipc.cn/hardware/experts/a.png')).toBe(
      'http://download.flowyaipc.cn/hardware/experts/a.png'
    );
    expect(displayableExpertAvatar('avatars/team.png')).toBeUndefined();
    expect(displayableExpertAvatar('javascript:alert(1)')).toBeUndefined();
    expect(displayableExpertAvatar('')).toBeUndefined();
  });

  test('maps the type filter onto the catalog query', () => {
    expect(expertTypeParam('all')).toBeUndefined();
    expect(expertTypeParam('agent')).toBe('agent');
    expect(expertTypeParam('team')).toBe('team');
  });

  test('keeps the hardcoded category order and omits a category for all', () => {
    expect(EXPERT_CATEGORIES.map((category) => category.id)).toEqual([
      '01-ProductDesign',
      '02-Engineering',
      '03-GameSpatial',
      '04-DataAI',
      '05-MarketingGrowth',
      '06-ContentCreative',
      '07-SalesCommerce',
      '08-FinanceInvestment',
      '09-OperationsHR',
      '10-ProjectQuality',
      '11-SecurityCompliance',
      '12-IndustryConsultant',
      '13-TencentZone',
      '14-GlobalDevelopment',
      '15-BackToSchool',
    ]);
    expect(expertCategoryParam('all')).toBeUndefined();
    expect(expertCategoryParam('02-Engineering')).toBe('02-Engineering');
  });

  test('stops a page when the total is reached and caps a full catalog', () => {
    expect(expertListHasMore(24, 24, 40, 1).hasMore).toBe(true);
    expect(expertListHasMore(24, 24, 24, 1)).toEqual({ hasMore: false, truncated: false });
    expect(expertListHasMore(40, 16, 40, 2)).toEqual({ hasMore: false, truncated: false });
    expect(expertListHasMore(EXPERT_PAGE_SIZE, EXPERT_PAGE_SIZE, 10_000, EXPERT_MAX_PAGES).truncated).toBe(true);
    const merged = mergeExpertPage(
      [{ slug: 'a', name: 'A' }],
      [
        { slug: 'a', name: 'A' },
        { slug: ' b ', name: 'B' },
      ]
    );
    expect(merged.map((item) => item.slug)).toEqual(['a', 'b']);
  });

  test('loads every page until the catalog is exhausted', async () => {
    const calls: number[] = [];
    const result = await loadAllExperts(async (pageNumber) => {
      calls.push(pageNumber);
      if (pageNumber === 1) {
        return page(
          Array.from({ length: EXPERT_PAGE_SIZE }, (_, index) => ({ slug: `expert-${index}`, name: `E${index}` })),
          EXPERT_PAGE_SIZE + 1
        );
      }
      return page(
        [{ slug: `expert-${EXPERT_PAGE_SIZE}`, name: 'Last' }, { slug: 'expert-0', name: 'Dup' }],
        EXPERT_PAGE_SIZE + 1
      );
    });
    expect(calls).toEqual([1, 2]);
    expect(result.truncated).toBe(false);
    expect(result.items).toHaveLength(EXPERT_PAGE_SIZE + 1);
    expect(result.items[0]?.slug).toBe('expert-0');
    expect(result.items.at(-1)?.slug).toBe(`expert-${EXPERT_PAGE_SIZE}`);
  });

  test('stops when a page is short and reports a truncated catalog at the page cap', async () => {
    const short = await loadAllExperts(async () => page([{ slug: 'only', name: 'Only' }]));
    expect(short.items).toHaveLength(1);
    expect(short.truncated).toBe(false);

    const truncated = await loadAllExperts(async (pageNumber) => {
      return page([{ slug: `page-${pageNumber}`, name: 'Row' }].concat(
        Array.from({ length: EXPERT_PAGE_SIZE - 1 }, (_, index) => ({
          slug: `page-${pageNumber}-${index}`,
          name: 'Row',
        }))
      ));
    }, 2);
    expect(truncated.truncated).toBe(true);
    expect(truncated.items).toHaveLength(EXPERT_PAGE_SIZE * 2);
  });

  test('reads prompts and team members from the detail payload', () => {
    expect(readPromptText({ zh: '你好', en: 'Hello' }, 'en-US')).toBe('Hello');
    expect(readPromptText('直接提问', 'zh-CN')).toBe('直接提问');
    const people = readExpertPeople(
      {
        teamInfo: {
          lead: { name: '负责人', nameEn: 'Lead', profession: '架构', professionEn: 'Architect' },
          members: [{ name: '成员', nameEn: 'Member' }],
        },
        members: [{ slug: 'extra' }, { name: '成员', nameEn: 'Member' }],
      },
      'zh-CN'
    );
    expect(people.lead).toEqual(['负责人 · 架构']);
    expect(people.members).toEqual(['成员', 'extra']);
  });

  test('formats package size and classifies catalog errors', () => {
    expect(formatZipSize(0)).toBe('');
    expect(formatZipSize(1536)).toBe('1.5 KB');
    expect(expertListErrorKind({ name: 'BackendHttpError', status: 401, code: 'UNAUTHORIZED' })).toBe('login');
    expect(
      expertDetailErrorKind({
        name: 'BackendHttpError',
        status: 404,
        code: 'NOT_FOUND',
        backendMessage: 'error.expert_not_found',
      })
    ).toBe('notFound');
    expect(expertDetailErrorKind(new Error('offline'))).toBe('failed');
  });
});
