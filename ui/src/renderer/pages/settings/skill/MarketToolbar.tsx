import { FlowyIcon } from '@renderer/components/base/FlowyIcon';
import FlowyButton from '@renderer/components/base/FlowyButton';
import FlowySearch from '@renderer/components/base/FlowySearch';
import type { SkillMarketSource } from '@/common/adapter/ipcBridge';
import PresetTagFilterBar from '../PresetSettings/PresetTagFilterBar';
import type { TagFilterState } from '../PresetSettings/presetUtils';
import type { PresetTag } from '@/common/types/agent/presetTypes';
import { Button, Select } from '@arco-design/web-react';
import React, { useId, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { marketSourceLabel } from './skillMarket';

type MarketToolbarProps = {
  title: string;
  description: string;
  sources: SkillMarketSource[];
  activeSource: SkillMarketSource;
  sourceCounts: Partial<Record<SkillMarketSource, number>>;
  onSourceChange: (source: SkillMarketSource) => void;
  loading: boolean;
  onRefresh: () => void;
  searchPlaceholder: string;
  searchQuery: string;
  searchExpanded: boolean;
  onSearchQueryChange: (value: string) => void;
  onSearchExpandedChange: (expanded: boolean) => void;
  isMobile: boolean;
  testId: (id: string) => string | undefined;
  enableTagFilter?: boolean;
  audienceTags?: PresetTag[];
  scenarioTags?: PresetTag[];
  tagFilter?: TagFilterState;
  onTagFilterChange?: (value: TagFilterState) => void;
  localeKey?: string;
  hideSearch?: boolean;
  /** Render the source switcher as a left-aligned tab row, matching the expert category bar. */
  sourcesAsTabs?: boolean;
};

const MarketToolbar: React.FC<MarketToolbarProps> = ({
  title,
  description,
  sources,
  activeSource,
  sourceCounts,
  onSourceChange,
  loading,
  onRefresh,
  searchPlaceholder,
  searchQuery,
  searchExpanded,
  onSearchQueryChange,
  onSearchExpandedChange,
  isMobile,
  testId,
  enableTagFilter = false,
  audienceTags = [],
  scenarioTags = [],
  tagFilter = { audience: [], scenario: [] },
  onTagFilterChange,
  localeKey = 'zh-CN',
  hideSearch = false,
  sourcesAsTabs = false,
}) => {
  const { t } = useTranslation();
  const headingId = useId();
  const rowRef = useRef<HTMLDivElement>(null);
  const headingRef = useRef<HTMLDivElement>(null);
  const controlsRef = useRef<HTMLDivElement>(null);
  const measureRef = useRef<HTMLDivElement>(null);
  const [compactSourcePicker, setCompactSourcePicker] = useState(false);
  const isSearchVisible = searchExpanded || searchQuery.length > 0;

  useLayoutEffect(() => {
    const row = rowRef.current;
    const heading = headingRef.current;
    const controls = controlsRef.current;
    const measure = measureRef.current;
    if (!row || !heading || !controls || !measure || typeof ResizeObserver === 'undefined') return;

    const update = () => {
      const rowWidth = row.getBoundingClientRect().width;
      const headingWidth = isMobile ? 0 : Math.min(heading.getBoundingClientRect().width, 680);
      const fixedControlsWidth = 88; // refresh and search icon buttons
      const sourceWidth = measure.scrollWidth;
      const available = isMobile ? rowWidth : rowWidth - headingWidth - 12 - fixedControlsWidth;
      setCompactSourcePicker(sourceWidth > Math.max(0, available));
    };
    const observer = new ResizeObserver(update);
    observer.observe(row);
    observer.observe(heading);
    observer.observe(controls);
    update();
    return () => observer.disconnect();
  }, [isMobile, sources]);

  return (
    <div className='mb-16px flex flex-col gap-12px pt-4px' aria-labelledby={headingId}>
      {sourcesAsTabs && (
        <div role='tablist' aria-label={title} className='flex max-w-full items-center gap-4px overflow-x-auto'>
          {sources.map((source) => {
            const selected = source === activeSource;
            return (
              <button
                key={source}
                type='button'
                role='tab'
                aria-selected={selected}
                data-testid={testId(`btn-{market}-source-${source}`)}
                onClick={() => {
                  if (!selected) onSourceChange(source);
                }}
                className={[
                  'shrink-0 cursor-pointer rounded-8px border-none px-12px py-6px text-13px leading-20px whitespace-nowrap',
                  'transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgba(var(--primary-6),0.28)]',
                  selected
                    ? 'bg-fill-2 font-medium text-t-primary'
                    : 'bg-transparent text-t-secondary hover:text-t-primary',
                ].join(' ')}
              >
                <span>{marketSourceLabel(source)}</span>
                {(sourceCounts[source] ?? 0) > 0 && <span className='ml-4px text-12px text-t-tertiary'>{sourceCounts[source]}</span>}
              </button>
            );
          })}
        </div>
      )}
      <div ref={rowRef} className={`flex gap-12px ${isMobile ? 'flex-col' : 'items-start justify-between'}`}>
        <div ref={headingRef} className='min-w-0'>
          <h2 id={headingId} className='sr-only'>{title}</h2>
          <p className='m-0 max-w-[680px] text-13px leading-20px text-t-tertiary'>{description}</p>
        </div>
        <div ref={controlsRef} className={`relative flex min-w-0 flex-wrap items-center justify-end gap-10px ${isMobile ? 'w-full' : 'flex-shrink-0'}`}>
          <div ref={measureRef} aria-hidden='true' className='pointer-events-none absolute left-0 top-0 inline-flex invisible items-center gap-4px whitespace-nowrap rounded-12px border border-solid border-border-2 p-3px'>
            {sources.map((source) => <span key={source} className='h-28px px-12px text-12px'>{marketSourceLabel(source)} <small>{sourceCounts[source] ?? 0}</small></span>)}
          </div>
          {!sourcesAsTabs && sources.length > 1 && (compactSourcePicker ? (
            <Select
              size='small'
              className='w-180px'
              value={activeSource}
              onChange={(source) => onSourceChange(source as SkillMarketSource)}
              options={sources.map((source) => ({ value: source, label: `${marketSourceLabel(source)}${(sourceCounts[source] ?? 0) > 0 ? ` · ${sourceCounts[source]}` : ''}` }))}
            />
          ) : (
            <div className='inline-flex max-w-full items-center gap-4px rounded-12px bg-[var(--color-fill-2)] p-3px'>
              {sources.map((source) => (
                <Button key={source} size='small' type='text' data-testid={testId(`btn-{market}-source-${source}`)} className={`!h-28px !rounded-9px !px-12px !text-12px !whitespace-nowrap ${activeSource === source ? '!bg-[var(--color-bg-1)] !font-600 !text-t-primary' : '!text-t-secondary'}`} onClick={() => onSourceChange(source)}>
                  <span>{marketSourceLabel(source)}</span>
                  {(sourceCounts[source] ?? 0) > 0 && <span className='ml-4px text-10px font-normal opacity-65'>{sourceCounts[source]}</span>}
                </Button>
              ))}
            </div>
          ))}
          <FlowyButton role='icon' size='small' data-testid={testId('btn-sync-{market}')} className='!text-t-secondary' icon={<FlowyIcon name='refresh' size={16} className={loading ? 'animate-spin' : ''} />} onClick={onRefresh} title={t('common.refresh', { defaultValue: '刷新' })} />
          {!hideSearch && (
          <FlowyButton role='icon' type={isSearchVisible ? 'secondary' : 'text'} size='small' data-testid={testId('btn-search-{market}')} className='!text-t-secondary' icon={isSearchVisible ? <FlowyIcon name='close' size={16} /> : <FlowyIcon name='search' size={16} />} onClick={() => onSearchExpandedChange(!isSearchVisible)} aria-label={t('common.search', { defaultValue: '搜索' })} aria-pressed={isSearchVisible} />
          )}
        </div>
      </div>
      {!hideSearch && isSearchVisible && <FlowySearch autoFocus value={searchQuery} data-testid={testId('input-search-{market}')} placeholder={searchPlaceholder} onChange={onSearchQueryChange} />}
      {enableTagFilter && onTagFilterChange && <PresetTagFilterBar audienceTags={audienceTags} scenarioTags={scenarioTags} value={tagFilter} onChange={onTagFilterChange} localeKey={localeKey} onManageTags={() => undefined} hideManageTags />}
    </div>
  );
};

export default MarketToolbar;
