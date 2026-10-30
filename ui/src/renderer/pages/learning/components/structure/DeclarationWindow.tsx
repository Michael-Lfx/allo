import { Alert, Button, Spin, Tag, Typography } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import type { BasisView, LearningStructureView } from '../../types';
import { GateCoverage } from './GateCoverage';

const { Text, Title } = Typography;

/**
 * 声明窗口（e3：建课后暂停在门口）：学习图课程零节点（结构完成、首批未生
 * 长）时学习 Tab 的落地画面——门口梯队 + 已会声明入口（复用过门自检卡，
 * 单向下传）+「开始学习」主动作（显式触发首批生长）。声明不是强制的：纯
 * 零基础用户直接点开始。learnhub 语义（ADR-0024/0032）：生长是学习动作的
 * 副作用，声明只铸概念不生成任何节点——窗口不代用户点火。
 */
export function DeclarationWindow({
  view,
  structureRunning,
  structureError,
  basis,
  declaring,
  revoking,
  onDeclare,
  onRevoke,
  starting,
  growthRunning,
  onStart,
}: {
  view: LearningStructureView | null;
  structureRunning: boolean;
  structureError: string | null;
  basis: BasisView | null;
  declaring: boolean;
  revoking: boolean;
  onDeclare?: (doors: string[]) => void;
  onRevoke?: () => void;
  starting: boolean;
  growthRunning: boolean;
  onStart: () => void;
}) {
  const { t } = useTranslation();

  let body;
  if (structureError) {
    body = (
      <Alert
        type='error'
        content={`${t('learning.loadFailed')}: ${structureError}`}
      />
    );
  } else if (view === null) {
    body = (
      <div className='flex items-center gap-8px'>
        <Spin size={14} />
        <Text type='secondary' className='text-12px'>
          {t('learning.learningGraphWindowStructurePending')}
        </Text>
      </div>
    );
  } else if (structureRunning || !view.structure) {
    body = (
      <div className='flex flex-wrap items-center gap-8px'>
        {structureRunning && (
          <Tag size='small' color='processing' className='!mx-0'>
            {t('learning.learningGraphStructureGenerating')}
          </Tag>
        )}
        <Text type='secondary' className='text-12px'>
          {view.structure
            ? t('learning.learningGraphWindowStructurePending')
            : t('learning.learningGraphWindowStructureMissing')}
        </Text>
      </div>
    );
  } else {
    body = (
      <>
        {view.terrain && (
          <GateCoverage
            structure={view.structure}
            terrain={view.terrain}
            basis={basis}
            declaring={declaring}
            revoking={revoking}
            onDeclare={onDeclare}
            onRevoke={onRevoke}
          />
        )}
        <div className='flex flex-wrap items-center gap-8px'>
          <Button
            type='primary'
            loading={starting}
            disabled={growthRunning}
            onClick={onStart}
          >
            {t('learning.learningGraphWindowStart')}
          </Button>
          {growthRunning && (
            <Text type='secondary' className='text-12px'>
              {t('learning.learningGraphWindowGrowing')}
            </Text>
          )}
        </div>
      </>
    );
  }

  return (
    <div className='flex flex-col gap-10px rounded-10px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-16px'>
      <div className='flex flex-col gap-4px'>
        <Title heading={5} className='!m-0'>
          {t('learning.learningGraphWindowTitle')}
        </Title>
        <Text type='secondary' className='text-12px leading-20px'>
          {t('learning.learningGraphWindowBody')}
        </Text>
      </div>
      {body}
    </div>
  );
}
