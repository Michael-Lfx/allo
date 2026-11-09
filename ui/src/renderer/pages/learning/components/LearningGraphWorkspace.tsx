/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import {
  Alert,
  Button,
  Empty,
  Modal,
  Spin,
  Table,
  Tabs,
  Tag,
  Tooltip,
  Typography,
} from '@arco-design/web-react';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';
import { AppMessage as Message } from '@/renderer/components/notifications';
import NomiCollapse from '@/renderer/components/base/NomiCollapse';
import { lessonStatusTagColors } from '../model';
import { batchStatusMeta, buildSystemActionRows } from '../systemActions';
import { overloadedBadgeCount } from '../capacityFeedback';
import { contentColumnWidth, STANDARD_GRAPH_MAX_WIDTH } from '../layout';
import { statusLabel } from '../utils';
import {
  loadStructurePresentedWatermark,
  saveStructurePresentedWatermark,
  shouldPresentStructure,
} from '../structurePresentation';
import { useGraphGenerationEvents } from '../hooks/useGraphGenerationEvents';
import { useAutoRefill } from '../hooks/useAutoRefill';
import { learningApi } from '../api';
import { errorMessage } from '../utils';
import type {
  Activity,
  AttemptRecord,
  BasisView,
  ConceptAuditReport,
  CourseDetail,
  GraphBatchView,
  GraphConceptRowView,
  LearningStructureView,
  Lesson,
  LessonStatus,
  SystemActionView,
} from '../types';
import { LessonBlock } from './LessonStudy';
import { CourseArchivedBadge, CourseRenameButton } from './CourseCard';
import LearningModelSelector, { useLearningAutogenModel } from './LearningModelSelector';
import ContentWidthToggle, { useWideContentLayout } from './ContentWidthToggle';
import { StructureGraphTab } from './structure/StructureGraphTab';
import { DeclarationWindow } from './structure/DeclarationWindow';

const { Text, Title, Paragraph } = Typography;

/**
 * 学习图课程工作区（beta，概念网 + 生长模型 ADR-0009）：
 * - 顶条：知识结构状态（生成中 / 已生成 / 失败重试）+ 手动补充节点
 *   （就绪 x/7）；
 * - 四个视图：学习（就绪集导航 + 节点课时内容）、学习记录（批次时间线，
 *   生长史即课程史）、概念表（概念 × 教/假定 × 档位 × 跨课程来源）、
 *   结构图（知识结构读侧投影，后续落地）。
 * 没有先修边、没有锁定态——发布即就绪。
 */
