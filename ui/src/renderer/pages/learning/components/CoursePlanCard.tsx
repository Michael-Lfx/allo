import { useTranslation } from 'react-i18next';
import { Alert, Button, Typography } from '@arco-design/web-react';
import {
  planReadout,
  planSlotLines,
  type CoursePlan,
  type PlanEstimateView,
} from '../coursePlan';

const { Text, Title } = Typography;

/** 建课计划卡（y05）：对话框内就地切换的只读确认面——四槽回显 + 确定性
 * 读数 + 「返回修改」「开始生成」。KB 流的成本门并入此卡（needs_confirm
 * 时卡内醒目提示），确认按钮就是唯一的生成触发点；图/描述流如实呈
 * 「按声明/简报酌定」，不显示虚构的节数/批数/时长。 */
export function CoursePlanCard({
  plan,
  sources,
  estimate,
  busy = false,
  onBack,
  onConfirm,
}: {
  plan: CoursePlan;
  /** KB 来源库名（首个 = 主来源；空 = 未解析到名称） */
  sources: string[];
  estimate: PlanEstimateView;
  busy?: boolean;
  /** 返回修改：回表单，原值保留，零请求 */
  onBack: () => void;
  /** 开始生成：计划态唯一的生成触发点（同一计划恰一次） */
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  const slots = planSlotLines(plan, sources, t);
  const readout = planReadout(plan, estimate, t);

  return (
    <div className='flex flex-col gap-16px' data-testid='course-plan'>
      <div>
        <Title heading={6} className='!m-0'>
          {t('learning.planTitle')}
        </Title>
        <Text type='secondary' className='text-12px'>
          {t('learning.planHint')}
        </Text>
      </div>

      {/* 四槽回显：方式/来源、目标或简报、教学风格、模型 */}
      <div className='flex flex-col gap-8px rounded-8px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-12px'>
        {slots.map((slot) => (
          <div key={slot.key} className='flex gap-12px text-13px leading-20px'>
            <span className='w-88px shrink-0 text-t-secondary'>{slot.label}</span>
            <span className='min-w-0 break-all text-t-primary'>{slot.value}</span>
          </div>
        ))}
      </div>

      {/* 确定性读数：KB 用现有零 LLM 估算；图/描述不带任何数字 */}
      <div className='flex flex-col gap-6px rounded-8px bg-[var(--color-fill-1)] p-12px'>
        <Text className='font-500'>{readout.title}</Text>
        {readout.note && <Text type='secondary'>{readout.note}</Text>}
        {readout.lines.map((line) => (
          <Text key={line} className='text-13px'>
            {line}
          </Text>
        ))}
        {readout.declarations.map((declaration) => (
          <Text
            key={declaration.label}
            type={declaration.present ? undefined : 'secondary'}
            className='text-13px'
          >
            {t(
              declaration.present
                ? 'learning.planDeclarationPresent'
                : 'learning.planDeclarationMissing',
              { label: declaration.label }
            )}
          </Text>
        ))}
        {readout.triageLine && <Text className='text-13px'>{readout.triageLine}</Text>}
        {readout.degraded && (
          <Text type='warning' className='text-12px'>
            {t('learning.genTriageDegraded')}
          </Text>
        )}
        {readout.uncertainLine && (
          <Text type='warning' className='text-12px'>
            {readout.uncertainLine}
          </Text>
        )}
        {readout.skipped.length > 0 && (
          <div className='flex flex-col gap-2px'>
            <Text type='secondary' className='text-12px'>
              {readout.skippedTitle}
            </Text>
            {readout.skipped.map((entry) => (
              <Text key={entry.path} type='secondary' className='text-12px break-all'>
                · {entry.path} — {entry.reason}
              </Text>
            ))}
          </div>
        )}
        {/* 成本门并入计划卡：超阈值时醒目提示，确认按钮即放行（无二次弹窗） */}
        {readout.needsConfirmHint && (
          <Alert type='warning' content={readout.needsConfirmHint} />
        )}
      </div>

      <div className='flex justify-end gap-8px'>
        <Button onClick={onBack}>{t('learning.planBack')}</Button>
        <Button type='primary' loading={busy} onClick={onConfirm}>
          {t('learning.planStart')}
        </Button>
      </div>
    </div>
  );
}
