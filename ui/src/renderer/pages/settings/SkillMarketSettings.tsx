/**
 * SkillMarketSettings — the managed SkillHub market surface. Installation is
 * performed by the backend and never by a shell command or a conversation draft.
 */
import { ipcBridge } from '@/common';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import type { ISkillMarketItem } from '@/common/adapter/ipcBridge';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';
import MarketSettingsPanel from './MarketSettingsPanel';
import { managedInstallErrorMessage, SKILL_MARKET_SOURCES } from './skill/skillMarket';
import { AVAILABLE_SKILLS_SWR_KEY } from './skill/availableSkills';
import React, { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import useSWR, { useSWRConfig } from 'swr';

const CACHE_KEY = 'nomifun.skillMarket.rankings.v5';
const AUTO_SYNC_KEY = 'nomifun.skillMarket.autoSynced.v5';
const INSTALLATIONS_KEY = '/api/skills/market/skill/installations';

type SkillMarketSettingsProps = {
  active?: boolean;
  hideSearch?: boolean;
  searchQuery?: string;
  onSearchQueryChange?: (value: string) => void;
};

const SkillMarketSettings: React.FC<SkillMarketSettingsProps> = ({
  active = true,
  hideSearch = false,
  searchQuery,
  onSearchQueryChange,
}) => {
  const { t } = useTranslation();
  const { mutate } = useSWRConfig();
  const [message, messageContext] = useArcoMessage({ maxCount: 10 });
  const [installationErrorIds, setInstallationErrorIds] = useState<Set<string>>(() => new Set());
  const [locallyInstalledMarketIds, setLocallyInstalledMarketIds] = useState<Set<string>>(() => new Set());
  const {
    data: installations,
    error: installationsError,
    isLoading: installationsLoading,
  } = useSWR(
    active ? INSTALLATIONS_KEY : null,
    () => ipcBridge.fs.listSkillMarketInstallations.invoke()
  );

  const installedMarketIds = useMemo(
    () =>
      new Set([
        ...(installations ?? []).map((installation) => `${installation.source}\u0000${installation.market_id}`),
        ...locallyInstalledMarketIds,
      ]),
    [installations, locallyInstalledMarketIds]
  );

  const handleInstall = useCallback(
    async (item: ISkillMarketItem) => {
      const key = `${item.source}\u0000${item.id}`;
      setInstallationErrorIds((current) => {
        const next = new Set(current);
        next.delete(key);
        return next;
      });
      try {
        await ipcBridge.fs.installSkillMarketSkill.invoke({ source: item.source, id: item.id });
        setLocallyInstalledMarketIds((current) => new Set(current).add(key));
        message.success(t('settings.skillsMarket.installSuccess', { defaultValue: '技能已安装' }));

        try {
          await Promise.all([mutate(INSTALLATIONS_KEY), mutate(AVAILABLE_SKILLS_SWR_KEY)]);
        } catch (refreshError) {
          // The backend commit already succeeded. Keep the completed state and
          // only report that the local catalog refresh needs another attempt.
          console.warn('Skill installed but catalog refresh failed:', refreshError);
          message.warning(
            t('settings.skillsMarket.installRefresh', {
              defaultValue: '技能已安装，但列表刷新失败，请稍后刷新。',
            })
          );
        }
      } catch (error) {
        console.error('Failed to install SkillHub skill:', error);
        setInstallationErrorIds((current) => new Set(current).add(key));
        const code = isBackendHttpError(error) ? error.code : '';
        const installMessage = managedInstallErrorMessage(code);
        message.error(t(installMessage.key, { defaultValue: installMessage.fallback }));
      }
    },
    [message, mutate, t]
  );

  const installationState = useCallback(
    (item: ISkillMarketItem) => {
      const key = `${item.source}\u0000${item.id}`;
      if (installationsLoading && !installations) return 'checking' as const;
      if (installedMarketIds.has(key)) return 'completed' as const;
      if (installationsError && !installations) return 'error' as const;
      if (installationErrorIds.has(key)) return 'error' as const;
      return 'ready' as const;
    },
    [installationErrorIds, installedMarketIds, installations, installationsError, installationsLoading]
  );

  return (
    <div className='w-full pb-16px'>
      {messageContext}
      <div className='space-y-16px'>
        <MarketSettingsPanel
          title={t('settings.skillsMarket.title', { defaultValue: '技能市场' })}
          description={t('settings.skillsMarket.description', {
            defaultValue: '同步 SkillHub 榜单，下载并安全安装技能到 Flowy 的 Skill 目录。',
          })}
          sources={SKILL_MARKET_SOURCES}
          cacheKey={CACHE_KEY}
          autoSyncKey={AUTO_SYNC_KEY}
          defaultSource='skillhub'
          searchPlaceholder={t('settings.skillsMarket.searchPlaceholder', { defaultValue: '搜索当前市场技能...' })}
          emptyText={t('settings.skillsMarket.empty', { defaultValue: '正在准备榜单，点击刷新可重新采集。' })}
          primaryAction={{
            label: t('settings.skillsMarket.install', { defaultValue: '安装' }),
            pendingLabel: t('settings.skillsMarket.installing', { defaultValue: '正在安装' }),
            completedLabel: t('settings.market.installed', { defaultValue: '已安装' }),
            resolveState: installationState,
            run: handleInstall,
          }}
          enableTagFilter
          testIdPrefix='skill-market'
          hideSearch={hideSearch}
          searchQuery={searchQuery}
          onSearchQueryChange={onSearchQueryChange}
          text={{
            syncSuccess: t('settings.skillsMarket.syncSuccess', { defaultValue: '技能市场已更新' }),
            syncKeptCache: t('settings.skillsMarket.syncKeptCache', { defaultValue: '未获取到新榜单，已保留本地缓存。' }),
            syncEmpty: t('settings.skillsMarket.syncEmpty', { defaultValue: '未采集到榜单数据。' }),
            syncError: t('settings.skillsMarket.syncError', { defaultValue: '更新技能市场失败' }),
            openFailed: t('settings.skillsMarket.openMarketFailed', { defaultValue: '无法打开技能市场' }),
            openInBrowser: t('settings.skillsMarket.openInBrowser', { defaultValue: '在浏览器中打开市场' }),
            noSearchMatch: (query, sourceLabel) =>
              t('settings.skillsMarket.noSearchMatch', {
                query,
                source: sourceLabel,
                defaultValue: `当前 ${sourceLabel} 未找到“${query}”相关技能。`,
              }),
            noFilterMatch: t('settings.skillsMarket.noMatch', { defaultValue: '没有符合当前筛选条件的技能。' }),
            lastUpdated: (time) =>
              t('settings.skillsMarket.lastUpdated', { time, defaultValue: '上次更新：{{time}}' }),
          }}
        />
      </div>
    </div>
  );
};

export default SkillMarketSettings;
