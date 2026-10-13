import { Typography } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import type { CourseSamplingView } from '../types';

const { Text } = Typography;

/** 采样披露行（t2）：已采/总数 + 连贯补入 + 低分未采；目标与库无交集时如实
 * 呈现退回默认顺序，不冒充按目标采样。建课工作区（悬浮看示例）与生成完成
 * 视图共用同一条——披露口径一处维护，两个视图不会说两套话。 */
export function CourseSamplingLine({
  sampling,
  className,
}: {
  sampling: CourseSamplingView;
  /** 调用方追加的样式（工作区里是 `cursor-help`——外面套着悬浮提示） */
  className?: string;
}) {
  const { t } = useTranslation();
  return (
    <Text type='secondary' className={`text-12px${className ? ` ${className}` : ''}`}>
      {sampling.goal_fallback
        ? t('learning.samplingLineFallback', {
            sampled: sampling.sampled_files,
            total: sampling.total_files,
          })
        : t('learning.samplingLine', {
            sampled: sampling.sampled_files,
            total: sampling.total_files,
          })}
      {sampling.coherence_files > 0
        ? ` · ${t('learning.samplingCoherence', { count: sampling.coherence_files })}`
        : ''}
      {sampling.low_score_files > 0
        ? ` · ${t('learning.samplingLowScore', { count: sampling.low_score_files })}`
        : ''}
    </Text>
  );
}
