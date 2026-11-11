import { useTranslation } from 'react-i18next';
import { Button, Card, Spin, Tag, Tooltip, Typography } from '@arco-design/web-react';
import {
  formatGrowthElapsed,
  growthCourseTitle,
  isGrowthActive,
  type GrowthPillItem,
} from '../growthTaskPill';
import type { Translate } from '../utils';

const { Text } = Typography;

/** 机器面 step 常量 → 词典 key（c3 全档映射）：后端 `set_step` 只产英文
 * 机器名，进度人面单句由前端词典承担。覆盖后端全量调用点（generate.rs
 * triage/source_plan/expansion、learning_graph.rs structure/mount/repair/
 * draft、lesson.rs generate）；未知 step 不出进度行（宁缺不泄漏 detail）。
 * 导出供渲染测试逐档断言双语词典对称。 */
export const STEP_KEYS: Record<string, `learning.growthPillStep${string}`> = {
  triage: 'learning.growthPillStepTriage',
  source_plan: 'learning.growthPillStepSourcePlan',
  expansion: 'learning.growthPillStepExpansion',
  structure: 'learning.growthPillStepStructure',
  mount: 'learning.growthPillStepMount',
  repair: 'learning.growthPillStepRepair',
  draft: 'learning.growthPillStepDraft',
  generate: 'learning.growthPillStepGenerate',
};

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

/** 第三行进度读数：打回重起草（WS 增强）> 机器步骤的词典文案（c3：人面
 * 由前端按 step 词典承担，未知/缺席 step 不出进度行，不回落后端 detail）；
 * 排队给位次；失败给原因（诊断面，票面界定保留）；完成/取消不赘述。 */
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
    const stepKey = item.step ? STEP_KEYS[item.step] : undefined;
    return stepKey ? t(stepKey) : null;
  }
  // 失败行不再直出后端 detail（o1）：人面出通用失败句，原文收进 Tooltip
  // 悬停可见（诊断不丢）。没有 detail 时宁缺不泄漏，不出提示行。
  if (item.status === 'failed') {
    return item.detail ? t('learning.growthPillFailedHint') : null;
  }
  return null;
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
  // o4：主题前缀失配（后端改措辞）时不再回显整句后端串——出词典兜底文案，
  // 与 restoredTaskTopic 的 taskKindUnknown 同水位（宁缺不泄漏）。
  const title = growthCourseTitle(item.topic) ?? t('learning.growthPillTopicFallback');
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
          {progress && item.status === 'failed' && item.detail ? (
            // 失败原因原文收纳进 Tooltip（o1：人面通用句 + 悬停诊断不丢）。
            <Tooltip content={item.detail}>
              <Text type='secondary' className='block truncate text-12px'>
                {progress}
              </Text>
            </Tooltip>
          ) : (
            progress && (
              <Text type='secondary' className='block truncate text-12px'>
                {progress}
              </Text>
            )
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
