import { Button, Empty, Tag, Typography } from '@arco-design/web-react';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { LearningStructureView } from '../../types';
import { GateCoverage } from './GateCoverage';
import { MatrixView } from './MatrixView';
import { Mindmap } from './Mindmap';
import { RelationNet } from './RelationNet';
import { RouteMap } from './RouteMap';

const { Text } = Typography;

const TABS = [
  ['mindmap', 'learningGraphStructMindmap'],
  ['net', 'learningGraphStructNet'],
  ['route', 'learningGraphStructRoute'],
  ['matrix', 'learningGraphStructMatrix'],
] as const;

type TabKey = (typeof TABS)[number][0];

/**
 * 结构图 Tab（wayfinder 票 p1 裁决）：思维导图 / 关系图谱 / 学习路线三图 +
 * 矩阵顺带；数据全部来自 `GET graph/structure` 读侧投影（前端不做第二份
 * 真相）；页脚硬标注「投影非权威；层号 = 依赖深度，不是排期」。课程内铁律：
 * 不渲染概念节点、不做块-概念名字匹配。j08 起带挂载三态段（读侧 join，
 * 零第二真相）：三态计数 + stale 显影 + 已挂清单 + 挂载观察 + 挂载按钮。
 */
export function StructureGraphTab({
  view,
  onMount,
  mounting,
  onRepair,
  repairing,
}: {
  view: LearningStructureView | null;
  onMount?: () => void;
  mounting?: boolean;
  onRepair?: () => void;
  repairing?: boolean;
}) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<TabKey>('mindmap');

  if (!view || !view.structure) {
    return (
      <Empty
        description={
          view ? t('learningGraphStructureMissing') : t('learningGraphStructureGenerating')
        }
      />
    );
  }
  const structure = view.structure;
  const mounts = view.mounts;

  return (
    <div className='flex flex-col gap-8px pb-16px'>
      <div className='flex overflow-hidden rounded-6px border-1 border-solid border-[var(--color-border-2)] text-12px'>
        {TABS.map(([key, labelKey]) => (
          <button
            key={key}
            type='button'
            onClick={() => setTab(key)}
            className={`cursor-pointer border-none px-12px py-5px text-12px transition-colors ${
              tab === key
                ? 'bg-[var(--color-primary-6)] text-white'
                : 'bg-transparent text-[var(--color-text-2)] hover:bg-[var(--color-fill-1)]'
            }`}
          >
            {t(labelKey)}
          </button>
        ))}
      </div>

      {mounts && (
        <div className='flex flex-col gap-6px rounded-8px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-10px'>
          <div className='flex flex-wrap items-center gap-8px'>
            <Text bold className='text-13px'>
              {t('learningGraphMountTitle')}
            </Text>
            <Tag size='small' color='arcoblue' className='!mx-0'>
              {t('learningGraphMountCounts', {
                mounted_count: mounts.mounted_count,
                outside_count: mounts.outside_count,
                not_evaluated_count: mounts.not_evaluated_count,
              })}
            </Tag>
            {mounts.stale && (
              <Tag size='small' color='orangered' className='!mx-0'>
                {t('learningGraphMountStale')}
              </Tag>
            )}
            <span className='flex-1' />
            {onMount && (
              <Button size='mini' loading={mounting} onClick={onMount}>
                {t('learningGraphMountRun')}
              </Button>
            )}
            {onRepair && (
              <Button size='mini' loading={repairing} onClick={onRepair}>
                {t('learningGraphRepairRun')}
              </Button>
            )}
          </div>
          {mounts.stale && (
            <Text type='warning' className='text-11px leading-16px'>
              {t('learningGraphRepairStaleBanner')}
            </Text>
          )}
          {mounts.index.length > 0 && (
            <div className='flex flex-col gap-2px'>
              {mounts.index.map((entry) => (
                <Text key={entry.block} type='secondary' className='text-11px leading-16px'>
                  {t('learningGraphMountIndexEntry', {
                    block: entry.block_name ?? entry.block,
                    count: entry.nodes.length,
                    nodes: entry.nodes.join('、'),
                  })}
                </Text>
              ))}
            </div>
          )}
          {mounts.observations.length > 0 && (
            <div className='flex flex-wrap items-center gap-4px'>
              {mounts.observations.map((observation, index) => (
                <Tag key={index} size='small' color='gray' className='!mx-0'>
                  {t(
                    observation.kind === 'outside-node'
                      ? 'learningGraphMountObsOutside'
                      : observation.kind === 'empty-block'
                        ? 'learningGraphMountObsEmpty'
                        : 'learningGraphMountObsOverloaded',
                    { detail: observation.detail }
                  )}
                </Tag>
              ))}
            </div>
          )}
        </div>
      )}

      {view.terrain && <GateCoverage structure={structure} terrain={view.terrain} />}

      {tab === 'mindmap' && <Mindmap structure={structure} />}
      {tab === 'net' && <RelationNet structure={structure} />}
      {tab === 'route' && <RouteMap structure={structure} />}
      {tab === 'matrix' && <MatrixView structure={structure} />}

      <Text type='secondary' className='text-11px leading-16px'>
        {t('learningGraphStructFooter')}
      </Text>
    </div>
  );
}
