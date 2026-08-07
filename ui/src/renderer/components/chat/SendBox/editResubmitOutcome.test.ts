import { describe, expect, test } from 'bun:test';

import { resolveEditResubmitOutcome } from '@/renderer/components/chat/SendBox/editResubmitOutcome';

/**
 * C2 时序纯决策覆盖（对应计划测试 9/10/12）。
 *
 * 这些测试直接驱动 resolveEditResubmitOutcome——SendBox 的 .then/.catch/.finally
 * 回调按其返回值执行 UI 副作用，故此处覆盖决策即等价覆盖行为契约，无需渲染组件。
 *
 * Coverage for the C2 timing pure decision (plan tests 9/10/12). These drive
 * resolveEditResubmitOutcome directly — SendBox's .then/.catch/.finally apply UI
 * side effects strictly per its return value, so covering the decision covers the
 * behavioral contract without a component render.
 */
describe('resolveEditResubmitOutcome', () => {
  test('success + revision unchanged: clears input and exits edit mode', () => {
    const outcome = resolveEditResubmitOutcome({
      isCurrentOperation: true,
      revisionUnchanged: true,
      status: 'success',
    });
    expect(outcome).toEqual({
      stale: false,
      clearInput: true,
      exitEditMode: true,
      restoreSubmittedInput: false,
      clearLoading: true,
    });
  });

  // 计划测试 9：飞行中改输入 → 成功保留新输入（不清空），仅退出编辑事务。
  // Plan test 9: input changed mid-flight → success keeps the new input (no
  // clear), only retiring this edit transaction. Crucially the decision keys on
  // the revision, not on whether the text happens to equal the submitted text.
  test('success + revision changed (mid-flight input): keeps current input, exits edit mode', () => {
    const outcome = resolveEditResubmitOutcome({
      isCurrentOperation: true,
      revisionUnchanged: false,
      status: 'success',
    });
    expect(outcome.clearInput).toBe(false);
    expect(outcome.exitEditMode).toBe(true);
    expect(outcome.stale).toBe(false);
    expect(outcome.restoreSubmittedInput).toBe(false);
  });

  test('success + revision changed even back to original text: still does not clear (revision, not string equality)', () => {
    // Even if the user typed and then reverted to the exact submitted string,
    // revisionUnchanged is false (the revision counter moved), so we must NOT
    // clear — otherwise a coincidental string match would wipe in-flight work.
    const outcome = resolveEditResubmitOutcome({
      isCurrentOperation: true,
      revisionUnchanged: false,
      status: 'success',
    });
    expect(outcome.clearInput).toBe(false);
  });

  // 计划测试 10：失败 + 飞行中未改输入 → 恢复已提交文本、保持编辑态以便重试。
  // Plan test 10 (unchanged case): failure with unchanged input restores the
  // submitted text and stays in edit mode for retry.
  test('failure + revision unchanged: restores submitted input, stays in edit mode', () => {
    const outcome = resolveEditResubmitOutcome({
      isCurrentOperation: true,
      revisionUnchanged: true,
      status: 'failure',
    });
    expect(outcome.restoreSubmittedInput).toBe(true);
    expect(outcome.exitEditMode).toBe(false);
    expect(outcome.clearInput).toBe(false);
    expect(outcome.stale).toBe(false);
  });

  // 计划测试 10（飞行中改输入分支）：失败但输入已变 → 保留当前输入与编辑态，
  // 旧失败回调不得覆盖用户正在输入的内容。
  // Plan test 10 (changed case): failure with changed input keeps the current
  // input and edit mode; the stale failure callback must not overwrite in-flight typing.
  test('failure + revision changed: keeps current input and edit mode', () => {
    const outcome = resolveEditResubmitOutcome({
      isCurrentOperation: true,
      revisionUnchanged: false,
      status: 'failure',
    });
    expect(outcome.restoreSubmittedInput).toBe(false);
    expect(outcome.exitEditMode).toBe(false);
    expect(outcome.clearInput).toBe(false);
  });

  test('failure never exits edit mode regardless of revision', () => {
    expect(
      resolveEditResubmitOutcome({ isCurrentOperation: true, revisionUnchanged: true, status: 'failure' })
        .exitEditMode
    ).toBe(false);
    expect(
      resolveEditResubmitOutcome({ isCurrentOperation: true, revisionUnchanged: false, status: 'failure' })
        .exitEditMode
    ).toBe(false);
  });

  // 计划测试 12：陈旧操作回调不触碰任何 UI 状态（含 .finally 的 setIsLoading）。
  // Plan test 12: a stale operation's callback (success OR failure) touches no UI
  // state, including isLoading in .finally — clearLoading must be false.
  describe('stale operation (operation token guard)', () => {
    test('stale success callback is a full no-op', () => {
      const outcome = resolveEditResubmitOutcome({
        isCurrentOperation: false,
        revisionUnchanged: true,
        status: 'success',
      });
      expect(outcome).toEqual({
        stale: true,
        clearInput: false,
        exitEditMode: false,
        restoreSubmittedInput: false,
        clearLoading: false,
      });
    });

    test('stale failure callback is a full no-op (no restore, no loading toggle)', () => {
      const outcome = resolveEditResubmitOutcome({
        isCurrentOperation: false,
        revisionUnchanged: true,
        status: 'failure',
      });
      expect(outcome.stale).toBe(true);
      expect(outcome.restoreSubmittedInput).toBe(false);
      expect(outcome.clearLoading).toBe(false);
      expect(outcome.clearInput).toBe(false);
      expect(outcome.exitEditMode).toBe(false);
    });

    test('stale callback ignores revision entirely', () => {
      const unchanged = resolveEditResubmitOutcome({
        isCurrentOperation: false,
        revisionUnchanged: true,
        status: 'success',
      });
      const changed = resolveEditResubmitOutcome({
        isCurrentOperation: false,
        revisionUnchanged: false,
        status: 'success',
      });
      expect(unchanged).toEqual(changed);
    });
  });

  test('clearLoading tracks the operation token for both success and failure (finally guard)', () => {
    // Current op lowers loading on both outcomes; stale op never does.
    expect(
      resolveEditResubmitOutcome({ isCurrentOperation: true, revisionUnchanged: true, status: 'success' })
        .clearLoading
    ).toBe(true);
    expect(
      resolveEditResubmitOutcome({ isCurrentOperation: true, revisionUnchanged: true, status: 'failure' })
        .clearLoading
    ).toBe(true);
    expect(
      resolveEditResubmitOutcome({ isCurrentOperation: false, revisionUnchanged: true, status: 'success' })
        .clearLoading
    ).toBe(false);
  });
});
