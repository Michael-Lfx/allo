import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@arco-design/web-react';
import { Lightning } from '@icon-park/react';
import { ipcBridge } from '@/common';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import type { IExpertSummary } from '@/common/adapter/ipcBridge';
import SettingsContentLoading from '@/renderer/components/layout/SettingsContentLoading';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';
import { useCapabilityHubSearch } from '../capabilityHub/CapabilityHubShell';
import ExpertCategoryBar from './ExpertCategoryBar';
import ExpertDetailModal from './ExpertDetailModal';
import ExpertTile, { EXPERT_CARD_GRID_COLS } from './ExpertTile';
import {
  EXPERT_PAGE_SIZE,
  expertCategoryParam,
  expertListErrorKind,
  expertListHasMore,
  localizedPair,
  mergeExpertPage,
  localizedTags,
  type ExpertCategoryFilter,
  type ExpertListErrorKind,
} from './expertCatalogModel';

type ExpertCatalogProps = {
  highlightId?: string | null;
  onHighlightConsumed?: () => void;
  onInstalled?: () => void;
  /** False while the catalog is kept mounted but hidden; it then ignores the shared search box. */
  active?: boolean;
};

/**
 * Last first-page result per filter, kept for the renderer's lifetime. Reopening
 * the catalog shows it immediately while the request revalidates in the background.
 */
const FIRST_PAGE_CACHE_LIMIT = 8;
const firstPageCache = new Map<string, IExpertSummary[]>();

const rememberFirstPage = (key: string, items: IExpertSummary[]) => {
  firstPageCache.delete(key);
  firstPageCache.set(key, items);
  if (firstPageCache.size > FIRST_PAGE_CACHE_LIMIT) {
    const oldest = firstPageCache.keys().next().value;
    if (oldest !== undefined) firstPageCache.delete(oldest);
  }
};

const ExpertCard: React.FC<{
  expert: IExpertSummary;
  language: string;
  pending: boolean;
  installed: boolean;
  onOpen: (expert: IExpertSummary) => void;
  onSummon: (expert: IExpertSummary) => void;
}> = ({ expert, language, pending, installed, onOpen, onSummon }) => {
  const { t } = useTranslation();
  const name = localizedPair(language, expert.name, expert.nameEn) || expert.slug;
  const summary = localizedPair(language, expert.summary, expert.summaryEn);
  const tags = localizedTags(language, expert.tags);
  const downloads = expert.downloadCount ?? 0;

  return (
    <ExpertTile
      testId={`expert-card-${expert.slug}`}
      titleTestId={`expert-title-${expert.slug}`}
      summaryTestId={`expert-summary-${expert.slug}`}
      actionTestId={`expert-summon-${expert.slug}`}
      name={name}
      avatarUrl={expert.avatarUrl}
      summary={summary}
      tags={tags}
      actionLabel={t('settings.experts.summon', { defaultValue: 'Summon' })}
      actionPendingLabel={t('settings.experts.summoning', { defaultValue: 'Summoning' })}
      actionDoneLabel={t('settings.experts.summoned', { defaultValue: 'Summoned' })}
      actionPending={pending}
      actionDone={installed}
      onAction={() => onSummon(expert)}
      footer={
        <div
          className='flex h-18px items-center gap-4px text-12px leading-18px text-t-tertiary'
          title={t('settings.experts.downloads', { defaultValue: 'Downloads / uses' })}
        >
          <span className='relative top-px inline-flex'>
            <Lightning theme='outline' size={12} />
          </span>
          <span>{downloads.toLocaleString(language)}</span>
        </div>
      }
      onOpen={() => onOpen(expert)}
    />
  );
};

