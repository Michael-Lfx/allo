/**
 * Canvas mode project list (DEV).
 */

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Input, Result, Spin } from '@arco-design/web-react';
import { Delete, Search } from '@icon-park/react';
import SegmentedTabs, { type SegmentedTabItem } from '@renderer/components/base/SegmentedTabs';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import {
  deleteCanvasProject,
  listCanvasProjects,
  type CanvasProjectMeta,
} from './api';
import { createServerBackedCanvasProject } from './lib/ocBridge';
import styles from './index.module.css';

function CreateCanvasButton({
  loading,
  onClick,
  children,
  className,
}: {
  loading?: boolean;
  onClick: () => void;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <button
      type='button'
      disabled={loading}
      onClick={onClick}
      className={[
        styles.createButton,
        loading ? styles.createButtonLoading : '',
        className || '',
      ]
        .filter(Boolean)
        .join(' ')}
    >
      <span className={styles.createButtonPlus} aria-hidden='true'>
        {loading ? '…' : '＋'}
      </span>
      <span>{children}</span>
    </button>
  );
}
function formatTime(ms: number): string {
  try {
    return new Date(ms).toLocaleString();
  } catch {
    return String(ms);
  }
}

const VideoCanvasListPage: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [message, messageHolder] = useArcoMessage();
  const [items, setItems] = useState<CanvasProjectMeta[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [creating, setCreating] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);

  const modeItems: SegmentedTabItem[] = useMemo(
    () => [
      {
        key: 'agent',
        label: t('videoGeneration.mode.agent', { defaultValue: 'Agent' }),
      },
      {
        key: 'canvas',
        label: (
          <span className='inline-flex items-center gap-6px'>
            {t('videoGeneration.mode.canvas', { defaultValue: 'Canvas' })}
            <span
              className='text-9px font-600 leading-none tracking-wide uppercase px-4px py-2px rd-4px bg-[rgba(var(--primary-6),0.12)] text-[rgb(var(--primary-6))]'
              aria-hidden='true'
            >
              {t('videoCanvas.dev.tag', { defaultValue: 'dev' })}
            </span>
          </span>
        ),
      },
    ],
    [t]
  );

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setItems(await listCanvasProjects());
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const displayed = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter((p) => p.title.toLowerCase().includes(q) || p.project_id.includes(q));
  }, [items, query]);

  const handleCreate = useCallback(async () => {
    if (creating) return;
    setCreating(true);
    try {
      const id = await createServerBackedCanvasProject(
        t('videoCanvas.list.untitled', { defaultValue: '未命名画布' })
      );
      navigate(`/video-generation/canvas/${id}`);
    } catch (e) {
      message.error(
        `${t('videoCanvas.actions.createFailed', { defaultValue: '创建失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setCreating(false);
    }
  }, [creating, message, navigate, t]);

  const handleDelete = useCallback(
    async (p: CanvasProjectMeta) => {
      if (deletingId) return;
      setDeletingId(p.project_id);
      try {
        await deleteCanvasProject(p.project_id);
        setItems((prev) => prev.filter((x) => x.project_id !== p.project_id));
        message.success(t('videoCanvas.actions.deleteOk', { defaultValue: '已删除' }));
      } catch (e) {
        message.error(
          `${t('videoCanvas.actions.deleteFailed', { defaultValue: '删除失败' })}: ${
            e instanceof Error ? e.message : String(e)
          }`
        );
      } finally {
        setDeletingId(null);
      }
    },
    [deletingId, message, t]
  );

  return (
    <div className={`${styles.listPage} flex-1 min-h-0 size-full box-border overflow-y-auto px-16px py-24px md:px-36px md:py-32px`}>
      {messageHolder}
      <div className={styles.listHeader}>
        <div>
          <div className={styles.listTitleRow}>
            <h2 className={styles.listTitle}>
              {t('videoGeneration.title', { defaultValue: '视频生成' })}
            </h2>
            <span className={styles.devBadge} aria-hidden='true'>
              {t('videoCanvas.dev.tag', { defaultValue: 'dev' })}
            </span>
          </div>
          <p className={styles.listSubtitle}>
            {t('videoCanvas.list.subtitle', {
              defaultValue: '基于节点的无限画布：手动编排文/图/视频生成链路。',
            })}
          </p>
        </div>
        <div className='flex flex-wrap items-center gap-12px'>
          <SegmentedTabs
            size='sm'
            items={modeItems}
            activeKey='canvas'
            onChange={(key) => {
              if (key === 'agent') navigate('/video-generation');
            }}
          />
          <CreateCanvasButton loading={creating} onClick={() => void handleCreate()}>
            {t('videoCanvas.list.create', { defaultValue: '新建画布' })}
          </CreateCanvasButton>
        </div>
      </div>

      <div className={styles.listToolbar}>
        <Input
          allowClear
          prefix={<Search theme='outline' size={14} />}
          placeholder={t('videoCanvas.list.searchPlaceholder', {
            defaultValue: '搜索画布…',
          })}
          value={query}
          onChange={setQuery}
          style={{ maxWidth: 320 }}
        />
      </div>

      {loading ? (
        <div className={styles.center}>
          <Spin />
        </div>
      ) : error ? (
        <Result
          status='error'
          title={t('videoCanvas.list.loadError', { defaultValue: '加载失败' })}
          subTitle={error}
          extra={
            <Button type='primary' onClick={() => void refresh()}>
              {t('videoCanvas.list.retry', { defaultValue: '重试' })}
            </Button>
          }
        />
      ) : displayed.length === 0 ? (
        <div className={styles.empty}>
          <p>
            {t('videoCanvas.list.empty', {
              defaultValue: '还没有画布项目。创建一个开始节点编排。',
            })}
          </p>
          <CreateCanvasButton loading={creating} onClick={() => void handleCreate()}>
            {t('videoCanvas.list.createFirst', { defaultValue: '创建第一个画布' })}
          </CreateCanvasButton>
        </div>
      ) : (
        <div className={styles.grid}>
          {displayed.map((p) => (
            <button
              key={p.project_id}
              type='button'
              className={styles.card}
              onClick={() => navigate(`/video-generation/canvas/${p.project_id}`)}
            >
              <div className={styles.cardTop}>
                <span className={styles.cardTitle}>{p.title || '未命名画布'}</span>
                <Button
                  size='mini'
                  type='text'
                  status='danger'
                  icon={<Delete theme='outline' size={14} />}
                  loading={deletingId === p.project_id}
                  onClick={(e) => {
                    e.stopPropagation();
                    void handleDelete(p);
                  }}
                />
              </div>
              <div className={styles.cardMeta}>
                <span>
                  {t('videoCanvas.list.nodes', {
                    defaultValue: '{{count}} 个节点',
                    count: p.node_count,
                  })}
                </span>
                <span>{formatTime(p.updated_at)}</span>
              </div>
            </button>
          ))}
        </div>
      )}
    </div>
  );
};

export default VideoCanvasListPage;