const LearningGraphWorkspace: React.FC<{
  detail: CourseDetail;
  busyId: string | null;
  attemptResults: Record<string, AttemptRecord>;
  onBack: () => void;
  onProgress: (lesson: Lesson, status: LessonStatus) => void;
  onAttempt: (activity: Activity, response: unknown) => void;
  onGenerate: (lesson: Lesson) => void;
  onRefresh: () => void;
}> = ({
  detail,
  busyId,
  attemptResults,
  onBack,
  onProgress,
  onAttempt,
  onGenerate,
  onRefresh,
}) => {
  const { t } = useTranslation();
  const model = useLearningAutogenModel();
  const { wide: wideContent } = useWideContentLayout();
  const graph = detail.graph;
  const [activeTab, setActiveTab] = useState('study');

  const lessons = useMemo(
    () => detail.modules.flatMap((module) => module.lessons),
    [detail.modules]
  );
  const lessonsById = useMemo(
    () => new Map(lessons.map((lesson) => [lesson.id, lesson])),
    [lessons]
  );

  // 后台结构生成/生长的 WS 事件触发刷新：结构启动翻出「生成中」徽章、
  // 画完立即上屏、终结（完成/失败/取消）落定状态——思考化之后结构与生长
  // 都要跑分钟级，不刷新用户会以为什么都没发生。
  useGraphGenerationEvents(detail.course.id, () => onRefresh());

  // 自动补批触发②（m10）：工作区加载 → 纯读裁决 → Due 自动补一批（后台）。
  // autoPreparing 是派发窗口的诚实读数，批次运行期仍由 growth_running 表达。
  const { autoPreparing } = useAutoRefill(detail.course.id, onRefresh);

  const [selectedLessonId, setSelectedLessonId] = useState<string | null>(
    () => graph?.recommended[0] ?? null
  );

  const isRecommended = useCallback(
    (lessonId: string) => graph?.recommended.includes(lessonId) ?? false,
    [graph]
  );
  const primaryReadyId = graph?.recommended[0] ?? null;
  const primaryLesson = primaryReadyId ? lessonsById.get(primaryReadyId) ?? null : null;

  // 完成节点后推荐首位会推进：内容区自动跟随到下一个应学节点；
  // 用户手动点选不被覆盖（与普通课程工作区的跟随策略一致）。
  const lastRecommendedIdRef = React.useRef<string | null>(primaryReadyId);
  useEffect(() => {
    if (primaryReadyId && primaryReadyId !== lastRecommendedIdRef.current) {
      lastRecommendedIdRef.current = primaryReadyId;
      setSelectedLessonId(primaryReadyId);
    }
  }, [primaryReadyId]);

  // 选中的课时在前端数据里消失时（生长后课程刷新）回退到推荐首位
  const selectedLesson = useMemo(
    () =>
      (selectedLessonId ? lessonsById.get(selectedLessonId) : null) ??
      (primaryReadyId ? lessonsById.get(primaryReadyId) : null) ??
      null,
    [lessonsById, selectedLessonId, primaryReadyId]
  );

  // ── 知识结构状态（读侧投影 + 生成中/失败态）─────────────────────────────
  const [structure, setStructure] = useState<LearningStructureView | null>(null);
  const [structureError, setStructureError] = useState<string | null>(null);
  const [structureRunning, setStructureRunning] = useState(false);
  const [regenerating, setRegenerating] = useState(false);
  const [mounting, setMounting] = useState(false);
  const [repairing, setRepairing] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      learningApi
        .graphStructure(detail.course.id)
        .then((view) => {
          if (cancelled) return;
          setStructure(view);
          setStructureError(null);
          if (view.structure) setStructureRunning(false);
          presentStructureOnce(view);
        })
        .catch((loadError) => {
          if (!cancelled) {
            setStructureError(loadError instanceof Error ? loadError.message : String(loadError));
          }
        });
    };
    load();
    // 结构生成的开始/终结帧驱动「生成中」状态机，终结即重取投影。
    // 失败必须显式清 running——否则首次生成失败会永久停在「生成中…」
    // （快照仍为 null，load 里无法自行消解）。
    const off = ipcBridge.learning.courseGeneration.on((event) => {
      if (event.course_id !== detail.course.id) return;
      if (event.event === 'structure_started') setStructureRunning(true);
      if (event.event === 'structure_completed') load();
      if (event.event === 'structure_failed' || event.event === 'structure_cancelled') {
        setStructureRunning(false);
        load();
      }
    });
    return () => {
      cancelled = true;
      off();
    };
  }, [detail.course.id]);

  const regenerateStructure = useCallback(async () => {
    setRegenerating(true);
    try {
      setStructureRunning(true);
      await learningApi.regenerateGraphStructure(detail.course.id, {
        provider_id: model.choice?.provider_id,
        model: model.choice?.model,
      });
    } catch (actionError) {
      setStructureRunning(false);
      Modal.error({
        title: t('learning.learningGraphStructureFailed'),
        content: errorMessage(t, actionError),
      });
    } finally {
      setRegenerating(false);
    }
  }, [detail.course.id, model.choice, t]);

  // 结构投影重取（回合命令完成后的刷新面）；失败静默保留旧投影。
  const refetchStructure = useCallback(async () => {
    await learningApi
      .graphStructure(detail.course.id)
      .then((view) => setStructure(view))
      .catch(() => undefined);
  }, [detail.course.id]);

  // ── 结构图自动呈现（c5）：「生成完成后至少呈现给用户一次」───────────────
  // 在场臂：WS structure_completed 到达时呈现（建课跳转后生长跑完的路径）；
  // 离场臂：挂载拉到「零节点 + 有新结构」时呈现（用户在生成期间退出，回来
  // 只走 HTTP 的路径）。水位 = localStorage 持久的结构 generated_at——重生成
  // 出新结构（时间戳更新）值得再呈现一次，同一次生成绝不重复抢焦点；用户
  // 已在结构图 Tab 视为已呈现。
  const structurePresentingRef = React.useRef(false);
  const presentStructureOnce = useCallback(
    (view: LearningStructureView) => {
      const generatedAt = view.generated_at;
      if (!shouldPresentStructure(loadStructurePresentedWatermark(detail.course.id), generatedAt)) {
        return;
      }
      if (generatedAt !== null) saveStructurePresentedWatermark(detail.course.id, generatedAt);
      if (structurePresentingRef.current) return;
      structurePresentingRef.current = true;
      setActiveTab((current) => {
        if (current === 'structure') return current;
        Message.info(t('learning.learningGraphStructureReadyToast'));
        return 'structure';
      });
    },
    [detail.course.id, t]
  );

  // 离场兜底臂（见 presentStructureOnce 注释）：仅当本课没有任何节点（零节
  // 点 = 首批未生长 = 生成期最可能离场的形态）且拉到新结构时触发一次。

  // 结构挂载回合（j08）：同步命令，完成后重取投影（挂载三态段随之刷新）。
  const mountStructure = useCallback(async () => {
    setMounting(true);
    try {
      await learningApi.mountStructure(detail.course.id);
      await refetchStructure();
    } catch (actionError) {
      Modal.error({
        title: t('learning.learningGraphMountFailed'),
        content: errorMessage(t, actionError),
      });
    } finally {
      setMounting(false);
    }
  }, [detail.course.id, refetchStructure, t]);

  // 结构修补回合（j09）：按留痕与观察修补结构；皆空时后端如实 skipped。
  const repairStructure = useCallback(async () => {
    setRepairing(true);
    try {
      const summary = await learningApi.repairStructure(detail.course.id);
      if (summary.status === 'noop') {
        Modal.info({ title: t('learning.learningGraphRepairNoop') });
      }
      await refetchStructure();
    } catch (actionError) {
      Modal.error({
        title: t('learning.learningGraphRepairFailed'),
        content: errorMessage(t, actionError),
      });
    } finally {
      setRepairing(false);
    }
  }, [detail.course.id, refetchStructure, t]);

  // ── 已会声明（Declared Known，m2）：读数 + 一次性声明（c3 起产品面无撤销
  // 入口；后端 DELETE /basis 留作纠错通道）──────────────────────────────
  const [basis, setBasis] = useState<BasisView | null>(null);
  const [declaringBasis, setDeclaringBasis] = useState(false);
  // 草稿勾选（c1）：工作区单持，学习 Tab 声明窗口与结构图 Tab 两个过门自检
  // 挂载点共享同一份——切 Tab 不丢；声明成功后清空，失败保留。
  const [basisDraftChecked, setBasisDraftChecked] = useState<string[]>([]);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      learningApi
        .basisView(detail.course.id)
        .then((view) => {
          if (!cancelled) setBasis(view);
        })
        .catch(() => {
          // 读数缺席不挡结构 Tab：保留 null（声明入口按未声明渲染）。
          if (!cancelled) setBasis(null);
        });
    };
    load();
    // 声明/生长经 WS course-generation 流（basis_declared 复用既有事件面）
    // 驱动重拉；后端纠错通道撤销（basis_revoked）同样会被读数接住。
    const off = ipcBridge.learning.courseGeneration.on((event) => {
      if (event.course_id === detail.course.id) load();
    });
    return () => {
      cancelled = true;
      off();
    };
  }, [detail.course.id]);

  const declareBasis = useCallback(
    async (doors: string[]) => {
      setDeclaringBasis(true);
      try {
        const result = await learningApi.declareBasis(detail.course.id, doors);
        setBasisDraftChecked([]);
        // 声明反馈走非阻塞 toast（c2）：阻塞弹窗叠在冻结展示上是二次打断；
        // 失败保持 Modal——需要用户看见并处置。
        if (result.noop) {
          Message.info(t('learning.learningGraphBasisDeclareNoop'));
        } else {
          Message.success(
            t('learning.learningGraphBasisDeclareDone', {
              doors: result.doors.join('、'),
              blocks: result.blocks.join('、'),
              minted: result.minted,
              reused: result.reused,
            })
          );
        }
        const view = await learningApi.basisView(detail.course.id);
        setBasis(view);
        // 声明改就绪集/推荐集（声明概念按能教计）：立即重拉课程详情，调度
        // 效果当场可见，而不是等下次无关刷新。
        onRefresh();
      } catch (actionError) {
        Modal.error({
          title: t('learning.learningGraphBasisDeclareFailed'),
          content: errorMessage(t, actionError),
        });
      } finally {
        setDeclaringBasis(false);
      }
    },
    [detail.course.id, onRefresh, t]
  );

  // ── 手动补充节点 ────────────────────────────────────────────────────────
  const [growing, setGrowing] = useState(false);
  const grow = useCallback(async () => {
    setGrowing(true);
    try {
      await learningApi.growGraph(detail.course.id);
      onRefresh();
    } catch (actionError) {
      Modal.error({ title: t('learning.learningGraphGrowFailed'), content: errorMessage(t, actionError) });
    } finally {
      setGrowing(false);
    }
  }, [detail.course.id, onRefresh, t]);

  if (!graph) {
    return <Empty description={t('learning.learningGraphEmpty')} />;
  }
  const readyCount = graph.ready_count;
  const readyStockFull = readyCount >= graph.ready_target;
  // 声明窗口（e3）：零节点（结构完成、首批未生长）时学习 Tab 落在门口
  // 声明画面——生长不再自动抢跑，首批由「开始学习」显式触发。
  const zeroNodes = lessons.length === 0;

  // 离场兜底臂挂载（零节点 + 拉到新结构 → 呈现一次；见 presentStructureOnce）。
  const zeroNodesAutoPresentedRef = React.useRef(false);
  useEffect(() => {
    if (!zeroNodes || zeroNodesAutoPresentedRef.current || structure === null) return;
    zeroNodesAutoPresentedRef.current = true;
    presentStructureOnce(structure);
  }, [zeroNodes, structure, presentStructureOnce]);

  return (
    <div className='app-page-shell h-full w-full box-border overflow-y-auto'>
      <div
        className={`mx-auto flex h-full w-full flex-col gap-10px ${contentColumnWidth(wideContent, STANDARD_GRAPH_MAX_WIDTH)}`}
      >
      {/* 头部：返回 + 标题/Beta + 学习目标 + 模型选择（对齐传统课程工作区） */}
      <Button type='text' className='self-start !px-0' onClick={onBack}>
        {t('learning.back')}
      </Button>
      <div className='flex flex-wrap items-start justify-between gap-12px'>
        <div className='min-w-0 flex-1'>
          <div className='flex items-center gap-8px'>
            <Title heading={3} className='!m-0'>
              {detail.course.title}
            </Title>
            <Tag size='small' color='orangered' className='!mx-0 shrink-0'>
              {t('learning.learningGraphBeta')}
            </Tag>
            {!detail.course.enabled && <CourseArchivedBadge />}
            {/* 容量聚合（y07）：图课程同样可见「N 节超载」；0 = 零版面
                （内容级纯读计数，不暴露任何用户的选择） */}
            {overloadedBadgeCount(detail.overloaded_lessons) !== null && (
              <Tooltip content={t('learning.capacityOverloadedHint')}>
                <Tag color='orange' className='!mx-0 shrink-0'>
                  {t('learning.capacityOverloadedCount', {
                    count: detail.overloaded_lessons,
                  })}
                </Tag>
              </Tooltip>
            )}
          </div>
          {graph.goal.trim() !== '' && (
            <Paragraph className='!mb-0 !mt-4px text-t-secondary'>{graph.goal}</Paragraph>
          )}
        </div>
        <div className='flex shrink-0 items-center gap-8px'>
          {!detail.course.enabled && (
            <Button
              size='small'
              type='primary'
              onClick={() => {
                learningApi
                  .restoreCourse(detail.course.id)
                  .then(() => {
                    Message.success(t('learning.courseRestoreDone'));
                    onRefresh();
                  })
                  .catch((actionError) => Message.error(errorMessage(t, actionError)));
              }}
            >
              {t('learning.courseRestore')}
            </Button>
          )}
          <CourseRenameButton course={detail.course} onChanged={onRefresh} size='small' />
          <ContentWidthToggle />
          <LearningModelSelector
            choice={model.choice}
            onChange={(choice) => void model.setChoice(choice)}
            size='small'
          />
        </div>
      </div>

      {/* 知识结构状态 + 补充节点（生长控制条） */}
      <div className='flex flex-col gap-8px rounded-10px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-12px'>
        <div className='flex flex-wrap items-center gap-8px'>
          <Text bold className='text-13px'>
            {t('learning.learningGraphStructureTitle')}
          </Text>
          {structureError ? (
            <Text type='secondary' className='text-12px'>
              {t('learning.learningGraphStructureLoadFailed')}
            </Text>
          ) : structure === null ? (
            <Spin size={12} />
          ) : structureRunning ? (
            <Tag size='small' color='processing' className='!mx-0'>
              {t('learning.learningGraphStructureGenerating')}
            </Tag>
          ) : structure.structure ? (
            <Text type='secondary' className='text-12px'>
              {t('learning.learningGraphStructureSummary', {
                spines: structure.structure.spines.length,
                blocks: structure.structure.blocks.length,
                doors: structure.terrain?.doors.length ?? 0,
              })}
              {structure.generated_at
                ? ` · ${new Date(structure.generated_at).toLocaleString()}`
                : ''}
            </Text>
          ) : (
            <Text type='secondary' className='text-12px'>
              {t('learning.learningGraphStructureMissing')}
            </Text>
          )}
          <Button
            size='mini'
            type='text'
            loading={regenerating}
            disabled={regenerating || structureRunning || busyId !== null}
            onClick={() => void regenerateStructure()}
          >
            {structure?.structure
              ? t('learning.learningGraphStructureRegenerate')
              : t('learning.learningGraphStructureGenerate')}
          </Button>
          <span className='flex-1' />
          {autoPreparing && (
            <Tag size='small' color='processing' className='!mx-0'>
              {t('learning.learningGraphAutoPreparing')}
            </Tag>
          )}
          {graph.growth_running && (
            <Tag size='small' color='processing' className='!mx-0'>
              {t('learning.learningGraphGrowing')}
            </Tag>
          )}
          <Button
            size='small'
            type='primary'
            loading={growing}
            disabled={growing || busyId !== null}
            onClick={() => void grow()}
          >
            {readyStockFull
              ? t('learning.learningGraphGrowReady')
              : t('learning.learningGraphGrow', {
                  ready: readyCount,
                  target: graph.ready_target,
                })}
          </Button>
        </div>
        {/* 尚未生成时的说明（生成按钮在上一行；生成中不显示） */}
        {!structureRunning && structure !== null && !structure.structure && !structureError && (
          <Text type='secondary' className='text-12px'>
            {t('learning.learningGraphStructureHint')}
          </Text>
        )}
        {/* R 软闸建议条：结构就绪但供给节点已遗忘（ADR-0009 Amendment 1） */}
        {graph.blocked.map((blocked) => (
          <Alert
            key={blocked.lesson_id}
            type='warning'
            content={t('learning.learningGraphBlockedAdvice', {
              title: blocked.title,
              supplier: blocked.supplier_title,
              percent: Math.round(blocked.r * 100),
              due: blocked.due_count,
            })}
          />
        ))}
      </div>

      <Tabs activeTab={activeTab} onChange={setActiveTab} type='line' className='flex min-h-0 flex-1 flex-col [&_.arco-tabs-content]:flex-1 [&_.arco-tabs-content]:pt-8px'>
        <Tabs.TabPane key='study' title={t('learning.learningGraphTabStudy')}>
          {zeroNodes ? (
            <DeclarationWindow
              view={structure}
              structureRunning={structureRunning}
              structureError={structureError}
              basis={basis}
              declaring={declaringBasis}
              basisChecked={basisDraftChecked}
              onBasisCheckedChange={setBasisDraftChecked}
              onDeclare={(doors) => void declareBasis(doors)}
              starting={growing}
              growthRunning={graph.growth_running}
              onStart={() => void grow()}
            />
          ) : (
          <div className='flex min-h-0 gap-12px'>
            <aside className='flex w-280px shrink-0 flex-col gap-10px'>
              {/* 主推荐卡：当前应学的节点——最重要的学习入口，点击即进入学习 */}
              <div
                className='cursor-pointer rounded-10px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-12px transition-colors hover:border-[var(--color-primary-6)]'
                onClick={() => primaryReadyId && setSelectedLessonId(primaryReadyId)}
              >
                <Text bold className='text-13px'>
                  {t('learning.learningGraphContinueTitle')}
                </Text>
                {primaryReadyId ? (
                  <>
                    <div className='mt-6px text-14px font-600 leading-20px text-[var(--color-text-1)]'>
                      {primaryLesson?.title}
                    </div>
                    <div className='mt-4px text-11px text-t-tertiary'>
                      {t('learning.learningGraphNodeMinutes', {
                        min: primaryLesson?.est ?? 10,
                      })}
                    </div>
                    {primaryLesson && !primaryLesson.generated && (
                      <div className='mt-10px'>
                        <Button
                          type='primary'
                          size='small'
                          loading={busyId === primaryLesson.id}
                          onClick={(event) => {
                            event.stopPropagation();
                            onGenerate(primaryLesson);
                          }}
                        >
                          {t('learning.learningGraphGenerateContent')}
                        </Button>
                      </div>
                    )}
                  </>
                ) : (
                  <Empty description={t('learning.learningGraphReadyEmpty')} />
                )}
              </div>
              {/* 其他可学节点 */}
              {graph.recommended.slice(1).map((lessonId) => {
                const lesson = lessonsById.get(lessonId);
                if (!lesson) return null;
                return (
                  <button
                    key={lessonId}
                    type='button'
                    onClick={() => setSelectedLessonId(lessonId)}
                    className={`rd-6px flex cursor-pointer items-center justify-between gap-8px border-none bg-transparent px-6px py-6px text-left font-inherit text-13px transition-colors hover:bg-[var(--color-fill-1)] ${
                      selectedLessonId === lessonId
                        ? 'bg-primary-1 font-500 text-primary-6'
                        : 'text-[var(--color-text-1)]'
                    }`}
                  >
                    <span className='min-w-0 truncate'>{lesson.title}</span>
                    <span className='shrink-0 text-11px text-t-tertiary'>
                      {t('learning.learningGraphNodeMinutes', { min: lesson.est })}
                    </span>
                  </button>
                );
              })}
            </aside>

            {/* 中央内容区：选中节点的完整课时学习（正文/练习题/完成/追加练习） */}
            <section className='flex min-w-0 flex-1 flex-col overflow-hidden rounded-10px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)]'>
              {!selectedLesson ? (
                <div className='flex h-full flex-1 items-center justify-center'>
                  <Empty description={t('learning.learningGraphContentEmpty')} />
                </div>
              ) : (
                <div className='h-full overflow-y-auto p-16px'>
                  <div className='flex flex-wrap items-center gap-8px'>
                    <Title heading={4} className='!m-0'>
                      {selectedLesson.title}
                    </Title>
                    <Tag
                      size='small'
                      color={lessonStatusTagColors[selectedLesson.status]}
                      className='!mx-0'
                    >
                      {statusLabel(selectedLesson.status, t)}
                    </Tag>
                    {isRecommended(selectedLesson.id) && (
                      <Tag size='small' color='gold' className='!mx-0'>
                        {t('learning.learningGraphRecommendedShort')}
                      </Tag>
                    )}
                    <span className='flex-1' />
                    <Button
                      size='small'
                      disabled={busyId !== null}
                      onClick={() =>
                        onProgress(
                          selectedLesson,
                          selectedLesson.status === 'skipped' ? 'not_started' : 'skipped'
                        )
                      }
                    >
                      {selectedLesson.status === 'skipped'
                        ? t('learning.learningGraphUnskip')
                        : t('learning.learningGraphSkip')}
                    </Button>
                  </div>
                  <div className='mt-12px'>
                    <LessonBlock
                      lesson={selectedLesson}
                      sourceKbId={detail.course.source_kb_id}
                      busyId={busyId}
                      attemptResults={attemptResults}
                      onProgress={onProgress}
                      onAttempt={onAttempt}
                      onGenerate={(target) => onGenerate(target)}
                      onRefresh={onRefresh}
                    />
                  </div>
                </div>
              )}
            </section>
          </div>
          )}
        </Tabs.TabPane>
        <Tabs.TabPane key='history' title={t('learning.learningGraphTabHistory')}>
          <GraphHistory courseId={detail.course.id} enrollmentKey={detail.enrollment_id} />
        </Tabs.TabPane>
        <Tabs.TabPane key='concepts' title={t('learning.learningGraphTabConcepts')}>
          <ConceptTable courseId={detail.course.id} />
        </Tabs.TabPane>
        <Tabs.TabPane key='structure' title={t('learning.learningGraphTabStructure')}>
          <StructureGraphTab
            view={structure}
            basis={basis}
            onMount={() => void mountStructure()}
            mounting={mounting}
            onRepair={() => void repairStructure()}
            repairing={repairing}
            basisChecked={basisDraftChecked}
            onBasisCheckedChange={setBasisDraftChecked}
            onDeclareBasis={(doors) => void declareBasis(doors)}
            declaringBasis={declaringBasis}
          />
        </Tabs.TabPane>
      </Tabs>
      </div>
    </div>
  );
};

