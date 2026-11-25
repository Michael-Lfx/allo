import { Alert, Tag, Typography } from '@arco-design/web-react';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';
import NomiCollapse from '@/renderer/components/base/NomiCollapse';
import { learningApi } from '../../api';
import type { StructureFindingView, StructureFindingsView } from '../../types';
import { errorMessage, type Translate } from '../../utils';

const { Text } = Typography;

/** 留痕来源的机器名 → 词典 key（写侧单源：`concept-review` = 评审 warn 通道，
 * `growth-budget` = 预算留痕）。未知值兜底 = 原样透出机器名——读侧宽容，
 * 后端新增来源时不显示裸词典键。 */
const SOURCE_LABEL_KEYS: Record<string, string> = {
  'concept-review': 'learning.learningGraphFindingsSourceConceptReview',
  'growth-budget': 'learning.learningGraphFindingsSourceGrowthBudget',
};

/** 被点名字段的机器名 → 词典 key（评审 enum + 降级 / 归一通道）；未知值同上
 * 原样透出。`findings[].field` 是自由文本（无枚举校验）：评审会把留痕挂在
 * 节点意图上（`intent`，真数据出现过）；未处置片段 / 评审问题转留痕 / 术语
 * 出处 / 新概念 / 新别名是降级与归一通道的留痕类别。 */
const FIELD_LABEL_KEYS: Record<string, string> = {
  teaches: 'learning.learningGraphFindingsFieldTeaches',
  assumes: 'learning.learningGraphFindingsFieldAssumes',
  mints: 'learning.learningGraphFindingsFieldMints',
  title: 'learning.learningGraphFindingsFieldTitle',
  scope: 'learning.learningGraphFindingsFieldScope',
  difficulty: 'learning.learningGraphFindingsFieldDifficulty',
  review: 'learning.learningGraphFindingsFieldReview',
  intent: 'learning.learningGraphFindingsFieldIntent',
  fragments: 'learning.learningGraphFindingsFieldFragments',
  review_problems: 'learning.learningGraphFindingsFieldReviewProblems',
  term_provenance: 'learning.learningGraphFindingsFieldTermProvenance',
  new_concepts: 'learning.learningGraphFindingsFieldNewConcepts',
  new_aliases: 'learning.learningGraphFindingsFieldNewAliases',
};

/** 机器名 → 展示文案：已知值走词典，未知值原样透出。 */
function machineLabel(t: Translate, labels: Record<string, string>, machineName: string): string {
  const key = labels[machineName];
  return key === undefined ? machineName : t(key);
}

/**
 * 评审留痕行（m1）：时间 + 节点上下文 + 字段 / 来源 / 「修补已读」标记 +
 * issue 原文 + advice 原文（仅非空时出，带「建议：」前缀）。留痕是只增审计
 * 面——这里没有编辑 / 删除 / 处置动作，也不推水位。「修补已读」只在
 * read_by_repair 为真时出现（= 修补回合读过，不是已修复）。独立成组件供
 * 渲染测试直接断言（同 MountObservationSummary）。
 */
export function StructureFindingsList({ findings }: { findings: StructureFindingView[] }) {
  const { t } = useTranslation();
  if (findings.length === 0) return null;
  return (
    <div className='flex flex-col'>
      {findings.map((finding) => (
        <div
          key={finding.seq}
          className='flex flex-wrap items-baseline gap-8px border-b border-b-solid border-b-[var(--color-border-1)] px-4px py-6px last:border-b-0'
        >
          <span className='text-11px text-t-tertiary'>{new Date(finding.ts).toLocaleString()}</span>
          {finding.node.trim() !== '' && (
            <span className='text-11px text-t-tertiary'>{finding.node}</span>
          )}
          {finding.field.trim() !== '' && (
            <Tag size='small' className='!mx-0'>
              {machineLabel(t, FIELD_LABEL_KEYS, finding.field)}
            </Tag>
          )}
          <Tag size='small' color='gray' className='!mx-0'>
            {machineLabel(t, SOURCE_LABEL_KEYS, finding.source)}
          </Tag>
          {finding.read_by_repair && (
            <Tag size='small' color='arcoblue' className='!mx-0'>
              {t('learning.learningGraphFindingsReadByRepair')}
            </Tag>
          )}
          {/* issue / advice 是长句：各占整行、原样折行（不截断——原文要读得到）。 */}
          <Text className='min-w-0 basis-full break-all text-12px'>{finding.issue}</Text>
          {finding.advice.trim() !== '' && (
            <Text type='secondary' className='min-w-0 basis-full break-all text-12px'>
              {t('learning.learningGraphFindingsAdvicePrefix')}
              {finding.advice}
            </Text>
          )}
        </div>
      ))}
    </div>
  );
}

