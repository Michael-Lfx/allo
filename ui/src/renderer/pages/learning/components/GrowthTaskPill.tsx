import { useTranslation } from 'react-i18next';
import { Button, Card, Spin, Tag, Typography } from '@arco-design/web-react';
import {
  formatGrowthElapsed,
  growthCourseTitle,
  isGrowthActive,
  type GrowthPillItem,
} from '../growthTaskPill';
import type { Translate } from '../utils';

const { Text } = Typography;

function statusLabel(t: Translate, item: GrowthPillItem): string {
  switch (item.status) {
    case 'queued':
      return t('learning.growthPillQueued');
    case 'running':
      return item.elapsedSecs === null
        ? t('learning.growthPillRunning')
        : `${t('learning.growthPillRunning')} · ${formatGrowthElapsed(item.elapsedSecs)}`;
    case 'done':
      return t('learning.growthPillDone');
    case 'failed':
      return t('learning.growthPillFailed');
    case 'cancelled':
      return t('learning.growthPillCancelled');
  }
}

/** 第三行进度读数：打回重起草（WS 增强）> 机器步骤的本地化文案 > 后端
 * 人面单句；排队给位次；失败给原因；完成/取消不赘述。 */
function progressLine(t: Translate, item: GrowthPillItem): string | null {
  if (item.status === 'queued') {
    return item.queuePosition === null
      ? null
      : t('learning.growthPillQueuePosition', { position: item.queuePosition });
  }
  if (item.status === 'running') {
    if (item.redraft) {
      return t(
        item.redraft.stage === 'concept_review'
          ? 'learning.growthPillRedraftReview'
          : 'learning.growthPillRedraftGates',
        { count: item.redraft.count }
      );
    }
    if (item.step === 'draft') return t('learning.growthPillStepDraft');
    return item.detail;
  }
  return item.status === 'failed' ? item.detail : null;
}

/** 学习图生长悬浮指示条（g4）：复用建课链 CourseGenerationPill 骨架。
 * 排队/运行中转圈 + 进度、可取消；终态（以 HTTP 快照为准）✓/✕ + 关闭。
 * `onView` 缺省 = 已在该课程图工作区，不再出「回到图工作区」。
 * 定位（fixed 右下角）由宿主 GrowthTaskPillHost 的停靠栏承担。 */
export function GrowthTaskPill({
  item,
  cancelling = false,
  onView,
  onCancel,
  onDismiss,
}: {
  item: GrowthPillItem;
  cancelling?: boolean;
  onView?: () => void;
  onCancel: () => void;
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
  const active = isGrowthActive(item.status);
  const title = growthCourseTitle(item.topic);
  const progress = progressLine(t, item);
  return (
    <Card size='small' className='shadow-lg'>
      <div className='flex items-center gap-12px'>
        {active ? (
          <Spin size={14} />
        ) : (
          <Tag
            size='small'
            color={item.status === 'failed' ? 'red' : item.status === 'cancelled' ? 'orange' : 'green'}
            className='!mx-0'
          >
            {item.status === 'done' ? '✓' : '✕'}
          </Tag>
        )}
        <div className='min-w-0 max-w-260px'>
          <Text className='block truncate text-13px font-500'>{statusLabel(t, item)}</Text>
          {title && (
            <Text type='secondary' className='block truncate text-12px'>
              {title}
            </Text>
          )}
          {progress && (
            <Text type='secondary' className='block truncate text-12px'>
              {progress}
            </Text>
          )}
        </div>
        {onView && (
          <Button size='mini' type='secondary' onClick={onView}>
            {t('learning.growthPillView')}
          </Button>
        )}
        {active ? (
          <Button size='mini' status='danger' loading={cancelling} onClick={onCancel}>
            {t('learning.genCancel')}
          </Button>
        ) : (
          <Button size='mini' type='text' onClick={onDismiss}>
            {t('learning.growthPillDismiss')}
          </Button>
        )}
      </div>
    </Card>
  );
}
