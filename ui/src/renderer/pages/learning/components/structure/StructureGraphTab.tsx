import { Button, Empty, Tag, Typography } from '@arco-design/web-react';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import NomiCollapse from '@/renderer/components/base/NomiCollapse';
import type { BasisView, LearningStructureView, MountObservationView } from '../../types';
import { GateCoverage } from './GateCoverage';
import { MatrixView } from './MatrixView';
import { Mindmap } from './Mindmap';
import { RelationNet } from './RelationNet';
import { RouteMap } from './RouteMap';

const { Text } = Typography;

const TABS = [
  ['mindmap', 'learning.learningGraphStructMindmap'],
  ['net', 'learning.learningGraphStructNet'],
  ['route', 'learning.learningGraphStructRoute'],
  ['matrix', 'learning.learningGraphStructMatrix'],
] as const;

type TabKey = (typeof TABS)[number][0];

/** 挂载观察 kind → 词典 key（o4 B6）：三档机器 kind（mounting.rs 单源）+
 * 未知档兜底。可见面只报计数，后端观察句不内插。 */
function mountObservationKey(kind: string): `learning.learningGraphMountObs${string}` {
  if (kind === 'outside-node') return 'learning.learningGraphMountObsOutside';
  if (kind === 'empty-block') return 'learning.learningGraphMountObsEmpty';
  if (kind === 'overloaded-block') return 'learning.learningGraphMountObsOverloaded';
  return 'learning.learningGraphMountObsOther';
}

/** 挂载观察行（o4 B6）：后端观察 detail 是中文模板句内嵌 slug/节点串，
 * 不再内插 Tag 上屏——可见面只出按 kind 的计数（词典模板，三档按固定序）；
 * 原始观察句收进「观察详情」折叠（诊断面如实保留）。独立成组件供渲染
 * 测试直接断言。 */
export function MountObservationSummary({ observations }: { observations: MountObservationView[] }) {
  const { t } = useTranslation();
  if (observations.length === 0) return null;
  const counts = new Map<string, number>();
  for (const observation of observations) {
    counts.set(observation.kind, (counts.get(observation.kind) ?? 0) + 1);
  }
  const knownOrder = ['outside-node', 'empty-block', 'overloaded-block'];
  const kinds = [
    ...knownOrder.filter((kind) => counts.has(kind)),
    ...[...counts.keys()].filter((kind) => !knownOrder.includes(kind)),
  ];
  return (
    <div className='flex flex-col gap-4px'>
      <div className='flex flex-wrap items-center gap-4px'>
        {kinds.map((kind) => (
          <Tag key={kind} size='small' color='gray' className='!mx-0'>
            {t(mountObservationKey(kind), { count: counts.get(kind) })}
          </Tag>
        ))}
      </div>
      <NomiCollapse bordered={false}>
        <NomiCollapse.Item
          name='mount-observation-details'
          header={t('learning.learningGraphMountObsDetails')}
        >
          {observations.map((observation, index) => (
            <Text key={index} type='secondary' className='block break-all text-12px'>
              · {observation.detail}
            </Text>
          ))}
        </NomiCollapse.Item>
      </NomiCollapse>
    </div>
  );
}

/**
 * 结构图 Tab（wayfinder 票 p1 裁决）：思维导图 / 关系图谱 / 学习路线三图 +
 * 矩阵顺带；数据全部来自 `GET graph/structure` 读侧投影（前端不做第二份
 * 真相）；页脚硬标注「投影非权威；层号 = 依赖深度，不是排期」。课程内铁律：
 * 不渲染概念节点、不做块-概念名字匹配。j08 起带挂载三态段（读侧 join，
 * 零第二真相）：三态计数 + stale 显影 + 已挂清单 + 挂载观察 + 挂载按钮。
 * m2 起过门自检卡带已会声明写侧（勾选门口 → 铸概念；声明读数与动作经
 * props 单向下传，本组件不持第二份声明状态）。
 */
