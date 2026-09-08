import { Button, Select } from '@arco-design/web-react';
import React from 'react';
import type { SkillHubMarketSort } from '@/common/adapter/ipcBridge';
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
  categoriesLoading = false,
  categoriesError = false,
  onRetryCategories,
  t,
}) => {
  const selectClass = '!w-auto min-w-136px max-w-full flex-[0_1_auto]';
  const sortOptions: Array<{ value: SkillHubMarketSort; label: string }> = [
    { value: 'score', label: t('settings.skillsMarket.sortPopular', { defaultValue: '热门' }) },
    { value: 'downloads', label: t('settings.skillsMarket.sortDownloads', { defaultValue: '下载量' }) },
    { value: 'updated_at', label: t('settings.skillsMarket.sortUpdated', { defaultValue: '最近更新' }) },
  ];

  return (
    <div
      className='sticky top-0 z-10 -mx-4px border-b border-solid border-[var(--color-border-2)] bg-[var(--color-bg-1)] px-4px py-10px'
      data-testid='skillhub-market-toolbar'
    >
      <div className='flex flex-wrap items-center gap-8px' role='toolbar' aria-label={t('settings.skillsMarket.filters', { defaultValue: '市场筛选' })}>
        <div className='flex shrink-0 flex-wrap items-center gap-4px' role='tablist' aria-label={t('settings.skillsMarket.sort', { defaultValue: '排序' })}>
          {sortOptions.map((option) => (
            <Button
              key={option.value}
              type={sortBy === option.value ? 'secondary' : 'text'}
              size='small'
              className='!h-30px !rounded-8px !px-12px !text-12px'
              role='tab'
              aria-selected={sortBy === option.value}
              onClick={() => onSortChange(option.value)}
            >
              {option.label}
            </Button>
          ))}
        </div>

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

        <div className='ml-auto flex shrink-0 items-center rounded-8px border border-solid border-[var(--color-border-2)] p-2px' role='group' aria-label={t('settings.skillsMarket.viewMode', { defaultValue: '展示方式' })}>
          {(['list', 'grid'] as const).map((mode) => (
            <Button
              key={mode}
              type={viewMode === mode ? 'secondary' : 'text'}
              size='small'
              className='!h-28px !rounded-6px !px-10px !text-12px'
              aria-pressed={viewMode === mode}
              onClick={() => onViewModeChange(mode)}
            >
              {mode === 'list'
                ? t('settings.skillsMarket.listView', { defaultValue: '列表' })
                : t('settings.skillsMarket.cardView', { defaultValue: '卡片' })}
            </Button>
          ))}
        </div>

        {categoriesError && onRetryCategories && (
          <Button type='text' size='small' onClick={onRetryCategories}>
            {t('common.retry', { defaultValue: '重试分类' })}
          </Button>
        )}
        </div>
      </div>
    </div>
  );
};

export default SkillHubMarketToolbar;
