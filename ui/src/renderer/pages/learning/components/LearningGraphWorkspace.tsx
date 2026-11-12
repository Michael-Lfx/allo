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
import { useNavigate } from 'react-router-dom';
import { ipcBridge } from '@/common';
import { BackendHttpError } from '@/common/adapter/httpBridge';
import { AppMessage as Message } from '@/renderer/components/notifications';
import NomiCollapse from '@/renderer/components/base/NomiCollapse';
import { lessonStatusTagColors } from '../model';
import { batchStatusMeta, buildSystemActionRows } from '../systemActions';
import { overloadedBadgeCount } from '../capacityFeedback';
import { contentColumnWidth, STANDARD_GRAPH_MAX_WIDTH } from '../layout';
import { statusLabel, errorMessage, isGenerationConflict } from '../utils';
import {
  clearInFlight,
  inFlightIdsWithPrefix,
  isInFlight,
  markInFlight,
} from '../learningInFlight';
import {
  loadStructurePresentedWatermark,
  saveStructurePresentedWatermark,
  shouldPresentStructure,
} from '../structurePresentation';
import {
  hasActiveStructureRow,
  startStructureReadoutPolling,
  structureBadgeCopy,
  type StructureReadout,
} from '../structureReadout';
import { useGraphGenerationEvents } from '../hooks/useGraphGenerationEvents';
import { useAutoRefill } from '../hooks/useAutoRefill';
import { learningApi } from '../api';
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
import { QuizEntryCard } from './QuizEntryCard';
import { CourseArchivedBadge, CourseRenameButton } from './CourseCard';
import LearningModelSelector, { useLearningAutogenModel } from './LearningModelSelector';
import ContentWidthToggle, { useWideContentLayout } from './ContentWidthToggle';
import { StructureGraphTab } from './structure/StructureGraphTab';
import { DeclarationWindow } from './structure/DeclarationWindow';

const { Text, Title, Paragraph } = Typography;

/** 顶条在跑态徽章（o6 顶条单一状态；F7 去重）：同一枚 processing Tag 渲染
 * 结构生成（v2 实时读数徽章）/ 生长 / 自动准备三种态——文案由调用方渲染，
 * 数据源（structureBadge 等）留在调用处，徽章只负责外观。 */
