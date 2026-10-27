import { Button, Select } from '@arco-design/web-react';
import { Refresh } from '@icon-park/react';
import React from 'react';
import type { SkillHubMarketSort } from '@/common/adapter/ipcBridge';
import FilterPills from '@/renderer/pages/videoGeneration/components/FilterPills';
import type { SkillHubMarketSourceFilter } from './useSkillHubMarket';

type SkillHubMarketToolbarProps = {
  source: SkillHubMarketSourceFilter;
  onSourceChange: (value: SkillHubMarketSourceFilter) => void;
  category?: string;
  categoryOptions: Array<{ value: string; label: string }>;
  onCategoryChange: (value?: string) => void;
  requiresApiKey?: boolean;
  onRequiresApiKeyChange: (value?: boolean) => void;
  sortBy: SkillHubMarketSort;
  onSortChange: (value: SkillHubMarketSort) => void;
  viewMode: 'list' | 'grid';
  onViewModeChange: (value: 'list' | 'grid') => void;
  resultCount?: number;
  refreshing?: boolean;
  onRefresh?: () => void;
  categoriesLoading?: boolean;
  categoriesError?: boolean;
  onRetryCategories?: () => void;
  t: (key: string, options?: Record<string, unknown>) => string;
};

const ALL_CATEGORY = '__all__';
const ALL_API_KEY = '__all__';

const asString = (value: string | number | undefined): string => String(value ?? '');

const SkillHubMarketToolbar: React.FC<SkillHubMarketToolbarProps> = ({
  source,
  onSourceChange,
  category,
  categoryOptions,
  onCategoryChange,
  requiresApiKey,
  onRequiresApiKeyChange,
  sortBy,
  onSortChange,
  viewMode,
  onViewModeChange,
  resultCount = 0,
  refreshing = false,
  onRefresh,
  categoriesLoading = false,
  categoriesError = false,
  onRetryCategories,
  t,
}) => {
  const selectClass = '!w-auto min-w-128px max-w-full flex-[0_1_auto]';
  const sortOptions: Array<{ value: SkillHubMarketSort; label: string }> = [
    { value: 'score', label: t('settings.skillsMarket.sortPopular', { defaultValue: '热门' }) },
    { value: 'downloads', label: t('settings.skillsMarket.sortDownloads', { defaultValue: '下载量' }) },
    { value: 'updated_at', label: t('settings.skillsMarket.sortUpdated', { defaultValue: '最近更新' }) },
  ];

  return (
    <div
      className='sticky top-0 z-10 mb-16px flex flex-wrap items-center gap-8px border-b border-b-solid border-[var(--color-border-2)] bg-[var(--color-bg-1)] py-10px'
      data-testid='skillhub-market-toolbar'
    >
      <FilterPills
        ariaLabel={t('settings.skillsMarket.sort', { defaultValue: '排序' })}
        active={sortBy}
        onChange={onSortChange}
        items={sortOptions.map((option) => ({ key: option.value, label: option.label }))}
      />

      {resultCount > 0 && (
        <span className='text-12px text-t-tertiary'>
          {t('settings.market.skillsCount', { count: resultCount, defaultValue: '{{count}} 个技能' })}
        </span>
      )}

      <div className='ml-auto flex min-w-0 flex-wrap items-center justify-end gap-8px'>
        <Select
          size='small'
          className={selectClass}
          style={{ width: 'auto' }}
          value={source}
          aria-label={t('settings.skillsMarket.contentSource', { defaultValue: '内容来源' })}
          onChange={(value) => onSourceChange(asString(value) as SkillHubMarketSourceFilter)}
        >
          <Select.Option value='all'>{t('settings.skillsMarket.allSources', { defaultValue: '全部来源' })}</Select.Option>
          <Select.Option value='skillhub'>SkillHub</Select.Option>
          <Select.Option value='clawhub'>ClawHub</Select.Option>
        </Select>

        <Select
          size='small'
          className={selectClass}
          style={{ width: 'auto' }}
          value={category ?? ALL_CATEGORY}
          loading={categoriesLoading}
          aria-label={t('settings.skillsMarket.category', { defaultValue: '场景分类' })}
          onChange={(value) => {
            const next = asString(value);
            onCategoryChange(next === ALL_CATEGORY ? undefined : next);
          }}
        >
          <Select.Option value={ALL_CATEGORY}>{t('settings.skillsMarket.allCategories', { defaultValue: '全部分类' })}</Select.Option>
          {categoryOptions.map((option) => (
            <Select.Option key={option.value} value={option.value}>{option.label}</Select.Option>
          ))}
        </Select>

        <Select
          size='small'
          className={selectClass}
          style={{ width: 'auto' }}
          value={requiresApiKey === undefined ? ALL_API_KEY : String(requiresApiKey)}
          aria-label={t('settings.skillsMarket.apiKeyFilter', { defaultValue: 'API Key 条件' })}
          onChange={(value) => {
            const next = asString(value);
            onRequiresApiKeyChange(next === ALL_API_KEY ? undefined : next === 'true');
          }}
        >
          <Select.Option value={ALL_API_KEY}>{t('settings.skillsMarket.allApiKey', { defaultValue: '不限 API Key' })}</Select.Option>
          <Select.Option value='true'>{t('settings.skillsMarket.requiresApi', { defaultValue: '需要 API Key' })}</Select.Option>
          <Select.Option value='false'>{t('settings.skillsMarket.noApi', { defaultValue: '无需 API Key' })}</Select.Option>
        </Select>

        <FilterPills
          ariaLabel={t('settings.skillsMarket.viewMode', { defaultValue: '展示方式' })}
          active={viewMode}
          onChange={onViewModeChange}
          items={[
            { key: 'grid', label: t('settings.skillsMarket.cardView', { defaultValue: '卡片' }) },
            { key: 'list', label: t('settings.skillsMarket.listView', { defaultValue: '列表' }) },
          ]}
        />

        {onRefresh && (
          <Button
            type='text'
            size='small'
            data-testid='btn-sync-skill-market'
            className='!h-32px !w-32px !rounded-10px !p-0 !text-t-secondary hover:!bg-fill-2 hover:!text-t-primary'
            icon={<Refresh size={16} fill='currentColor' className={refreshing ? 'animate-spin' : ''} />}
            onClick={onRefresh}
            title={t('common.refresh', { defaultValue: '刷新' })}
          />
        )}

        {categoriesError && onRetryCategories && (
          <Button type='text' size='small' onClick={onRetryCategories}>
            {t('common.retry', { defaultValue: '重试分类' })}
          </Button>
        )}
      </div>
    </div>
  );
};

export default SkillHubMarketToolbar;
