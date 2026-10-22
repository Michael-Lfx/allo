import { useTranslation } from 'react-i18next';
import { Alert, Modal, Typography } from '@arco-design/web-react';
import type { CourseGenerationEstimateView } from '../types';
import { estimateLines, triageCountsLine, triageViewCounts, uncertainTooMany } from '../model';

const { Text } = Typography;

/** t5 成本门确认弹窗：>40 文件或预估 >20 次 LLM 调用时，提交建课前展示
 * 估算（文件数/批数/调用数/预计耗时）与规则分诊读数（计数 + 跳过摘要），
 * 「继续」放行原请求、「取消」丢弃。低于阈值不打扰（后端单源判定）。
 * 估算的分诊是规则分诊（degraded），如实标注「未做 LLM 判定」。 */
export function CourseCostGateModal({
  estimate,
  onConfirm,
  onCancel,
}: {
  estimate: CourseGenerationEstimateView;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const lines = estimateLines(estimate, t);
  const counts = triageViewCounts(estimate.triage);
  const skipped = estimate.skipped_examples ?? [];

  return (
    <Modal
      title={t('learning.costGateTitle')}
      visible
      okText={t('learning.costGateConfirm')}
      cancelText={t('learning.costGateCancel')}
      onOk={onConfirm}
      onCancel={onCancel}
      style={{ width: 480 }}
    >
      <div className='flex flex-col gap-12px'>
        <div className='flex flex-col gap-4px'>
          {lines.map((line) => (
            <Text key={line} className='text-13px'>
              {line}
            </Text>
          ))}
        </div>
        {/* 顺带的规则分诊读数（Q13）：估算阶段只做了规则判定 */}
        <div className='flex flex-col gap-4px rounded-8px bg-[var(--color-fill-1)] p-8px'>
          <Text className='text-13px'>{triageCountsLine(counts, t)}</Text>
          {estimate.triage.degraded && (
            <Text type='warning' className='text-12px'>
              {t('learning.genTriageDegraded')}
            </Text>
          )}
          {uncertainTooMany(counts) && (
            <Text type='warning' className='text-12px'>
              {t('learning.genUncertainTooMany', { count: counts.uncertain })}
            </Text>
          )}
          {skipped.length > 0 && (
            <div className='flex flex-col gap-2px'>
              <Text type='secondary' className='text-12px'>
                {t('learning.genTriageExcludedTitle')}
              </Text>
              {skipped.map((entry) => (
                <Text key={entry.path} type='secondary' className='text-12px break-all'>
                  · {entry.path} — {entry.reason}
                </Text>
              ))}
            </div>
          )}
        </div>
        <Alert type='info' content={t('learning.costGateHint')} />
      </div>
    </Modal>
  );
}
