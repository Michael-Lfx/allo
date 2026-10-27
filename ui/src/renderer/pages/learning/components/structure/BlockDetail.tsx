import { Empty, Tag, Typography } from '@arco-design/web-react';
import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { BlockView, StructureView } from '../../types';
import { buildGroupPalette } from './palette';

const { Text } = Typography;

/**
 * 结构块详情（投影非权威标注收敛到详情层）：五键结构位 + 所在主干/阶段 +
 * 依赖层 + 门位标签（door_refs，m1 过门自检读侧）+ 关联关系（三型边，双向
 * 列出）+ 固定文案。点图上的块节点弹出。块 stage 是契约闭词表中文字面
 * （入门/进阶/高级）——展示走 i18n 映射，不直出（en-US 红线）。
 */
export function BlockDetail({
  block,
  structure,
  onClose,
}: {
  block: BlockView;
  structure: StructureView;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const colorFor = useMemo(
    () => buildGroupPalette(structure.spines.map((spine) => spine.name)),
    [structure.spines]
  );
  const relations = structure.edges.filter(
    (edge) => edge.source === block.name || edge.target === block.name
  );
  const stageLabel = (stage: BlockView['stage']) => {
    switch (stage) {
      case '入门':
        return t('learningGraphStructStage_entry');
      case '进阶':
        return t('learningGraphStructStage_intermediate');
      default:
        return t('learningGraphStructStage_advanced');
    }
  };
  const relationLabel = (type: string) => {
    switch (type) {
      case 'assumes':
        return t('learningGraphStructRelation_assumes');
      case 'composes':
        return t('learningGraphStructRelation_composes');
      case 'contrasts':
        return t('learningGraphStructRelation_contrasts');
      default:
        return type;
    }
  };

  return (
    <div
      className='relative mt-8px rounded-8px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-10px'
      style={{ borderLeft: `3px solid ${colorFor(block.spine)}` }}
    >
      <div className='flex flex-wrap items-center gap-8px'>
        <Text bold className='text-13px'>
          {block.name}
        </Text>
        <Tag size='small' color='arcoblue' className='!mx-0'>
          {block.spine}
        </Tag>
        <Tag size='small' className='!mx-0'>
          {stageLabel(block.stage)}
        </Tag>
        <Text type='secondary' className='text-11px'>
          {t('learningGraphStructLayer', { layer: block.layer })}
          {block.no_prereq ? ` · ${t('learningGraphStructNoPrereq')}` : ''}
        </Text>
        <span className='flex-1' />
        <button
          type='button'
          className='cursor-pointer border-none bg-transparent text-12px text-t-secondary'
          onClick={onClose}
        >
          ✕
        </button>
      </div>
      {block.note.trim() !== '' && (
        <div className='mt-6px text-12px text-[var(--color-text-2)]'>{block.note}</div>
      )}
      {block.door_refs.length > 0 && (
        <div className='mt-6px flex flex-wrap items-center gap-4px text-11px'>
          <Text type='secondary' className='text-11px'>
            {t('learningGraphStructDoorRefsLabel')}
          </Text>
          {block.door_refs.map((door) => (
            <Tag key={door} size='small' color='green' className='!mx-0'>
              {door}
            </Tag>
          ))}
        </div>
      )}
      <div className='mt-6px text-11px text-t-tertiary'>slug: {block.slug}</div>
      {relations.length > 0 && (
        <div className='mt-8px flex flex-col gap-4px'>
          {relations.map((edge, index) => (
            <div
              key={`${edge.source}|${edge.target}|${edge.edge_type}|${index}`}
              className='flex flex-wrap items-center gap-6px text-11px'
            >
              <Tag size='small' color='gray' className='!mx-0'>
                {relationLabel(edge.edge_type)}
              </Tag>
              <span className='text-[var(--color-text-2)]'>
                {edge.source === block.name
                  ? t('learningGraphStructRelationOut', {
                      target: edge.target,
                      tier: edge.tier,
                    })
                  : t('learningGraphStructRelationIn', {
                      source: edge.source,
                      tier: edge.tier,
                    })}
              </span>
              {edge.note.trim() !== '' && (
                <span className='text-t-tertiary'>— {edge.note}</span>
              )}
            </div>
          ))}
        </div>
      )}
      <div className='mt-8px border-t border-t-solid border-t-[var(--color-border-2)] pt-6px text-11px text-t-tertiary'>
        {t('learningGraphStructDetailNote')}
      </div>
    </div>
  );
}

/** 未生成/空结构的统一空态。 */
export function StructureEmpty({ hint }: { hint: string }) {
  return <Empty description={hint} />;
}
