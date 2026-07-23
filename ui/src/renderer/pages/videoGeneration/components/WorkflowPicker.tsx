

import React from 'react';
import { useTranslation } from 'react-i18next';
import { Modal, Input } from '@arco-design/web-react';
import { VideoOne, FileText, BookOpen } from '@icon-park/react';
import type { VimaxWorkflow } from '../types';

const WORKFLOWS: {
  id: VimaxWorkflow;
  icon: React.ReactNode;
  titleKey: string;
  titleDefault: string;
  descKey: string;
  descDefault: string;
}[] = [
  {
    id: 'idea2video',
    icon: <VideoOne theme='outline' size={22} fill='currentColor' />,
    titleKey: 'videoGeneration.workflow.idea2video.title',
    titleDefault: '灵感成片',
    descKey: 'videoGeneration.workflow.idea2video.desc',
    descDefault: '从一个想法出发，自动规划并生成视频。',
  },
  {
    id: 'script2video',
    icon: <FileText theme='outline' size={22} fill='currentColor' />,
    titleKey: 'videoGeneration.workflow.script2video.title',
    titleDefault: '剧本成片',
    descKey: 'videoGeneration.workflow.script2video.desc',
    descDefault: '基于完整剧本拆解镜头并渲染成片。',
  },
  {
    id: 'novel2video',
    icon: <BookOpen theme='outline' size={22} fill='currentColor' />,
    titleKey: 'videoGeneration.workflow.novel2video.title',
    titleDefault: '小说成片',
    descKey: 'videoGeneration.workflow.novel2video.desc',
    descDefault: '从小说文本提炼剧情，规划并生成视频。',
  },
];

interface WorkflowPickerProps {
  visible: boolean;
  loading?: boolean;
  onCancel: () => void;
  onConfirm: (workflow: VimaxWorkflow, title?: string) => void;
}

const WorkflowPicker: React.FC<WorkflowPickerProps> = ({ visible, loading, onCancel, onConfirm }) => {
  const { t } = useTranslation();
  const [selected, setSelected] = React.useState<VimaxWorkflow>('idea2video');
  const [title, setTitle] = React.useState('');

  React.useEffect(() => {
    if (visible) {
      setSelected('idea2video');
      setTitle('');
    }
  }, [visible]);

  return (
    <Modal
      title={t('videoGeneration.create.title', { defaultValue: '新建视频任务' })}
      visible={visible}
      confirmLoading={loading}
      onCancel={onCancel}
      onOk={() => onConfirm(selected, title.trim() || undefined)}
      okText={t('videoGeneration.create.confirm', { defaultValue: '创建并打开' })}
      cancelText={t('common.cancel', { defaultValue: '取消' })}
      unmountOnExit
      style={{ width: 520 }}
    >
      <div className='flex flex-col gap-16px'>
        <p className='m-0 text-13px text-[var(--color-text-3)] leading-20px'>
          {t('videoGeneration.create.hint', {
            defaultValue: '选择一种工作流。创建后工作流类型不可更改。',
          })}
        </p>

        <div className='flex flex-col gap-8px'>
          {WORKFLOWS.map((wf) => {
            const active = selected === wf.id;
            return (
              <button
                key={wf.id}
                type='button'
                onClick={() => setSelected(wf.id)}
                className={[
                  'flex items-start gap-12px w-full text-left px-14px py-12px rd-10px cursor-pointer',
                  'border border-solid transition-colors',
                  active
                    ? 'border-[rgb(var(--primary-6))] bg-[rgba(var(--primary-6),0.08)] text-[rgb(var(--primary-6))]'
                    : 'border-[var(--color-border-2)] bg-[var(--color-bg-2)] text-[var(--color-text-1)] hover:border-[var(--color-border-3)] hover:bg-[var(--color-fill-1)]',
                ].join(' ')}
              >
                <span
                  className={[
                    'flex items-center justify-center w-36px h-36px rd-9px shrink-0',
                    active ? 'text-[rgb(var(--primary-6))]' : 'text-[var(--color-text-3)]',
                  ].join(' ')}
                  style={{
                    background: active
                      ? 'rgba(var(--primary-6),0.12)'
                      : 'var(--color-fill-2)',
                  }}
                >
                  {wf.icon}
                </span>
                <span className='min-w-0 flex-1'>
                  <span className='block text-14px font-600 leading-22px'>
                    {t(wf.titleKey, { defaultValue: wf.titleDefault })}
                  </span>
                  <span className='block text-12px leading-18px text-[var(--color-text-3)] mt-2px'>
                    {t(wf.descKey, { defaultValue: wf.descDefault })}
                  </span>
                </span>
              </button>
            );
          })}
        </div>

        <div className='flex flex-col gap-6px'>
          <label className='text-13px text-[var(--color-text-2)]'>
            {t('videoGeneration.create.titleLabel', { defaultValue: '标题（可选）' })}
          </label>
          <Input
            value={title}
            onChange={setTitle}
            maxLength={80}
            placeholder={t('videoGeneration.create.titlePlaceholder', {
              defaultValue: '给这次生成起个名字',
            })}
            onPressEnter={() => onConfirm(selected, title.trim() || undefined)}
          />
        </div>
      </div>
    </Modal>
  );
};

export default WorkflowPicker;
