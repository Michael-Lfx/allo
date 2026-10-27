import { Tag, Typography } from '@arco-design/web-react';
import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { StructureView, TerrainView } from '../../types';

const { Text } = Typography;

/**
 * 过门自检读侧（m1，learnhub #165 契约 v2.14）：「门口 → 覆盖块」映射展示。
 * 数据全部来自 GET graph/structure 读侧投影（terrain.doors 的块覆盖统计 +
 * blocks[].door_refs 反查覆盖块名），前端不做第二份真相；声明写侧（勾选 →
 * 铸概念）归后续版本，本卡只读。课程内铁律照旧：不渲染概念、不跨课解析。
 */
export function GateCoverage({ structure, terrain }: { structure: StructureView; terrain: TerrainView }) {
  const { t } = useTranslation();
  const blocksByDoor = useMemo(() => {
    const byDoor = new Map<string, string[]>();
    for (const block of structure.blocks) {
      for (const door of block.door_refs) {
        const names = byDoor.get(door) ?? [];
        names.push(block.name);
        byDoor.set(door, names);
      }
    }
    return byDoor;
  }, [structure.blocks]);

  if (terrain.doors.length === 0) return null;

  return (
    <div className='flex flex-col gap-6px rounded-8px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-10px'>
      <div className='flex flex-wrap items-center gap-8px'>
        <Text bold className='text-13px'>
          {t('learningGraphGateTitle')}
        </Text>
        <Tag size='small' color='arcoblue' className='!mx-0'>
          {t('learningGraphGateCounts', { doors: terrain.doors.length })}
        </Tag>
        <span className='flex-1' />
        <Text type='secondary' className='text-11px'>
          {t('learningGraphGateHint')}
        </Text>
      </div>
      <div className='flex flex-col gap-4px'>
        {terrain.doors.map((door) => {
          const blocks = blocksByDoor.get(door.id) ?? [];
          return (
            <div
              key={door.id}
              className='flex flex-wrap items-center gap-6px text-11px'
            >
              <Tag size='small' color={door.kind === 'hard' ? 'orangered' : 'green'} className='!mx-0'>
                {door.id}
              </Tag>
              <Tag size='small' className='!mx-0'>
                {door.kind === 'hard'
                  ? t('learningGraphGateKindHard')
                  : t('learningGraphGateKindSelfCheck')}
              </Tag>
              <span className='text-[var(--color-text-2)]'>{door.criterion}</span>
              <span className='flex-1' />
              <Text type='secondary' className='text-11px leading-16px'>
                {blocks.length > 0
                  ? t('learningGraphGateDoorCovers', {
                      count: blocks.length,
                      blocks: blocks.join('、'),
                    })
                  : t('learningGraphGateDoorCoversNone')}
              </Text>
            </div>
          );
        })}
      </div>
    </div>
  );
}
