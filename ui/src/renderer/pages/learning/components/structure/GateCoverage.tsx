import { Button, Modal, Tag, Typography } from '@arco-design/web-react';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { BasisView, StructureView, TerrainView } from '../../types';

const { Text } = Typography;

/**
 * 过门自检读侧（m1，learnhub #165 契约 v2.14）+ 已会声明写侧（m2，ADR-0032
 * 移植）：「门口 → 覆盖块」映射展示；勾选满足的门口 → 确认（三后果如实：
 * 不生成学习节点、不安排学习、不计 XP）→ 按声明铸概念。数据全部来自读侧
 * 投影（terrain.doors + blocks[].door_refs + GET basis 读数），前端不做第二
 * 份真相。撤销 = 删锚行收缩本课已知投影；铸出的概念留池（声明可撤、概念
 * 不灭）。课程内铁律照旧：不渲染概念节点、不跨课解析。
 */
export function GateCoverage({
  structure,
  terrain,
  basis,
  declaring = false,
  revoking = false,
  onDeclare,
  onRevoke,
}: {
  structure: StructureView;
  terrain: TerrainView;
  basis: BasisView | null;
  declaring?: boolean;
  revoking?: boolean;
  onDeclare?: (doors: string[]) => void;
  onRevoke?: () => void;
}) {
  const { t } = useTranslation();
  const [checked, setChecked] = useState<string[]>([]);
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

  const declaredDoors = useMemo(() => new Set(basis?.doors ?? []), [basis]);
  const selectableDoors = useMemo(
    () => terrain.doors.filter((door) => !declaredDoors.has(door.id)),
    [terrain.doors, declaredDoors]
  );
  const pendingSelection = checked.filter((door) => !declaredDoors.has(door));

  const confirmDeclare = () => {
    if (pendingSelection.length === 0 || !onDeclare) return;
    Modal.confirm({
      title: t('learning.learningGraphBasisConfirmTitle'),
      content: t('learning.learningGraphBasisConfirmContent', {
        count: pendingSelection.length,
        doors: pendingSelection.join('、'),
      }),
      okText: t('learning.learningGraphBasisConfirmOk'),
      cancelText: t('learning.learningGraphBasisCancel'),
      onOk: () => {
        onDeclare(pendingSelection);
        setChecked([]);
      },
    });
  };

  const confirmRevoke = () => {
    if (!onRevoke) return;
    Modal.confirm({
      title: t('learning.learningGraphBasisRevokeConfirmTitle'),
      content: t('learning.learningGraphBasisRevokeConfirmContent', {
        doors: (basis?.doors ?? []).join('、'),
      }),
      okText: t('learning.learningGraphBasisRevokeConfirmOk'),
      cancelText: t('learning.learningGraphBasisCancel'),
      onOk: () => onRevoke(),
    });
  };

  if (terrain.doors.length === 0) return null;

  return (
    <div className='flex flex-col gap-6px rounded-8px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-10px'>
      <div className='flex flex-wrap items-center gap-8px'>
        <Text bold className='text-13px'>
          {t('learning.learningGraphGateTitle')}
        </Text>
        <Tag size='small' color='arcoblue' className='!mx-0'>
          {t('learning.learningGraphGateCounts', { doors: terrain.doors.length })}
        </Tag>
        <span className='flex-1' />
        <Text type='secondary' className='text-11px'>
          {t('learning.learningGraphGateHint')}
        </Text>
      </div>
      <div className='flex flex-col gap-4px'>
        {terrain.doors.map((door) => {
          const blocks = blocksByDoor.get(door.id) ?? [];
          const declared = declaredDoors.has(door.id);
          const isSelectable = selectableDoors.some((candidate) => candidate.id === door.id);
          return (
            <div
              key={door.id}
              className='flex flex-wrap items-center gap-6px text-11px'
            >
              {isSelectable && onDeclare ? (
                <input
                  type='checkbox'
                  className='shrink-0 cursor-pointer'
                  aria-label={`${t('learning.learningGraphBasisDoorAria')}: ${door.id}`}
                  checked={checked.includes(door.id)}
                  onChange={(event) =>
                    setChecked((previous) =>
                      event.target.checked
                        ? [...previous, door.id]
                        : previous.filter((candidate) => candidate !== door.id)
                    )
                  }
                />
              ) : null}
              <Tag size='small' color={door.kind === 'hard' ? 'orangered' : 'green'} className='!mx-0'>
                {door.id}
              </Tag>
              <Tag size='small' className='!mx-0'>
                {door.kind === 'hard'
                  ? t('learning.learningGraphGateKindHard')
                  : t('learning.learningGraphGateKindSelfCheck')}
              </Tag>
              {declared && (
                <Tag size='small' color='green' className='!mx-0'>
                  {t('learning.learningGraphBasisDeclaredTag')}
                </Tag>
              )}
              <span className='text-[var(--color-text-2)]'>{door.criterion}</span>
              <span className='flex-1' />
              <Text type='secondary' className='text-11px leading-16px'>
                {blocks.length > 0
                  ? t('learning.learningGraphGateDoorCovers', {
                      count: blocks.length,
                      blocks: blocks.join('、'),
                    })
                  : t('learning.learningGraphGateDoorCoversNone')}
              </Text>
            </div>
          );
        })}
      </div>
      {onDeclare && (
        <div className='flex flex-wrap items-center gap-8px'>
          {basis !== null && !basis.enabled && (
            <Tag size='small' color='gray' className='!mx-0'>
              {t('learning.learningGraphBasisOff')}
            </Tag>
          )}
          {basis !== null && basis.unresolved_concept_ids.length > 0 && (
            <Tag size='small' color='orange' className='!mx-0'>
              {t('learning.learningGraphBasisUnresolved', {
                count: basis.unresolved_concept_ids.length,
              })}
            </Tag>
          )}
          <span className='flex-1' />
          {basis !== null && basis.declarations > 0 && onRevoke && (
            <Button size='mini' status='warning' loading={revoking} onClick={confirmRevoke}>
              {t('learning.learningGraphBasisRevoke')}
            </Button>
          )}
          <Button
            type='primary'
            size='mini'
            disabled={pendingSelection.length === 0 || (basis !== null && !basis.enabled)}
            loading={declaring}
            onClick={confirmDeclare}
          >
            {t('learning.learningGraphBasisDeclareButton', { count: pendingSelection.length })}
          </Button>
        </div>
      )}
      {basis !== null && basis.doors.length > 0 && (
        <Text type='secondary' className='text-11px leading-16px'>
          {t('learning.learningGraphBasisDeclaredDoors', {
            doors: basis.doors.join('、'),
            count: basis.concepts.length,
          })}
        </Text>
      )}
    </div>
  );
}
