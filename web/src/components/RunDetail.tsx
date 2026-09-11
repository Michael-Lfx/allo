/**
 * W6 Run 状态树（R11）—— Run 面主体。
 *
 * 只渲染 store 已经跟随的 Run（`activeRunId` + `runEvents`，订阅由 W2 的
 * `followRun` 建立）：本组件**不发请求、不自己订阅**，所以审批卡、状态树、引导
 * 输入看到的是同一份事件投影，不会各说各话。
 *
 * 树由 `lib/run-tree.ts` 纯投影得到（运行头 → 计划修订 → 步骤 → 尝试），文本全部
 * 走 i18n；协议状态值用 `run.statusValue.*` 家族，遇到未知值直接回落到原值（不
 * 假装认识）。原始事件降级成底部可展开的调试面板（默认收起）。
 */

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronRight, CircleStop, ListTree } from "lucide-react";

import { useAppStore } from "../store/appStore";
import { buildRunTree, orderRunEvents, runStatusTone, type StepNode } from "../lib/run-tree";
import {
  formatDuration,
  planProgress,
  planStepAnchor,
  planStepViews,
  runStepAnchor,
} from "../lib/run-plan";
import { shortId } from "../ui/format";
import type { RunEvent, RunPlan } from "../lib/protocol";

/** 已知标记 → i18n key；未登记的标记直接显示原文（调试面板的价值之一）。 */
const MARKER_KEYS: Record<string, string> = {
  retry_requested: "run.marker.retryRequested",
  output_adopted: "run.marker.outputAdopted",
  conversation_effect_requested: "run.marker.effectRequested",
  conversation_effect_delivered: "run.marker.effectDelivered",
  queued_before_restart: "run.marker.queuedBeforeRestart",
  process_restart: "run.marker.processRestart",
  initial_turn_receipt: "run.marker.initialTurnReceipt",
  stop_turn: "run.marker.effectStopTurn",
  steer: "run.marker.effectSteer",
};

/** 已知计划变更类型 → i18n key；未登记的显示原文。 */
const PLAN_CHANGE_KEYS: Record<string, string> = {
  initial_plan: "run.planChange.initialPlan",
  replanned: "run.planChange.replanned",
  adjusted: "run.planChange.adjusted",
  steps_added: "run.planChange.stepsAdded",
  delegated_steps_appended: "run.planChange.delegatedStepsAppended",
};

/**
 * 纯展示层：显式接收 `runId` / `events`，不读 store。
 *
 * 拆开的理由不只是分层——store 走的 `useSyncExternalStore` 在服务端渲染时读的是
 * **初始** state，所以「用真实 store 渲染再断言 HTML」的测试必须能绕开它。测试直接
 * 渲染 `RunSurface`（`components/RunDetail.render.test.tsx`）。
 */