const ProcessingStateTag: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <Tag size='small' color='processing' className='!mx-0'>
    {children}
  </Tag>
);

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
  /** 测验进度刷新令牌（u6）：与 CourseWorkspace 同款——进页初值，会话
   * 结束/冲突回调后 index.tsx bump，QuizEntryCard 随之重拉 N/K。 */
  quizProgressToken: number;
  onBack: () => void;
  onProgress: (lesson: Lesson, status: LessonStatus) => void;
  onAttempt: (activity: Activity, response: unknown) => void;
  onGenerate: (lesson: Lesson) => void;
  /** u6 接线位：打开测验会话容器（index.tsx 同步 setState；409 边界态
   * 归 QuizSession 容器处理，入口卡零 loading 态）。 */
  onStartQuiz: () => void;
  onRefresh: () => void;
}> = ({
  detail,
  busyId,
  attemptResults,
  quizProgressToken,
  onBack,
  onProgress,
  onAttempt,
  onGenerate,
  onStartQuiz,
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
  const { autoPreparing } = useAutoRefill(detail.course.id, onRefresh, model.choice);

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
  // WS 帧驱动的运行旗（best-effort，不重放——刷新即丢）；对外呈现用下方
  // 与注册表快照行取并集后的 structureRunning。
  const [structureRunningWs, setStructureRunningWs] = useState(false);
  const [regenerating, setRegenerating] = useState(false);
  const [mounting, setMounting] = useState(false);
  const [repairing, setRepairing] = useState(false);
  // 快照轮询终态的接棒柄：指向下方 WS 效应内的 load 闭包（保留课程切换的
  // cancelled 守卫——装载路径只此一份，快照终态面不另写第二条）。
  const structureLoadRef = React.useRef<() => void>(() => undefined);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      learningApi
        .graphStructure(detail.course.id)
        .then((view) => {
          if (cancelled) return;
          setStructure(view);
          setStructureError(null);
          if (view.structure) setStructureRunningWs(false);
          presentStructureOnce(view);
        })
        .catch((loadError) => {
          if (!cancelled) {
            // o1：state 只存 errorMessage 单源映射句，原始报文不进人面。
            setStructureError(errorMessage(t, loadError));
          }
        });
    };
    structureLoadRef.current = load;
    load();
    // 结构生成的开始/终结帧驱动「生成中」状态机，终结即重取投影。
    // 失败必须显式清 running——否则首次生成失败会永久停在「生成中…」
    // （快照仍为 null，load 里无法自行消解）。
    const off = ipcBridge.learning.courseGeneration.on((event) => {
      if (event.course_id !== detail.course.id) return;
      if (event.event === 'structure_started') setStructureRunningWs(true);
      if (event.event === 'structure_completed') load();
      if (event.event === 'structure_failed' || event.event === 'structure_cancelled') {
        setStructureRunningWs(false);
        load();
      }
    });
    return () => {
      cancelled = true;
      off();
    };
  }, [detail.course.id, t]);

  const regenerateStructure = useCallback(async () => {
    setRegenerating(true);
    try {
      setStructureRunningWs(true);
      await learningApi.regenerateGraphStructure(detail.course.id, {
        provider_id: model.choice?.provider_id,
        model: model.choice?.model,
      });
    } catch (actionError) {
      setStructureRunningWs(false);
      // g3 冲突显性化：生成进行中重复触发 = 后端 409 → 非阻塞 toast 明确
      // 反馈（此前 200 kicked=true 静默吞掉，「生成中」Tag 卡死到刷新）；
      // 其余失败同样走非阻塞 toast（c2 口径，o1 收编：错误文案由
      // errorMessage 单源映射，不再强弹窗打断）。
      if (actionError instanceof BackendHttpError && actionError.status === 409) {
        Message.warning(t('learning.learningGraphStructureConflict'));
      } else {
        // F2：失败 toast 带各自动作标题（notify 面原生 title 两行），不再
        // 五处同文分不清哪个动作失败；正文仍是 errorMessage 单源映射句。
        Message.error({
          title: t('learning.learningGraphStructureFailed'),
          content: errorMessage(t, actionError),
        });
      }
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
    markInFlight(`mount:${detail.course.id}`);
    try {
      await learningApi.mountStructure(detail.course.id);
      await refetchStructure();
    } catch (actionError) {
      if (isGenerationConflict(actionError, `learning-mount-${detail.course.id}`)) {
        Message.info(t('learning.inflightTaskToast'));
        await refetchStructure();
      } else {
        // 强弹窗降级为非阻塞 toast（o1 收编，与同文件其它动作一致）；
        // F2：带各自动作标题。
        Message.error({
          title: t('learning.learningGraphMountFailed'),
          content: errorMessage(t, actionError),
        });
      }
    } finally {
      clearInFlight(`mount:${detail.course.id}`);
      setMounting(false);
    }
  }, [detail.course.id, refetchStructure, t]);

  // 结构修补回合（j09）：按留痕与观察修补结构；皆空时后端如实 skipped。
  const repairStructure = useCallback(async () => {
    setRepairing(true);
    markInFlight(`repair:${detail.course.id}`);
    try {
      const summary = await learningApi.repairStructure(detail.course.id);
      if (summary.status === 'noop') {
        Modal.info({ title: t('learning.learningGraphRepairNoop') });
      }
      await refetchStructure();
    } catch (actionError) {
      if (isGenerationConflict(actionError, `learning-repair-${detail.course.id}`)) {
        Message.info(t('learning.inflightTaskToast'));
        await refetchStructure();
      } else {
        // 强弹窗降级为非阻塞 toast（o1 收编，与同文件其它动作一致）；
        // F2：带各自动作标题。
        Message.error({
          title: t('learning.learningGraphRepairFailed'),
          content: errorMessage(t, actionError),
        });
      }
    } finally {
      clearInFlight(`repair:${detail.course.id}`);
      setRepairing(false);
    }
  }, [detail.course.id, refetchStructure, t]);

  // ── 在途恢复（d6/d7）：组件本地在途态随路由卸载即丢——服务端真相（生成
  // 任务注册表快照）∪ 会话级 pending 标记取并集，挂载与课程事件帧时重算。
  // 声明（d6 起 basis_declare 也进注册表）/挂载/修补按 course 匹配，课时内容
  // 生成按 node_id（= lessonId）匹配。
  const [registryBusy, setRegistryBusy] = useState<{
    basis: boolean;
    structure: boolean;
    mount: boolean;
    repair: boolean;
    lessons: Set<string>;
  }>({ basis: false, structure: false, mount: false, repair: false, lessons: new Set() });
  const refreshRegistryBusy = useCallback(async () => {
    const courseId = detail.course.id;
    const sessionBasis = isInFlight(`basis:${courseId}`);
    const sessionMount = isInFlight(`mount:${courseId}`);
    const sessionRepair = isInFlight(`repair:${courseId}`);
    const sessionLessons = inFlightIdsWithPrefix(`lesson:${courseId}:`);
    setRegistryBusy({
      basis: sessionBasis,
      structure: false,
      mount: sessionMount,
      repair: sessionRepair,
      lessons: sessionLessons,
    });
    try {
      const tasks = await learningApi.generationTasks();
      const active = tasks.filter(
        (task) =>
          task.course_id === courseId && (task.state === 'running' || task.state === 'queued')
      );
      setRegistryBusy({
        basis: sessionBasis || active.some((task) => task.kind === 'basis_declare'),
        // structure 类补位（G3 失忆面）：d7 的 active 过滤四类恰好漏了本尊
        // ——刷新后结构明明在跑却呈现「尚未生成 + 可点生成按钮（点了才
        // 409）」。快照行即服务端真相，用与读数轮询同源的判定。
        structure: hasActiveStructureRow(tasks, courseId),
        mount: sessionMount || active.some((task) => task.kind === 'structure_mount'),
        repair: sessionRepair || active.some((task) => task.kind === 'structure_repair'),
        lessons: new Set([
          ...sessionLessons,
          ...active
            .filter((task) => task.kind === 'lesson' && task.node_id)
            .map((task) => task.node_id as string),
        ]),
      });
    } catch {
      // 快照不可达：保留会话级 pending（上方已先按 session 置位）。
    }
  }, [detail.course.id]);
  useEffect(() => {
    void refreshRegistryBusy();
  }, [refreshRegistryBusy]);

  // structureRunning 三源并集（G3）：WS 旗（不重放，刷新即丢）∪ 注册表快照
  // 行（上字段补位）。既有 load() 消解不动（投影非空才清 WS 旗）；快照源
  // 随终态行出集自然落定（WS 终态帧触发的注册表重算 + 下方轮询终态接棒）。
  const structureRunning = structureRunningWs || registryBusy.structure;

  // ── 结构生成实时读数（G2 哑巴面修复）：running 期间 5s 轮询任务快照，
  // attempt/elapsed 全取服务端真相（不自算钟）；快照缺行或 attempt 未回写
  // → 回退静态「生成中…」；快照不可达保留现值不臆造（c2 收敛轮询纪律）。
  // 快照终态接棒语义对齐 WS 终态臂：failed/cancelled 显式清旗（快照 null
  // 无法自行消解），done 由 load 按投影非空消解；注册表并集随终态行出集
  // 同步重算。
  const [structureReadout, setStructureReadout] = useState<StructureReadout | null>(null);
  useEffect(() => {
    if (!structureRunning) {
      setStructureReadout(null);
      return undefined;
    }
    return startStructureReadoutPolling({
      courseId: detail.course.id,
      fetchTasks: () => learningApi.generationTasks(),
      onReadout: setStructureReadout,
      onTerminal: (state) => {
        if (state === 'failed' || state === 'cancelled') setStructureRunningWs(false);
        void refreshRegistryBusy();
        structureLoadRef.current();
      },
    });
  }, [structureRunning, detail.course.id, refreshRegistryBusy]);
  // 徽章文案机面（c3 契约）：读数在 → 插值键；缺 → 静态「生成中…」。
  const structureBadge = structureBadgeCopy(structureReadout);

  // ── 已会声明（Declared Known，m2）：读数 + 一次性声明（c3 起产品面无撤销
  // 入口；后端 DELETE /basis 留作纠错通道）──────────────────────────────
  const [basis, setBasis] = useState<BasisView | null>(null);
  const [declaringBasis, setDeclaringBasis] = useState(false);
  // 声明在途合成（d6）：本地发起态 ∪ 会话 pending ∪ 注册表快照——切页重挂
  // 载后三者任一为真即恢复 loading/不可点，竞态双击不再发生。
  const basisDeclareBusy =
    declaringBasis ||
    registryBusy.basis ||
    isInFlight(`basis:${detail.course.id}`);
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
      if (event.course_id === detail.course.id) {
        load();
        // 任一课程帧都可能改变注册表在途面（basis_declared / mount/repair
        // 终态帧等）——重算派生（d6/d7）。
        void refreshRegistryBusy();
      }
    });
    return () => {
      cancelled = true;
      off();
    };
  }, [detail.course.id]);

  const declareBasis = useCallback(
    async (doors: string[]) => {
      setDeclaringBasis(true);
      markInFlight(`basis:${detail.course.id}`);
      try {
        const result = await learningApi.declareBasis(detail.course.id, doors, model.choice);
        setBasisDraftChecked([]);
        // 声明反馈走非阻塞 toast（c2）：阻塞弹窗叠在冻结展示上是二次打断；
        // 失败保持 Modal——需要用户看见并处置。
        if (result.noop) {
          Message.info(t('learning.learningGraphBasisDeclareNoop'));
        } else {
          // o4：成功 toast 只报数量——门口 id 串与块 slug 串不再内插上屏
          // （与 GateCoverage 已声明行同口径；铸出/复用计数随 o2 口语化版
          // 本文退出用户面）。
          Message.success(
            t('learning.learningGraphBasisDeclareDone', {
              doorsCount: result.doors.length,
              blocksCount: result.blocks.length,
            })
          );
        }
        const view = await learningApi.basisView(detail.course.id);
        setBasis(view);
        // 声明改就绪集/推荐集（声明概念按能教计）：立即重拉课程详情，调度
        // 效果当场可见，而不是等下次无关刷新。
        onRefresh();
      } catch (actionError) {
        if (isGenerationConflict(actionError, `basis-declare-${detail.course.id}`)) {
          // 槽锁 409（d6）：原始请求还在后台跑（切页重挂载后的竞态双击）——
          // 非阻塞提示 + 重拉读数，不弹原始 JSON 错误。
          Message.info(t('learning.inflightBasisToast'));
          const view = await learningApi.basisView(detail.course.id).catch(() => null);
          if (view) setBasis(view);
        } else {
          // 强弹窗降级为非阻塞 toast（o1 收编，与同文件其它动作一致）；
          // F2：带各自动作标题。
          Message.error({
            title: t('learning.learningGraphBasisDeclareFailed'),
            content: errorMessage(t, actionError),
          });
        }
      } finally {
        clearInFlight(`basis:${detail.course.id}`);
        setDeclaringBasis(false);
        void refreshRegistryBusy();
      }
    },
    [detail.course.id, model.choice, onRefresh, refreshRegistryBusy, t]
  );

  // ── 手动补充节点 ────────────────────────────────────────────────────────
  const [growing, setGrowing] = useState(false);
  const grow = useCallback(async () => {
    setGrowing(true);
    markInFlight(`grow:${detail.course.id}`);
    try {
      await learningApi.growGraph(detail.course.id, undefined, model.choice);
      onRefresh();
    } catch (actionError) {
      if (isGenerationConflict(actionError, `graph-growth-${detail.course.id}`)) {
        Message.info(t('learning.inflightTaskToast'));
        onRefresh();
      } else {
        // 强弹窗降级为非阻塞 toast（o1 收编，与同文件其它动作一致）；
        // F2：带各自动作标题。
        Message.error({
          title: t('learning.learningGraphGrowFailed'),
          content: errorMessage(t, actionError),
        });
      }
    } finally {
      clearInFlight(`grow:${detail.course.id}`);
      setGrowing(false);
    }
  }, [detail.course.id, model.choice, onRefresh, t]);

  if (!graph) {
    return <Empty description={t('learning.learningGraphEmpty')} />;
  }
  const readyCount = graph.ready_count;
  const readyStockFull = readyCount >= graph.ready_target;
  // 声明窗口（e3）：零节点（结构完成、首批未生长）时学习 Tab 落在门口
  // 声明画面——生长不再自动抢跑，首批由「开始学习」显式触发。
  const zeroNodes = lessons.length === 0;

  // ── 顶条单一状态徽章（o6 呈现收敛）：结构生成 / 生长 / 自动准备三个
  // processing 态此前可并排亮三个 Tag——现在按优先级（结构生成 > 生长 >
  // 自动准备）只亮一个，一行只讲一件事。v2 的实时读数徽章仍是结构态的
  // 呈现载体；v3 Pill、v4 卡片结构态行零回改。被压后的态与静态结构摘要
  // 转顶条下方次要行——只收纳不删信息。
  const processingPrimary: 'structure' | 'growth' | 'auto' | null = structureRunning
    ? 'structure'
    : graph.growth_running
      ? 'growth'
      : autoPreparing
        ? 'auto'
        : null;
  const structureSummaryText = structure?.structure
    ? t('learning.learningGraphStructureSummary', {
        spines: structure.structure.spines.length,
        blocks: structure.structure.blocks.length,
        doors: structure.terrain?.doors.length ?? 0,
      }) +
      (structure.generated_at
        ? ` · ${new Date(structure.generated_at).toLocaleString()}`
        : '')
    : null;
  const processingSecondaryParts: string[] = [];
  if (processingPrimary !== null) {
    if (graph.growth_running && processingPrimary !== 'growth') {
      processingSecondaryParts.push(t('learning.learningGraphGrowing'));
    }
    if (autoPreparing && processingPrimary !== 'auto') {
      processingSecondaryParts.push(t('learning.learningGraphAutoPreparing'));
    }
    // 处理徽章在场时加载失败句让位降级为次要行（仍可见，不丢）。
    if (structureError) {
      processingSecondaryParts.push(t('learning.learningGraphStructureLoadFailed'));
    }
    if (structureSummaryText) processingSecondaryParts.push(structureSummaryText);
  }
  const processingSecondaryLine = processingSecondaryParts.join(' · ');

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
          {processingPrimary === 'structure' ? (
            // v2 实时读数徽章：文案与插值仍出自 structureBadge（数据源不动）。
            <ProcessingStateTag>
              {t(
                structureBadge.key,
                'vars' in structureBadge ? structureBadge.vars : undefined
              )}
            </ProcessingStateTag>
          ) : processingPrimary === 'growth' ? (
            <ProcessingStateTag>{t('learning.learningGraphGrowing')}</ProcessingStateTag>
          ) : processingPrimary === 'auto' ? (
            <ProcessingStateTag>{t('learning.learningGraphAutoPreparing')}</ProcessingStateTag>
          ) : structureError ? (
            <Text type='secondary' className='text-12px'>
              {t('learning.learningGraphStructureLoadFailed')}
            </Text>
          ) : structure === null ? (
            <Spin size={12} />
          ) : structure.structure ? (
            <Text type='secondary' className='text-12px'>
              {structureSummaryText}
            </Text>
          ) : (
            <Text type='secondary' className='text-12px'>
              {t('learning.learningGraphStructureMissing')}
            </Text>
          )}
          {/* 重生成已从产品面移除（d3，用户裁决：重生成遗留问题太多）——仅在
              结构缺失/失败时保留首次「生成」入口（失败重试路径）；后端
              regenerate 端点保留作纠错通道。 */}
          {structure !== null && !structure.structure && (
            <Button
              size='mini'
              type='text'
              loading={regenerating}
              disabled={regenerating || structureRunning || busyId !== null}
              onClick={() => void regenerateStructure()}
            >
              {t('learning.learningGraphStructureGenerate')}
            </Button>
          )}
          <span className='flex-1' />
          <Button
            size='small'
            type='primary'
            loading={growing || graph.growth_running}
            // growth_running 是服务端真相（d7）：切页往返后本地 growing 已丢，
            // 服务端在跑的生长批次照样禁用补充节点（防槽锁 409 双击）。
            // structureRunning 同防呆（G5）：结构生成期间显式生长恒过后端
            // 零节点停摆臂（e3/m10 只拦自动臂），按钮侧先关闸。
            disabled={growing || graph.growth_running || busyId !== null || structureRunning}
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
        {/* o6 次要行：主徽章占用顶条后，被压后的 processing 态与静态结构
            摘要转纯文字次要行——信息不丢，但不再与主徽章抢一行。 */}
        {processingSecondaryLine !== '' && (
          <Text type='secondary' className='text-12px'>
            {processingSecondaryLine}
          </Text>
        )}
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

      {/* 内容区确定高链（d1，二轮修正）：arco 默认 .arco-tabs-content
          overflow:hidden + content-inner 高度 auto + content-item-active
          height:auto——超高内容被静默裁掉且全链无滚动条。三层都要钉高：
          content（flex-1 min-h-0，槽高）→ inner（h-full；缺这层时下一层的
          百分比高度对 auto 父级解析失效，首轮修复即漏于此）→ 激活 item
          （h-full + 自滚）。各 TabPane 在固定视口内自管滚动（学习 Tab 两栏
          行 h-full 后由内部各自滚）。 */}
      <Tabs
        activeTab={activeTab}
        onChange={setActiveTab}
        type='line'
        className='flex min-h-0 flex-1 flex-col [&_.arco-tabs-content]:min-h-0 [&_.arco-tabs-content]:flex-1 [&_.arco-tabs-content]:pt-8px [&_.arco-tabs-content-inner]:h-full [&_.arco-tabs-content-item-active]:h-full [&_.arco-tabs-content-item-active]:overflow-y-auto'
      >
        <Tabs.TabPane key='study' title={t('learning.learningGraphTabStudy')}>
          {zeroNodes ? (
            <DeclarationWindow
              view={structure}
              structureRunning={structureRunning}
              structureError={structureError}
              basis={basis}
              declaring={basisDeclareBusy}
              basisChecked={basisDraftChecked}
              onBasisCheckedChange={setBasisDraftChecked}
              onDeclare={(doors) => void declareBasis(doors)}
              starting={growing}
              growthRunning={graph.growth_running}
              onStart={() => void grow()}
            />
          ) : (
          /* 学习视图主列（u6）：两栏区 flex-1 自管滚动，知识检测入口固定
             在就绪集推荐区之后（Q2 裁决）——推荐轴（前向，未学节点）与
             检测轴（回向，已结算节点）并存但方向有别，入口常驻视口底缘
             不随推荐列表/正文滚动没入。 */
          <div className='flex h-full min-h-0 flex-col gap-10px'>
            <div className='flex min-h-0 flex-1 gap-12px'>
            <aside className='flex max-h-full w-280px shrink-0 flex-col gap-10px overflow-y-auto'>
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
                          loading={
                            busyId === primaryLesson.id ||
                            registryBusy.lessons.has(primaryLesson.id)
                          }
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
                      externalBusy={registryBusy.lessons.has(selectedLesson.id)}
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
            {/* 知识检测入口（u6，Q2 裁决）：就绪集推荐区之后。QuizEntryCard
                自带 N/K 读数与开测按钮（quiz* 既有词条）；这里只补一条回向
                语境句——图工作区推荐 = 前向学新，检测 = 回向考已结算节点，
                两者并存时方向由文案点破。归档课程不渲染也不拉读数（与
                CourseWorkspace 挂点同存在性语义）。 */}
            {detail.course.enabled && (
              <section className='flex flex-col gap-4px'>
                <Text type='secondary' className='text-12px'>
                  {t('learning.quizGraphEntryContext')}
                </Text>
                <QuizEntryCard
                  courseId={detail.course.id}
                  refreshToken={quizProgressToken}
                  onStartQuiz={onStartQuiz}
                />
              </section>
            )}
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
            mounting={mounting || registryBusy.mount}
            onRepair={() => void repairStructure()}
            repairing={repairing || registryBusy.repair}
            basisChecked={basisDraftChecked}
            onBasisCheckedChange={setBasisDraftChecked}
            onDeclareBasis={(doors) => void declareBasis(doors)}
            declaringBasis={basisDeclareBusy}
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
          // o1：state 只存 errorMessage 单源映射句，原始报文不进人面。
          if (!cancelled) setError(errorMessage(t, loadError));
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
  }, [courseId, t]);

  if (error) {
    return <Alert type='error' content={error} />;
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
 * 徽标；失败红色 role=alert 带原因分类句（原文悬停 Tooltip 可见）；空态
 * 「暂无系统动作」。 */
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
          // o1：state 只存 errorMessage 单源映射句，原始报文不进人面。
          if (!cancelled) setError(errorMessage(t, loadError));
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
  }, [courseId, t]);

  const model = actions ? buildSystemActionRows(actions) : null;
  return (
    <NomiCollapse bordered={false}>
      <NomiCollapse.Item
        name='system-actions'
        header={<span className='text-12px'>{t('learning.learningGraphSystemActions')}</span>}
      >
        {error ? (
          <Alert type='error' content={error} />
        ) : !model ? (
          <div className='flex justify-center py-16px'>
            <Spin />
          </div>
        ) : model.empty ? (
          <Empty description={t('learning.learningGraphSystemActionsEmpty')} />
        ) : (
          <div className='flex flex-col'>
            {model.rows.map((row) => {
              // 失败原因：分类句为主显示；诊断原文只经 Tooltip 收纳（同波
              // GrowthTaskPill failed detail 先例——收纳不删信息），无原文
              // 不出 Tooltip。
              const reasonLine =
                row.reasonKey.trim() !== '' ? (
                  <Text type='error' className='min-w-0 basis-full truncate text-11px'>
                    {t(row.reasonKey)}
                  </Text>
                ) : null;
              return (
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
                  {reasonLine && row.reasonDetail !== '' ? (
                    <Tooltip content={row.reasonDetail}>{reasonLine}</Tooltip>
                  ) : (
                    reasonLine
                  )}
                </div>
              );
            })}
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
  const navigate = useNavigate();
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
          // o1：state 只存 errorMessage 单源映射句，原始报文不进人面。
          if (!cancelled) setError(errorMessage(t, loadError));
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
  }, [courseId, t]);

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
    return <Alert type='error' content={error} />;
  }
  if (rows === null) {
    return (
      <div className='flex justify-center py-32px'>
        <Spin />
      </div>
    );
  }
  // 课程内概念表只是全局概念的本课切片（c8）：表头常驻全局总览入口，行点击
  // 进概念详情（定义全文 / 关联关系 / 跨课程应用）。
  const header = (
    <div className='flex flex-wrap items-center gap-8px'>
      <Text type='secondary' className='text-12px leading-18px'>
        {t('learning.conceptTableHint')}
      </Text>
      <span className='flex-1' />
      <Button size='small' onClick={() => navigate('/learn/concepts')}>
        {t('learning.conceptsOverviewTitle')}
      </Button>
    </div>
  );
  if (rows.length === 0) {
    return (
      <div className='flex flex-col gap-6px'>
        {header}
        <Empty description={t('learning.learningGraphConceptsEmpty')} />
      </div>
    );
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
              fragments: fragments.join(t('learning.listSeparator')),
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
      {header}
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
        rowClassName={() => 'cursor-pointer'}
        onRow={(row: GraphConceptRowView) => ({
          onClick: () => navigate(`/learn/concepts/${encodeURIComponent(row.concept_id)}`),
        })}
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
                      alias.definition
                        ? t('learning.learningGraphConceptAliasWithDefinition', {
                            name: alias.name,
                            definition: alias.definition,
                          })
                        : alias.name
                    )
                    .join(t('learning.listSeparator'))}
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
              <div className='flex flex-wrap items-center gap-x-8px'>
                {row.other_courses.map((course) => (
                  <Button
                    key={course.course_id}
                    type='text'
                    size='mini'
                    className='!px-0 text-12px'
                    onClick={(event) => {
                      // 行点击 = 进概念详情；课程链接不冒泡，直接去该课程。
                      event.stopPropagation();
                      navigate(`/learn/${encodeURIComponent(course.course_id)}`);
                    }}
                  >
                    {course.title}
                  </Button>
                ))}
              </div>
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
