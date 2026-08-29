
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Result, Spin } from '@arco-design/web-react';
import { Trophy } from '@icon-park/react';
import SegmentedTabs, { type SegmentedTabItem } from '@renderer/components/base/SegmentedTabs';
import { listCampaigns } from '../api';
import type { CampaignSummary } from '../types';
import CampaignCard from './CampaignCard';

const PAGE_SIZE = 20;

type CampaignRange = 'current' | 'ended';

const CampaignPanel: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const [range, setRange] = useState<CampaignRange>('current');
  const [campaigns, setCampaigns] = useState<CampaignSummary[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const campaignsRef = useRef(campaigns);
  campaignsRef.current = campaigns;

  const rangeItems: SegmentedTabItem[] = [
    {
      key: 'current',
      label: t('videoGeneration.campaign.range.current', { defaultValue: '进行中' }),
    },
    {
      key: 'ended',
      label: t('videoGeneration.campaign.range.ended', { defaultValue: '往期' }),
    },
  ];

  const refresh = useCallback(async () => {
    const showSpinner = campaignsRef.current.length === 0;
    if (showSpinner) setLoading(true);
    try {
      const data = await listCampaigns({
        page: 1,
        pageSize: PAGE_SIZE,
        includeEnded: range === 'ended',
      });
      setCampaigns(data.list ?? []);
      setTotal(data.total ?? 0);
      setError(null);
    } catch (e) {
      console.error('[videoGeneration] campaign list failed', e);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [range]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const loadMore = useCallback(async () => {
    if (loadingMore || campaigns.length >= total) return;
    setLoadingMore(true);
    try {
      const nextPage = Math.floor(campaigns.length / PAGE_SIZE) + 1;
      const data = await listCampaigns({
        page: nextPage,
        pageSize: PAGE_SIZE,
        includeEnded: range === 'ended',
      });
      setCampaigns((prev) => [...prev, ...(data.list ?? [])]);
      setTotal(data.total ?? 0);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoadingMore(false);
    }
  }, [campaigns.length, loadingMore, range, total]);

  const openCampaign = useCallback(
    (campaign: CampaignSummary) => {
      navigate(`/video-generation/campaigns/${campaign.id}`, {
        state: { fromSearch: location.search },
      });
    },
    [location.search, navigate]
  );

  return (
    <div className='flex flex-col gap-12px'>
      <SegmentedTabs
        size='sm'
        items={rangeItems}
        activeKey={range}
        onChange={(key) => {
          setCampaigns([]);
          setRange(key as CampaignRange);
        }}
      />

      {error ? (
        <Result
          status='error'
          title={t('videoGeneration.list.loadError', { defaultValue: '加载失败' })}
          subTitle={error}
          extra={
            <Button onClick={() => void refresh()}>
              {t('videoGeneration.list.retry', { defaultValue: '重试' })}
            </Button>
          }
        />
      ) : loading ? (
        <div className='flex justify-center py-38px'>
          <Spin />
        </div>
      ) : campaigns.length === 0 ? (
        <div className='flex items-center gap-12px rd-14px border border-dashed border-[var(--color-border-2)] bg-[var(--color-fill-1)] px-16px py-18px'>
          <span className='flex h-38px w-38px shrink-0 items-center justify-center rd-11px bg-[rgba(var(--primary-6),0.1)] text-[rgb(var(--primary-6))]'>
            <Trophy theme='outline' size={19} fill='currentColor' />
          </span>
          <div>
            <div className='text-13px font-600 text-[var(--color-text-1)]'>
              {t('videoGeneration.campaign.empty.title', { defaultValue: '暂无活动' })}
            </div>
            <div className='mt-2px text-12px text-[var(--color-text-3)]'>
              {range === 'ended'
                ? t('videoGeneration.campaign.empty.endedDesc', {
                    defaultValue: '还没有往期活动。',
                  })
                : t('videoGeneration.campaign.empty.desc', {
                    defaultValue: '新活动上架后会出现在这里。',
                  })}
            </div>
          </div>
        </div>
      ) : (
        <>
          <div
            className='grid gap-12px'
            style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(min(300px, 100%), 1fr))' }}
          >
            {campaigns.map((campaign) => (
              <CampaignCard key={campaign.id} campaign={campaign} onOpen={openCampaign} />
            ))}
          </div>
          {campaigns.length < total ? (
            <div className='flex justify-center pt-4px'>
              <Button size='small' loading={loadingMore} onClick={() => void loadMore()}>
                {t('videoGeneration.campaign.loadMore', { defaultValue: '加载更多' })}
              </Button>
            </div>
          ) : null}
        </>
      )}
    </div>
  );
};

export default CampaignPanel;