export function RunSurface({
  runId,
  events,
  plan,
  onCancel,
}: {
  runId: string | null;
  events: RunEvent[];
  /** W4：`run/plan` 的权威快照（步骤标题 / 状态 / 成员 / 尝试细节）。 */
  plan?: RunPlan | null;
  onCancel?: () => void;
}) {
  const { t } = useTranslation();
  const [openSteps, setOpenSteps] = useState<Record<string, boolean>>({});

  if (!runId) return null;

  const tree = buildRunTree(events);
  const tone = runStatusTone(tree.header.status);
  const statusLabel = tree.header.status
    ? t(`run.statusValue.${tree.header.status}`, { defaultValue: tree.header.status })
    : t("run.statusUnknown");

  const toggleStep = (stepId: string) =>
    setOpenSteps((open) => ({ ...open, [stepId]: !open[stepId] }));

  // W4：计划快照 → 待办行。事件树与快照是**互补**的两半：事件给「发生过什么」，
  // 快照给「现在是什么」（标题 / 成员 / 每次尝试的原因与耗时）。
  const todos = planStepViews(plan);
  const progress = planProgress(todos);

  /** 「与 W6 step 树互跳」：滚动到对侧的同一步骤锚点并高亮一瞬。 */
  const jumpTo = (anchor: string) => {
    const target = typeof document === "undefined" ? null : document.getElementById(anchor);
    target?.scrollIntoView?.({ behavior: "smooth", block: "center" });
    target?.classList?.add("is-jump-target");
    // 高亮是纯视觉反馈：没有 classList/window 的环境（SSR、测试）静默跳过。
    if (typeof window !== "undefined" && typeof window.setTimeout === "function") {
      window.setTimeout(() => target?.classList?.remove("is-jump-target"), 1200);
    }
  };

  return (
    <section className="run-surface" aria-label={t("run.title")}>
      <header className="run-head">
        <span className="run-head-title">
          <ListTree aria-hidden="true" size={15} strokeWidth={1.8} />
          {t("run.title")} <span className="run-head-id">{shortId(runId)}</span>
        </span>
        <span className={`run-badge run-badge-${tone}`}>{statusLabel}</span>
        <span className="run-head-meta">{t("run.eventCount", { count: tree.header.eventCount })}</span>
        <button
          className="quiet-button run-cancel"
          type="button"
          onClick={onCancel}
          disabled={tree.header.terminal || !onCancel}
          title={t("run.cancelHint")}
        >
          <CircleStop aria-hidden="true" size={14} strokeWidth={1.8} />
          {t("run.cancel")}
        </button>
      </header>

      {tree.header.statusReason && (
        <p className="run-reason">
          {t("run.statusReason", {
            reason: t(`run.statusReasonValue.${tree.header.statusReason}`, {
              defaultValue: tree.header.statusReason,
            }),
          })}
        </p>
      )}

      {/* W4：计划与待办（来自 `run/plan` 的权威快照）。事件里没有标题与耗时，
          所以这一段不是事件树的重复——它是「现在是什么」，事件树是「发生过什么」。 */}
      <div className="run-section-title">
        {t("run.todoTitle", { done: progress.done, total: progress.total })}
      </div>
      {todos.length === 0 ? (
        <p className="run-empty-line">{t("run.todoUnavailable")}</p>
      ) : (
        <ul className="run-todos">
          {todos.map((todo) => (
            <li className={`run-todo is-${todo.tone}`} key={todo.stepId} id={planStepAnchor(todo.stepId)}>
              <div className="run-todo-head">
                <span className="run-todo-index">{todo.index}</span>
                <span className="run-todo-title">{todo.title}</span>
                <span className={`run-badge run-badge-${todo.tone}`}>
                  {t(`run.statusValue.${todo.status}`, { defaultValue: todo.status })}
                </span>
                {todo.member && <span className="run-todo-member">{todo.member}</span>}
                {todo.superseded && <span className="run-todo-superseded">{t("run.todoSuperseded")}</span>}
                {todo.introducedInRevision > 1 && (
                  <span className="run-todo-rev">{t("run.todoRevision", { n: todo.introducedInRevision })}</span>
                )}
                <button
                  className="run-todo-jump"
                  type="button"
                  onClick={() => jumpTo(runStepAnchor(todo.stepId))}
                  title={t("run.todoJump")}
                >
                  {t("run.todoJump")}
                </button>
              </div>
              {todo.attempts.length > 0 && (
                <ul className="run-todo-attempts">
                  {todo.attempts.map((attempt) => (
                    <li className="run-todo-attempt" key={attempt.attemptId}>
                      <span className="run-todo-attempt-no">#{attempt.ordinal}</span>
                      <span className={`run-badge run-badge-${attempt.tone}`}>
                        {t(`run.statusValue.${attempt.status}`, { defaultValue: attempt.status })}
                      </span>
                      <span className="run-todo-attempt-reason">
                        {t(`run.statusReasonValue.${attempt.triggerReason}`, {
                          defaultValue: attempt.triggerReason,
                        })}
                      </span>
                      {/* 耗时只在两端都有时间戳时显示；进行中不显示 0s。 */}
                      {formatDuration(attempt.durationMs) && (
                        <span className="run-todo-attempt-duration">{formatDuration(attempt.durationMs)}</span>
                      )}
                      {attempt.member && <span className="run-todo-attempt-member">{attempt.member}</span>}
                      {attempt.tokens !== null && (
                        <span className="run-todo-attempt-tokens">{t("run.todoTokens", { count: attempt.tokens })}</span>
                      )}
                      {attempt.error && <span className="run-todo-attempt-error">{attempt.error}</span>}
                      {attempt.outputSummary && (
                        <span className="run-todo-attempt-output">{attempt.outputSummary}</span>
                      )}
                    </li>
                  ))}
                </ul>
              )}
            </li>
          ))}
        </ul>
      )}

      <div className="run-section-title">{t("run.planRevisions", { count: tree.planRevisions.length })}</div>
      {tree.planRevisions.length === 0 ? (
        <p className="run-empty-line">{t("run.planNone")}</p>
      ) : (
        <ol className="run-plan">
          {tree.planRevisions.map((revision) => (
            <li className="run-plan-item" key={`plan-${revision.sequence}`}>
              <span className="run-seq">#{revision.sequence}</span>
              <span className="run-plan-change">
                {t(PLAN_CHANGE_KEYS[revision.change] ?? `run.planChange.${revision.change}`, {
                  defaultValue: revision.change,
                })}
              </span>
              {revision.status && (
                <span className="run-plan-status">
                  {t(`run.statusValue.${revision.status}`, { defaultValue: revision.status })}
                </span>
              )}
              {revision.intent && (
                <span className="run-plan-intent">{t("run.planIntent", { intent: revision.intent })}</span>
              )}
            </li>
          ))}
        </ol>
      )}

      <div className="run-section-title">{t("run.steps", { count: tree.steps.length })}</div>
      {tree.steps.length === 0 ? (
        <p className="run-empty-line">{t("run.stepsNone")}</p>
      ) : (
        <ul className="run-steps">
          {tree.steps.map((step) => (
            <StepRow
              key={step.stepId}
              step={step}
              // 默认展开：有尝试或有过引导记录（W3 的回执应当在不需要点击时就看得见）。
              open={openSteps[step.stepId] ?? (step.attempts.length > 0 || step.effects.length > 0)}
              onToggle={() => toggleStep(step.stepId)}
            />
          ))}
        </ul>
      )}

      {tree.unattributedEventCount > 0 && (
        <p className="run-unattributed">
          {t("run.unattributed", { count: tree.unattributedEventCount })}
        </p>
      )}

      <details className="run-raw">
        <summary>{t("run.rawEvents", { count: tree.header.eventCount })}</summary>
        <table className="run-raw-table">
          <thead>
            <tr>
              <th>{t("run.rawSeq")}</th>
              <th>{t("run.rawType")}</th>
              <th>{t("run.rawScope")}</th>
              <th>{t("run.rawPayload")}</th>
            </tr>
          </thead>
          <tbody>
            {orderRunEvents(events).map((event) => (
              <tr key={`${event.run_id}:${event.sequence}`}>
                <td>{event.sequence}</td>
                <td>{event.event_type}</td>
                <td className="run-raw-scope">
                  {event.step_id ? shortId(event.step_id) : "—"}
                  {event.attempt_id ? ` / ${shortId(event.attempt_id)}` : ""}
                </td>
                <td className="run-raw-payload">{JSON.stringify(event.payload)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </details>
    </section>
  );
}

function StepRow({ step, open, onToggle }: { step: StepNode; open: boolean; onToggle: () => void }) {
  const { t } = useTranslation();
  const tone = runStatusTone(step.status);
  return (
    <li className="run-step" id={runStepAnchor(step.stepId)}>
      <button className="run-step-head" type="button" onClick={onToggle} aria-expanded={open}>
        <ChevronRight className={open ? "is-expanded" : ""} aria-hidden="true" size={15} strokeWidth={1.8} />
        <span className="run-step-id">{shortId(step.stepId)}</span>
        {step.kind && <span className="run-step-kind">{step.kind}</span>}
        <span className={`run-badge run-badge-${tone}`}>
          {step.status ? t(`run.statusValue.${step.status}`, { defaultValue: step.status }) : t("run.statusUnknown")}
        </span>
        {step.retries > 0 && <span className="run-step-retries">{t("run.retries", { count: step.retries })}</span>}
        <span className="run-step-meta">
          {t("run.attemptCount", { count: step.attempts.length })} · {t("run.eventCount", { count: step.eventCount })}
        </span>
      </button>
      {open && (
        <div className="run-step-body">
          {step.effects.length > 0 && (
            <ul className="run-effects">
              {step.effects.map((effect) => (
                <li key={`effect-${effect.sequence}`}>
                  <span className="run-seq">#{effect.sequence}</span>
                  {t(`run.marker.effect${effect.effect === "steer" ? "Steer" : "StopTurn"}`, {
                    defaultValue: effect.effect,
                  })}
                  {" · "}
                  {effect.delivered ? t("run.marker.effectDelivered") : t("run.marker.effectRequested")}
                </li>
              ))}
            </ul>
          )}
          {step.attempts.length === 0 ? (
            <p className="run-empty-line">{t("run.attemptsNone")}</p>
          ) : (
            <ul className="run-attempts">
              {step.attempts.map((attempt) => (
                <li className="run-attempt" key={attempt.attemptId}>
                  <div className="run-attempt-head">
                    <span className="run-attempt-id">{shortId(attempt.attemptId)}</span>
                    <span className={`run-badge run-badge-${runStatusTone(attempt.status)}`}>
                      {attempt.status
                        ? t(`run.statusValue.${attempt.status}`, { defaultValue: attempt.status })
                        : t("run.statusUnknown")}
                    </span>
                    <span className="run-attempt-seq">
                      #{attempt.firstSequence}
                      {attempt.lastSequence !== attempt.firstSequence ? `–#${attempt.lastSequence}` : ""}
                    </span>
                    <span className="run-attempt-count">{t("run.eventCount", { count: attempt.eventCount })}</span>
                  </div>
                  {attempt.markers.length > 0 && (
                    <div className="run-markers">
                      {attempt.markers.map((marker) => (
                        <span className="run-marker" key={`${attempt.attemptId}-${marker}`}>
                          {MARKER_KEYS[marker] ? t(MARKER_KEYS[marker]) : marker}
                        </span>
                      ))}
                    </div>
                  )}
                  {attempt.approval && (
                    <div className="run-approval">
                      <span className="run-seq">#{attempt.approval.sequence}</span>
                      <span className="run-approval-question">{attempt.approval.question}</span>
                      <span className={`run-badge run-badge-${attempt.approval.answered ? "ok" : "attention"}`}>
                        {attempt.approval.answered ? t("run.approvalAnswered") : t("run.approvalPending")}
                      </span>
                    </div>
                  )}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </li>
  );
}

/**
 * store 连接层：跟随 W2 建立的订阅状态（不自己订阅、不自己发请求），把取消动作
 * 接到 `cancelRun`（同样先读服务端版本再提交 CAS）。
 */
export function RunDetail() {
  const activeRunId = useAppStore((s) => s.activeRunId);
  const runEvents = useAppStore((s) => s.runEvents);
  const runPlan = useAppStore((s) => s.runPlan);
  const cancelRun = useAppStore((s) => s.cancelRun);

  return (
    <RunSurface
      runId={activeRunId}
      events={runEvents}
      plan={runPlan}
      onCancel={() => void cancelRun()}
    />
  );
}