/** 学习记录：批次时间线（倒序）——生长史即课程史；顶部「系统动作」折叠节
 * 是自主/后台动作的终态留痕（y04：进行中看任务 pill，终态看账本）。 */
const GraphHistory: React.FC<{ courseId: string; enrollmentKey: string | null }> = ({
  courseId,
}) => {
  const { t } = useTranslation();
  const [batches, setBatches] = useState<GraphBatchView[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      learningApi
        .graphHistory(courseId)
        .then((view) => {
          if (!cancelled) setBatches(view.batches);
        })
        .catch((loadError) => {
          if (!cancelled) setError(loadError instanceof Error ? loadError.message : String(loadError));
        });
    };
    load();
    // 只有生长回合改批次时间线（开始落 pending 行、终结落定状态）——生长
    // 全程之外的帧（basis_declared 等）与本账本无关，过滤掉避免无意义闪刷
    // （c2：声明不该让历史列表重拉）。
    const off = ipcBridge.learning.courseGeneration.on((event) => {
      if (event.course_id !== courseId) return;
      if ((event.event ?? '').startsWith('growth_')) load();
    });
    return () => {
      cancelled = true;
      off();
    };
  }, [courseId]);

  if (error) {
    return <Alert type='error' content={`${t('learning.loadFailed')}: ${error}`} />;
  }
  if (batches === null) {
    return (
      <div className='flex justify-center py-32px'>
        <Spin />
      </div>
    );
  }
  return (
    <div className='flex flex-col gap-12px overflow-y-auto pb-16px'>
      <SystemActionsSection courseId={courseId} />
      {batches.length === 0 ? (
        <Empty description={t('learning.learningGraphHistoryEmpty')} />
      ) : (
        batches.map((batch) => {
          const statusMeta = batchStatusMeta(batch.status);
          return (
            <div
              key={batch.batch_id}
              className='rounded-10px border-1 border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-12px'
            >
              <div className='flex flex-wrap items-baseline gap-8px'>
                <Text bold className='text-13px'>
                  {t('learning.learningGraphBatchLabel', { seq: batch.seq })}
                </Text>
                {statusMeta && (
                  <Tag size='small' color={statusMeta.color} className='!mx-0'>
                    {t(statusMeta.labelKey)}
                  </Tag>
                )}
                <Text type='secondary' className='text-11px'>
                  {new Date(batch.created_at).toLocaleString()}
                </Text>
                {batch.note.trim() !== '' && (
                  <Text type='secondary' className='text-12px'>
                    {batch.note}
                  </Text>
                )}
              </div>
              <div className='mt-8px flex flex-col'>
                {batch.nodes.map((node) => (
                  <div
                    key={node.lesson_id}
                    className='flex items-center justify-between gap-8px rd-6px px-6px py-6px hover:bg-[var(--color-fill-1)]'
                  >
                    <span className='min-w-0 truncate text-13px text-[var(--color-text-1)]'>
                      {node.title}
                    </span>
                    <span className='flex shrink-0 items-center gap-8px text-11px text-t-tertiary'>
                      <span>{t('learning.learningGraphNodeMinutes', { min: node.est })}</span>
                      <Tag
                        size='small'
                        color={lessonStatusTagColors[node.status]}
                        className='!mx-0'
                      >
                        {statusLabel(node.status, t)}
                      </Tag>
                      {node.completed_at ? (
                        <span>{new Date(node.completed_at).toLocaleDateString()}</span>
                      ) : null}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          );
        })
      )}
    </div>
  );
};

/** 系统动作（y04）：折叠节的读侧。行 = 时间 + 类型 + 触发源 + basis + 结果
 * 徽标；失败红色 role=alert 带 reason；空态「暂无系统动作」。 */
const SystemActionsSection: React.FC<{ courseId: string }> = ({ courseId }) => {
  const { t } = useTranslation();
  const [actions, setActions] = useState<SystemActionView[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      learningApi
        .systemActions(courseId)
        .then((view) => {
          if (!cancelled) setActions(view);
        })
        .catch((loadError) => {
          if (!cancelled)
            setError(loadError instanceof Error ? loadError.message : String(loadError));
        });
    };
    load();
    // 账本行只由生长回合（auto_refill 补批）与结构维护回合（挂载/修补）写
    // ——只听这三组帧（c2）：basis_declared 等其余事件不添行，不过滤会
    // 假刷新。
    const off = ipcBridge.learning.courseGeneration.on((event) => {
      if (event.course_id !== courseId) return;
      const name = event.event ?? '';
      if (name.startsWith('growth_') || name.startsWith('structure_mount_') || name.startsWith('structure_repair_')) {
        load();
      }
    });
    return () => {
      cancelled = true;
      off();
    };
  }, [courseId]);

  const model = actions ? buildSystemActionRows(actions) : null;
  return (
    <NomiCollapse bordered={false}>
      <NomiCollapse.Item
        name='system-actions'
        header={<span className='text-12px'>{t('learning.learningGraphSystemActions')}</span>}
      >
        {error ? (
          <Alert type='error' content={`${t('learning.loadFailed')}: ${error}`} />
        ) : !model ? (
          <div className='flex justify-center py-16px'>
            <Spin />
          </div>
        ) : model.empty ? (
          <Empty description={t('learning.learningGraphSystemActionsEmpty')} />
        ) : (
          <div className='flex flex-col'>
            {model.rows.map((row) => (
              <div
                key={row.key}
                role={row.resultRole}
                className='flex flex-wrap items-baseline gap-8px border-b border-b-solid border-b-[var(--color-border-1)] px-4px py-6px last:border-b-0'
              >
                <span className='text-11px text-t-tertiary'>{new Date(row.at).toLocaleString()}</span>
                <Tag size='small' className='!mx-0'>
                  {t(row.kindKey)}
                </Tag>
                <span className='text-11px text-t-tertiary'>{t(row.triggerKey)}</span>
                {row.basis.trim() !== '' && (
                  <span className='text-11px text-t-tertiary'>{row.basis}</span>
                )}
                <span className='flex-1' />
                <Tag size='small' color={row.resultColor} className='!mx-0'>
                  {t(row.resultKey)}
                </Tag>
                {row.reason.trim() !== '' && (
                  <Text type='error' className='min-w-0 basis-full truncate text-11px'>
                    {row.reason}
                  </Text>
                )}
              </div>
            ))}
          </div>
        )}
      </NomiCollapse.Item>
    </NomiCollapse>
  );
};