/**
 * 评审留痕折叠段（m1）：结构图页签里独立、默认折叠的一段——标题行 = 标题 +
 * 计数 Tag（limit 截断时换「最近 N 条 · 共 M 条」，如实说没全展示）+ 未读
 * Tag（修补回合还没读过的条数）；展开 = 水位语义提示 + 行列表。纯展示：
 * 数据与错误经 props 单向下传，取数在 StructureFindingsLoader。
 *
 * 空态整段不渲染（没有留痕的课程不打扰）。读数失败例外：标题行留在场、
 * 展开见错误——段落不静默消失；此前已取到的行照旧展示（留痕只增不改，上
 * 一次的读数仍是可读的审计面），错误条置顶。
 *
 * 不放进挂载卡的「详情」折叠：挂载卡与那个折叠只在有挂载行时存在，没跑过
 * 挂载的课程（首批生长后、唤醒臂挂载前）已有留痕却会永远看不到。
 */
export function StructureFindingsSection({
  view,
  error,
}: {
  view?: StructureFindingsView | null;
  error?: string | null;
}) {
  const { t } = useTranslation();
  const findings = view?.findings ?? [];
  if (error == null && findings.length === 0) return null;
  const total = view?.total ?? 0;
  const unread = view?.unread ?? 0;
  const truncated = total > findings.length;
  return (
    <NomiCollapse bordered={false}>
      <NomiCollapse.Item
        name='structure-findings'
        header={
          <span className='flex flex-wrap items-center gap-8px'>
            <Text bold className='text-13px'>
              {t('learning.learningGraphFindingsTitle')}
            </Text>
            {findings.length > 0 && (
              <Tag size='small' color='arcoblue' className='!mx-0'>
                {truncated
                  ? t('learning.learningGraphFindingsCountTruncated', {
                      shown: findings.length,
                      total,
                    })
                  : t('learning.learningGraphFindingsCount', { count: findings.length })}
              </Tag>
            )}
            {unread > 0 && (
              <Tag size='small' color='orangered' className='!mx-0'>
                {t('learning.learningGraphFindingsUnread', { count: unread })}
              </Tag>
            )}
          </span>
        }
      >
        <div className='flex flex-col gap-6px'>
          {/* 水位 = 已读，不是已修复（map 纪律）：展开先读这一句，再看行上的
              「修补已读」标记。 */}
          {findings.length > 0 && (
            <Text type='secondary' className='text-11px leading-16px'>
              {t('learning.learningGraphFindingsReadHint')}
            </Text>
          )}
          {/* 刷新失败不清空已取到的行：错误条置顶，上次读数照旧可读。 */}
          {error != null && <Alert type='error' content={error} />}
          <StructureFindingsList findings={findings} />
        </div>
      </NomiCollapse.Item>
    </NomiCollapse>
  );
}

/**
 * 评审留痕段的取数壳（m1，同 SystemActionsSection 的自取数形态）：挂载即取
 * `GET graph/structure/findings`，并听生长 / 挂载 / 修补三组 WS 帧重取——与
 * 系统动作簿同一组帧（生长回合追加留痕，修补回合读过留痕后推水位）。basis_
 * declared 等其余帧不添行也不过水位，过滤掉避免无意义闪刷。
 *
 * 取数中不渲染任何占位（空态与未完成态同形：整段不出——默认折叠的旁路
 * 读面，不打扰）。失败走 errorMessage 单源映射：state 只存词典句，后端原始
 * 报文不进人面；此前已取到的读数保留（见 StructureFindingsSection）。
 */
export function StructureFindingsLoader({ courseId }: { courseId: string }) {
  const { t } = useTranslation();
  const [view, setView] = useState<StructureFindingsView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      learningApi
        .structureFindings(courseId)
        .then((next) => {
          if (cancelled) return;
          setView(next);
          // 后续重取成功即清掉上一次的失败态（错误不是终态）。
          setError(null);
        })
        .catch((loadError) => {
          // o1：state 只存 errorMessage 单源映射句，原始报文不进人面。
          if (!cancelled) setError(errorMessage(t, loadError));
        });
    };
    load();
    const off = ipcBridge.learning.courseGeneration.on((event) => {
      if (event.course_id !== courseId) return;
      const name = event.event ?? '';
      if (
        name.startsWith('growth_') ||
        name.startsWith('structure_mount_') ||
        name.startsWith('structure_repair_')
      ) {
        load();
      }
    });
    return () => {
      cancelled = true;
      off();
    };
  }, [courseId, t]);

  return <StructureFindingsSection view={view} error={error} />;
}
