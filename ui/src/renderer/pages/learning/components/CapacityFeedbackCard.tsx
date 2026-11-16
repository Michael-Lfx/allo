import { Button, Typography } from '@arco-design/web-react';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { learningApi } from '../api';
import {
  capacityChoiceKeys,
  capacityChoices,
  capacityNoteText,
  selectedCapacityChoice,
} from '../capacityFeedback';
import type { CapacityFeedbackChoice, CapacityFeedbackView, Lesson } from '../types';

const { Text } = Typography;

/** 节点容量反馈卡（y07）：仅课时有生成期超容自报（`capacity_note`）时呈现；
 * 三选一 latest-wins 留痕、成功后回显、失败行内 `role=alert` 可重按。
 * **零结算零置位**：这里只调一条留痕接口，不触发重生成/拆节/结算。 */
export function CapacityFeedbackCard({
  lesson,
  onRecorded,
}: {
  lesson: Lesson;
  /** 保存成功回调（调用方可将 saved view 回填本地详情）。 */
  onRecorded?: (feedback: CapacityFeedbackView) => void;
}) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<CapacityFeedbackChoice | null>(() =>
    selectedCapacityChoice(lesson.capacity_feedback)
  );
  const [submitting, setSubmitting] = useState<CapacityFeedbackChoice | null>(null);
  const [saved, setSaved] = useState(false);
  const [failed, setFailed] = useState(false);

  // 详情异步到达 / 切换课时：回显以服务端留痕为准（重载不失忆）。
  useEffect(() => {
    setSelected(selectedCapacityChoice(lesson.capacity_feedback));
    setSaved(false);
    setFailed(false);
  }, [lesson.id, lesson.capacity_feedback]);

  const note = capacityNoteText(lesson);
  if (!note) return null;

  const submit = async (choice: CapacityFeedbackChoice) => {
    setSubmitting(choice);
    setFailed(false);
    try {
      const view = await learningApi.recordCapacityFeedback(lesson.id, choice);
      setSelected(view.choice);
      setSaved(true);
      onRecorded?.(view);
    } catch {
      // 失败不改选中态：行内报错 + 按钮原样可重按。
      setFailed(true);
    } finally {
      setSubmitting(null);
    }
  };

  return (
    <div
      className='flex flex-col gap-8px rd-10px border border-solid border-[var(--color-border-2)] bg-fill-1 p-12px'
      data-testid='capacity-feedback-card'
    >
      <div className='font-600 text-t-primary'>{t('learning.capacityFeedbackTitle')}</div>
      <Text type='secondary' className='text-13px'>
        {t('learning.capacityFeedbackIntro')}
      </Text>
      <div className='text-13px leading-20px text-t-primary'>{note}</div>
      <div className='flex flex-wrap items-center gap-8px'>
        {capacityChoices.map((choice) => (
          <Button
            key={choice}
            size='small'
            type={selected === choice ? 'primary' : 'secondary'}
            loading={submitting === choice}
            disabled={submitting !== null && submitting !== choice}
            onClick={() => void submit(choice)}
          >
            {t(capacityChoiceKeys[choice])}
          </Button>
        ))}
        {saved && (
          <Text type='success' className='text-12px'>
            {t('learning.capacityFeedbackSaved')}
          </Text>
        )}
      </div>
      <div className='text-12px text-t-tertiary'>{t('learning.capacityFeedbackHint')}</div>
      {failed && (
        <Text role='alert' className='text-12px text-[var(--color-danger-6)]'>
          {t('learning.capacityFeedbackError')}
        </Text>
      )}
    </div>
  );
}
