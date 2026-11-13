import type { GrowthVerdictView } from './types';

/**
 * 自动补批触发②的纯决策面（m10）：仅 due（就绪水位以下）且非停摆臂才
 * 自动补一批。below_threshold（水位以上）与 disabled（开关合力关）如实
 * 不动作；stalled（空图）抑制——ADR-0010 只留空图臂，前端与后端同语义。
 */
export function shouldAutoFireGrowth(view: GrowthVerdictView): boolean {
  return view.verdict === 'due' && !view.stalled;
}

/**
 * 会话内重入集（m10，learnhub BTreeSet 语义域中立化）：每课每会话至多
 * 自动触发一批；后端课级 slot 锁 / 任务注册表是双保险（m7 闸已落地）。
 * 派发失败或裁决判定不动作时 release——配额只在真发了一批后消耗，
 * 不把下次工作区加载挂死。
 */
export class AutoRefillSessionGate {
  private readonly fired = new Set<string>();

  /** 首次标记返回 true（本轮由你触发）；已在集内返回 false（勿重复触发）。 */
  tryMark(courseId: string): boolean {
    if (this.fired.has(courseId)) return false;
    this.fired.add(courseId);
    return true;
  }

  /** 释放配额（未派发成功时调用；未标记过的课释放是空操作）。 */
  release(courseId: string): void {
    this.fired.delete(courseId);
  }
}

/** 模块级单例：跨工作区挂载共享（同一前端会话内每课至多自动一批）。
 * 单测自建实例，不碰单例状态。 */
export const autoRefillSessionGate = new AutoRefillSessionGate();