const ExpertCatalog: React.FC<ExpertCatalogProps> = ({
  highlightId,
  onHighlightConsumed,
  onInstalled,
  active = true,
}) => {
  const { t, i18n } = useTranslation();
  const { searchQuery } = useCapabilityHubSearch();
  const [message, messageContext] = useArcoMessage({ maxCount: 3 });
  const [category, setCategory] = useState<ExpertCategoryFilter>('all');
  const [keyword, setKeyword] = useState('');
  const [items, setItems] = useState<IExpertSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [hasMore, setHasMore] = useState(false);
  const [truncated, setTruncated] = useState(false);
  const [errorKind, setErrorKind] = useState<ExpertListErrorKind | null>(null);
  const [reloadKey, setReloadKey] = useState(0);
  const [selected, setSelected] = useState<IExpertSummary | null>(null);
  const [pendingSlug, setPendingSlug] = useState<string | null>(null);
  const [installedSlugs, setInstalledSlugs] = useState<ReadonlySet<string>>(() => new Set());
  const listSeq = useRef(0);
  const inFlight = useRef(false);
  const pageRef = useRef(1);
  const itemsRef = useRef(items);
  const filterRef = useRef({ keyword, category });
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  itemsRef.current = items;
  filterRef.current = { keyword, category };

  useEffect(() => {
    if (!active) return;
    const timer = window.setTimeout(() => setKeyword(searchQuery.trim()), 300);
    return () => window.clearTimeout(timer);
  }, [active, searchQuery]);

  useEffect(() => {
    if (!highlightId) return;
    onHighlightConsumed?.();
  }, [highlightId, onHighlightConsumed]);

  useEffect(() => {
    let cancelled = false;
    void ipcBridge.cloud.installedExperts
      .invoke()
      .then((result) => {
        if (!cancelled) setInstalledSlugs(new Set(result.slugs ?? []));
      })
      .catch(() => {
        if (!cancelled) setInstalledSlugs(new Set());
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const summonExpert = (expert: IExpertSummary) => {
    const slug = expert.slug?.trim();
    if (!slug || pendingSlug || installedSlugs.has(slug)) return;
    setPendingSlug(slug);
    const name = localizedPair(i18n.language, expert.name, expert.nameEn) || slug;
    void ipcBridge.cloud.installExpert
      .invoke({ slug, id: expert.id?.trim() || undefined })
      .then((result) => {
        setInstalledSlugs((current) => new Set(current).add(slug));
        setItems((current) =>
          current.map((item) =>
            item.slug === slug ? { ...item, downloadCount: (item.downloadCount ?? 0) + 1 } : item
          )
        );
        onInstalled?.();
        if (result.alreadyInstalled) {
          message.info(
            t('settings.experts.alreadyInstalled', {
              name: result.name || name,
              defaultValue: '{{name}} is already summoned',
            })
          );
          return;
        }
        message.success(
          t('settings.experts.installSuccess', {
            name: result.name || name,
            defaultValue: 'Summoned {{name}}',
          })
        );
      })
      .catch((error: unknown) => {
        const detail = isBackendHttpError(error) ? error.backendMessage.trim() : '';
        const notFound = detail.includes('expert_not_found');
        message.error(
          notFound
            ? t('settings.experts.notFound', { defaultValue: 'This expert is not listed or does not exist.' })
            : detail || t('settings.experts.installFailed', { defaultValue: 'Could not summon this expert' })
        );
      })
      .finally(() => setPendingSlug(null));
  };

  const fetchExpertPage = (pageNumber: number, mode: 'replace' | 'append') => {
    if (mode === 'append' && inFlight.current) return;
    const seq = mode === 'replace' ? ++listSeq.current : listSeq.current;
    const filter = filterRef.current;
    const firstPageKey = `${filter.category}\u0000${filter.keyword}`;
    inFlight.current = true;
    setLoading(true);
    if (mode === 'replace') {
      // Seed from the last result for this filter so the grid is not blank
      // while the request revalidates; the response below replaces it.
      const seed = firstPageCache.get(firstPageKey) ?? [];
      pageRef.current = 1;
      setErrorKind(null);
      setItems(seed);
      itemsRef.current = seed;
      setHasMore(false);
      setTruncated(false);
    }
    void ipcBridge.cloud.listExperts
      .invoke({
        page: pageNumber,
        pageSize: EXPERT_PAGE_SIZE,
        keyword: filter.keyword || undefined,
        categoryId: expertCategoryParam(filter.category),
      })
      .then((data) => {
        if (seq !== listSeq.current) return;
        const pageLength = data.list?.length ?? 0;
        const total = typeof data.total === 'number' ? data.total : null;
        let loadedCount = 0;
        setItems((current) => {
          const merged = mergeExpertPage(mode === 'replace' ? [] : current, data.list);
          if (mode === 'replace') rememberFirstPage(firstPageKey, merged);
          itemsRef.current = merged;
          loadedCount = merged.length;
          return merged;
        });
        const more = expertListHasMore(loadedCount, pageLength, total, pageNumber);
        pageRef.current = pageNumber;
        setHasMore(more.hasMore);
        setTruncated(more.truncated);
      })
      .catch((error: unknown) => {
        if (seq !== listSeq.current) return;
        if (mode === 'replace') setErrorKind(expertListErrorKind(error));
        setHasMore(false);
      })
      .finally(() => {
        if (seq !== listSeq.current) return;
        inFlight.current = false;
        setLoading(false);
      });
  };

  useEffect(() => {
    fetchExpertPage(1, 'replace');
    return () => {
      listSeq.current += 1;
      inFlight.current = false;
    };
    // The page loader closes over the latest filter; restarting only on those inputs avoids a request loop.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [category, keyword, reloadKey]);

  useEffect(() => {
    const node = sentinelRef.current;
    if (!node || !hasMore || loading) return;
    // Prefetch the next page before the sentinel is on screen so scrolling never stalls.
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting) || inFlight.current) return;
        fetchExpertPage(pageRef.current + 1, 'append');
      },
      { rootMargin: '400px 0px' }
    );
    observer.observe(node);
    return () => observer.disconnect();
    // Re-arm only after the current page settles, so a visible sentinel cannot overlap the in-flight request.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hasMore, items.length, loading]);

  const errorText =
    errorKind === 'login'
      ? t('settings.experts.loginRequired', { defaultValue: 'Sign in to the cloud account to browse experts.' })
      : errorKind
        ? t('common.failed', { defaultValue: 'Failed' })
        : '';

  return (
    <div className='flex flex-col gap-16px pb-16px' data-testid='expert-catalog'>
      {messageContext}
      <ExpertCategoryBar active={category} onChange={setCategory} />
      {loading && items.length === 0 ? <SettingsContentLoading className='min-h-220px' /> : null}
      {!loading && errorText && items.length === 0 ? (
        <div
          className='flex min-h-220px flex-col items-center justify-center gap-10px rd-8px border border-dashed border-arco-2 px-20px py-32px text-center'
          role='alert'
        >
          <div className='text-13px text-t-secondary'>{errorText}</div>
          <Button type='secondary' size='small' onClick={() => setReloadKey((value) => value + 1)}>
            {t('common.retry', { defaultValue: 'Retry' })}
          </Button>
        </div>
      ) : null}
      {!loading && !errorText && items.length === 0 ? (
        <div className='flex min-h-220px items-center justify-center rd-8px border border-dashed border-arco-2 bg-2 px-20px py-32px text-center text-13px text-t-secondary'>
          {t('settings.experts.empty', { defaultValue: 'No experts match the current search.' })}
        </div>
      ) : null}
      {items.length > 0 ? (
        <div
          className='grid gap-16px [&>*]:[content-visibility:auto] [&>*]:[contain-intrinsic-size:auto_200px]'
          style={{ gridTemplateColumns: EXPERT_CARD_GRID_COLS }}
        >
          {items.map((expert) => (
            <ExpertCard
              key={expert.slug}
              expert={expert}
              language={i18n.language}
              pending={pendingSlug === expert.slug}
              installed={installedSlugs.has(expert.slug)}
              onOpen={setSelected}
              onSummon={summonExpert}
            />
          ))}
        </div>
      ) : null}
      {hasMore ? <div ref={sentinelRef} className='h-1px' aria-hidden='true' /> : null}
      {loading && items.length > 0 ? (
        <div className='text-12px text-t-tertiary'>{t('common.loading', { defaultValue: 'Loading' })}</div>
      ) : null}
      {truncated ? (
        <div className='text-12px text-t-tertiary'>
          {t('settings.experts.truncated', { defaultValue: 'Showing the first part of the catalog.' })}
        </div>
      ) : null}
      <ExpertDetailModal
        expert={selected}
        onClose={() => setSelected(null)}
        pending={Boolean(selected && pendingSlug === selected.slug)}
        installed={Boolean(selected && installedSlugs.has(selected.slug))}
        onSummon={() => {
          if (selected) summonExpert(selected);
        }}
      />
    </div>
  );
};

export default ExpertCatalog;
