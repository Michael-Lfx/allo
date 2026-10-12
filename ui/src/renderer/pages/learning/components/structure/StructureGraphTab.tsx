import { Empty, Typography } from '@arco-design/web-react';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { LearningStructureView } from '../../types';
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
 * 不渲染概念节点、不做块-概念名字匹配。
 */
export function StructureGraphTab({ view }: { view: LearningStructureView | null }) {
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