/** 概念表：本课程涉及的概念（教/假定 × 档位 × 节点）+ 跨课程来源 + 定义审计
 * 行级标记 + 概念定义（c6：判断「是不是这个意思」的一手材料，行内常驻）。
 * 审计与行表是两个请求两份错误（c6）：审计失败只降级审计列，不打翻整表。 */
const ConceptTable: React.FC<{ courseId: string }> = ({ courseId }) => {
  const { t } = useTranslation();
  const [rows, setRows] = useState<GraphConceptRowView[] | null>(null);
  const [audit, setAudit] = useState<ConceptAuditReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [auditUnavailable, setAuditUnavailable] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      learningApi
        .graphConcepts(courseId)
        .then((view) => {
          if (!cancelled) {
            setRows(view);
            setError(null);
          }
        })
        .catch((loadError) => {
          if (!cancelled) setError(loadError instanceof Error ? loadError.message : String(loadError));
        });
      // 定义审计（m9 只读）：与行表并行拉取；失败只降级审计列并明示
      // （审计缺席是静默误导——缺口看不见不等于没有缺口），不打翻概念表。
      learningApi
        .conceptAudit(courseId)
        .then((report) => {
          if (!cancelled) {
            setAudit(report);
            setAuditUnavailable(false);
          }
        })
        .catch(() => {
          if (!cancelled) setAuditUnavailable(true);
        });
    };
    load();
    // 生长落库会铸新概念入登记表、结构回合改节点引用、声明给概念打已会标
    // ——听这三组（c2）；其余帧（如 basis_revoked，UI 已无入口）不重拉。
    const off = ipcBridge.learning.courseGeneration.on((event) => {
      if (event.course_id !== courseId) return;
      const name = event.event ?? '';
      if (name.startsWith('growth_') || name.startsWith('structure_') || name === 'basis_declared') {
        load();
      }
    });
    return () => {
      cancelled = true;
      off();
    };
  }, [courseId]);

  // 行级标记索引（concept_id → 缺口片段 / 自指 / 互指对手名——互指可多组，
  // 收集全部而不是只剩最后一个 partner（c6 修覆盖写 bug））。
  const auditMarkers = useMemo(() => {
    const fragments = new Map<string, string[]>();
    const selfRefs = new Set<string>();
    const mutualWith = new Map<string, string[]>();
    if (audit) {
      for (const entry of audit.flagged) fragments.set(entry.concept_id, entry.fragments);
      for (const entry of audit.self_refs) selfRefs.add(entry.concept_id);
      for (const pair of audit.mutual_cycles) {
        const aPartners = mutualWith.get(pair.a_concept_id) ?? [];
        aPartners.push(pair.b_name);
        mutualWith.set(pair.a_concept_id, aPartners);
        const bPartners = mutualWith.get(pair.b_concept_id) ?? [];
        bPartners.push(pair.a_name);
        mutualWith.set(pair.b_concept_id, bPartners);
      }
    }
    return { fragments, selfRefs, mutualWith };
  }, [audit]);

  if (error) {
    return <Alert type='error' content={`${t('learning.loadFailed')}: ${error}`} />;
  }
  if (rows === null) {
    return (
      <div className='flex justify-center py-32px'>
        <Spin />
      </div>
    );
  }
  if (rows.length === 0) {
    return <Empty description={t('learning.learningGraphConceptsEmpty')} />;
  }
  const renderAuditMarkers = (row: GraphConceptRowView) => {
    const fragments = auditMarkers.fragments.get(row.concept_id) ?? [];
    const isSelfRef = auditMarkers.selfRefs.has(row.concept_id);
    const mutualPartners = auditMarkers.mutualWith.get(row.concept_id) ?? [];
    if (fragments.length === 0 && !isSelfRef && mutualPartners.length === 0) {
      return <span className='text-12px text-t-tertiary'>—</span>;
    }
    return (
      <div className='flex flex-wrap items-center gap-4px'>
        {isSelfRef && (
          <Tooltip content={t('learning.learningGraphAuditSelfRefTooltip')}>
            <Tag size='small' color='red' className='!mx-0'>
              {t('learning.learningGraphAuditSelfRef')}
            </Tag>
          </Tooltip>
        )}
        {mutualPartners.map((partner) => (
          <Tooltip
            key={partner}
            content={t('learning.learningGraphAuditMutualTooltip', { name: partner })}
          >
            <Tag size='small' color='purple' className='!mx-0'>
              {t('learning.learningGraphAuditMutual', { name: partner })}
            </Tag>
          </Tooltip>
        ))}
        {fragments.length > 0 && (
          <Tooltip
            content={t('learning.learningGraphAuditFragmentsTooltip', {
              fragments: fragments.join('、'),
            })}
          >
            <Tag size='small' color='orange' className='!mx-0'>
              {t('learning.learningGraphAuditFragments', { count: fragments.length })}
            </Tag>
          </Tooltip>
        )}
      </div>
    );
  };
  return (
    <div className='flex flex-col gap-6px'>
      {auditUnavailable && (
        <Text type='warning' className='text-12px leading-18px'>
          {t('learning.learningGraphAuditUnavailable')}
        </Text>
      )}
      <Table
        size='small'
        border={{ wrapper: true, cell: true }}
        pagination={false}
        scroll={{ x: true }}
        data={rows}
        rowKey='concept_id'
        columns={[
        {
          title: t('learning.learningGraphConceptCanonical'),
          dataIndex: 'canonical',
          width: 200,
          render: (_: unknown, row: GraphConceptRowView) => (
            <div className='min-w-0'>
              <div className='flex items-center gap-4px'>
                <span className='min-w-0 truncate font-500 text-[var(--color-text-1)]'>
                  {row.canonical}
                </span>
                {row.source === 'basis-declaration' && (
                  <Tag size='small' color='green' className='!mx-0 shrink-0'>
                    {t('learning.learningGraphConceptSourceDeclaration')}
                  </Tag>
                )}
              </div>
              {/* 概念定义常驻副行（c6）：两行截断 + 悬浮看全文——判断
                  「是不是这个意思」不必离开表格。 */}
              {row.definition.trim() !== '' && (
                <Tooltip content={row.definition}>
                  <div className='mt-2px text-11px leading-16px text-t-secondary [display:-webkit-box] [-webkit-box-orient:vertical] [-webkit-line-clamp:2] overflow-hidden'>
                    {row.definition}
                  </div>
                </Tooltip>
              )}
              {row.aliases.length > 0 && (
                <div className='truncate text-11px text-t-tertiary'>
                  {row.aliases
                    .map((alias) =>
                      alias.definition ? `${alias.name}（${alias.definition}）` : alias.name
                    )
                    .join('、')}
                </div>
              )}
            </div>
          ),
        },
        {
          title: t('learning.learningGraphConceptRefs'),
          dataIndex: 'refs',
          render: (_: unknown, row: GraphConceptRowView) => (
            <div className='flex flex-col gap-2px'>
              {row.refs.length === 0 && row.declared && (
                <span className='text-12px text-t-tertiary'>
                  {t('learning.learningGraphConceptDeclaredOnly')}
                </span>
              )}
              {row.refs.map((reference) => (
                <div key={`${reference.lesson_id}:${reference.role}`} className='flex items-center gap-6px text-12px'>
                  <Tag
                    size='small'
                    color={reference.role === 'teaches' ? 'green' : 'orange'}
                    className='!mx-0 shrink-0'
                  >
                    {reference.role === 'teaches'
                      ? t('learning.learningGraphRoleTeaches', {
                          tier: t(`learning.learningGraphTier_${reference.tier}`),
                        })
                      : t('learning.learningGraphRoleAssumes', {
                          tier: t(`learning.learningGraphTier_${reference.tier}`),
                        })}
                  </Tag>
                  <span className='min-w-0 truncate text-[var(--color-text-1)]'>{reference.title}</span>
                  {reference.ref_name && reference.ref_name !== row.canonical && (
                    <span className='shrink-0 text-11px text-t-tertiary'>
                      {t('learning.learningGraphRefAs', { name: reference.ref_name })}
                    </span>
                  )}
                  <span className='shrink-0 text-11px text-t-tertiary'>
                    {statusLabel(reference.status, t)}
                  </span>
                </div>
              ))}
            </div>
          ),
        },
        {
          title: t('learning.learningGraphConceptOtherCourses'),
          dataIndex: 'other_courses',
          width: 180,
          render: (_: unknown, row: GraphConceptRowView) =>
            row.other_courses.length > 0 ? (
              <span className='text-12px text-t-secondary'>
                {row.other_courses.map((course) => course.title).join('、')}
              </span>
            ) : (
              <span className='text-12px text-t-tertiary'>—</span>
            ),
        },
        {
          title: t('learning.learningGraphConceptAudit'),
          dataIndex: 'audit',
          width: 200,
          render: (_: unknown, row: GraphConceptRowView) =>
            auditUnavailable ? (
              <span className='text-12px text-t-tertiary'>—</span>
            ) : (
              renderAuditMarkers(row)
            ),
        },
        ]}
      />
    </div>
  );
};

export default LearningGraphWorkspace;