export function StructureGraphTab({
  view,
  basis,
  onMount,
  mounting,
  onRepair,
  repairing,
  basisChecked,
  onBasisCheckedChange,
  onDeclareBasis,
  declaringBasis,
}: {
  view: LearningStructureView | null;
  basis?: BasisView | null;
  onMount?: () => void;
  mounting?: boolean;
  onRepair?: () => void;
  repairing?: boolean;
  basisChecked?: string[];
  onBasisCheckedChange?: (doors: string[]) => void;
  onDeclareBasis?: (doors: string[]) => void;
  declaringBasis?: boolean;
}) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<TabKey>('mindmap');

  if (!view || !view.structure) {
    return (
      <Empty
        description={
          view ? t('learning.learningGraphStructureMissing') : t('learning.learningGraphStructureGenerating')
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

      {/* 切换器只控制图视图：画布与图注紧贴切换器（c4）。挂载三态段与过门
          自检是另外两组管理内容，放在图之后——夹在切换器与画布之间会被读
          成「切换器控制的内容」。 */}
      {tab === 'mindmap' && <Mindmap structure={structure} />}
      {tab === 'net' && <RelationNet structure={structure} />}
      {tab === 'route' && <RouteMap structure={structure} />}
      {tab === 'matrix' && <MatrixView structure={structure} />}

      <Text type='secondary' className='text-11px leading-16px'>
        {t('learning.learningGraphStructFooter')}
      </Text>

      {mounts && (
        <div className='flex flex-col gap-6px rounded-8px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-10px'>
          <div className='flex flex-wrap items-center gap-8px'>
            <Text bold className='text-13px'>
              {t('learning.learningGraphMountTitle')}
            </Text>
            <Tag size='small' color='arcoblue' className='!mx-0'>
              {t('learning.learningGraphMountCounts', {
                mounted: mounts.mounted_count,
                rest: mounts.outside_count + mounts.not_evaluated_count,
              })}
            </Tag>
            {mounts.stale && (
              <Tag size='small' color='orangered' className='!mx-0'>
                {t('learning.learningGraphMountStale')}
              </Tag>
            )}
            <span className='flex-1' />
            {onMount && (
              <Button size='mini' loading={mounting} onClick={onMount}>
                {t('learning.learningGraphMountRun')}
              </Button>
            )}
            {onRepair && (
              <Button size='mini' loading={repairing} onClick={onRepair}>
                {t('learning.learningGraphRepairRun')}
              </Button>
            )}
          </div>
          {/* o6 明细收纳：stale 修补指引 / 挂载索引逐行 / 挂载观察（o4 计数
              Tags + 观察详情二级折叠，组件原样保留）收进单一「详情」折叠
              （默认收起）——顶条只留结论（o5 计数口径 + stale 徽标 + 动作）。 */}
          {(mounts.stale || mounts.index.length > 0 || mounts.observations.length > 0) && (
            <NomiCollapse bordered={false}>
              <NomiCollapse.Item
                name='mount-details'
                header={t('learning.learningGraphMountDetails')}
              >
                <div className='flex flex-col gap-6px'>
                  {mounts.stale && (
                    <Text type='warning' className='text-11px leading-16px'>
                      {t('learning.learningGraphRepairStaleBanner')}
                    </Text>
                  )}
                  {mounts.index.length > 0 && (
                    <div className='flex flex-col gap-2px'>
                      {mounts.index.map((entry) => (
                        <Text key={entry.block} type='secondary' className='text-11px leading-16px'>
                          {t('learning.learningGraphMountIndexEntry', {
                            block: entry.block_name ?? entry.block,
                            count: entry.nodes.length,
                            nodes: entry.nodes.join(t('learning.listSeparator')),
                          })}
                        </Text>
                      ))}
                    </div>
                  )}
                  {mounts.observations.length > 0 && (
                    // o4：观察可见面只报计数，后端观察句收进折叠详情。
                    <MountObservationSummary observations={mounts.observations} />
                  )}
                </div>
              </NomiCollapse.Item>
            </NomiCollapse>
          )}
        </div>
      )}

      {view.terrain && (
        // o6 双挂载去重：学习 Tab（声明窗口）保留整卡为主挂载点；结构图
        // Tab 降为默认收起的摘要入口（标题 + 门口计数 + 已声明读数），
        // 展开即整卡（勾选/声明功能不丢）——不做两份默认整卡渲染。
        <NomiCollapse>
          <NomiCollapse.Item
            name='gate-coverage'
            header={
              <span className='flex flex-wrap items-center gap-8px'>
                <Text bold className='text-13px'>
                  {t('learning.learningGraphGateTitle')}
                </Text>
                <Tag size='small' color='arcoblue' className='!mx-0'>
                  {t('learning.learningGraphGateCounts', {
                    doors: view.terrain.doors.length,
                  })}
                </Tag>
                {basis != null && basis.doors.length > 0 && (
                  <Text type='secondary' className='text-11px'>
                    {/* o4 口径：只报数量，不拼门口 id 串。 */}
                    {t('learning.learningGraphBasisDeclaredDoors', {
                      doorsCount: basis.doors.length,
                      count: basis.concepts.length,
                    })}
                  </Text>
                )}
              </span>
            }
          >
            <GateCoverage
              structure={structure}
              terrain={view.terrain}
              basis={basis ?? null}
              checked={basisChecked}
              onCheckedChange={onBasisCheckedChange}
              declaring={declaringBasis}
              onDeclare={onDeclareBasis}
            />
          </NomiCollapse.Item>
        </NomiCollapse>
      )}
    </div>
  );
}
