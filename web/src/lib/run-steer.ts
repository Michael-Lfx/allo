/**
 * W3 中断与引导（R9）—— `run/steer` 的可用性判定与拒绝口径。
 *
 * 协议侧 (`05` §5.2) 的语义：`run/steer` 只对**正在运行**的 Run 生效，且必须带
 * `expectedVersion`（陈旧版本由服务端以 `conflict` 拒绝，客户端必须重新读取再
 * 提交，不能本地猜测版本）。这里只放**纯判定**，真正的往返在 store 里。
 *
 * 与 `run/cancel` 的分工：steer 是「不打断当前回合的补充输入」，cancel 是终止；
 * 终态 Run 两者都不该被当成引导输入悄悄吞掉——所以终态提交要**明确拒绝并说明**，
 * 而不是发一个必然失败的请求。
 */

/** 引导输入的可用性。 */
export type SteerAvailability =
  | "available"
  | "busy"
  /** Run 已到终态：提交必须被拒绝（且说明原因），不发请求。 */
  | "terminal"
  /** 还没有跟到任何 Run。 */
  | "no-run";

export interface SteerContext {
  hasRun: boolean;
  /** 是否已有一次 steer 在途（避免重复提交）。 */
  busy: boolean;
  /** `run/events` 报告的最新状态；`null` 表示事件尚未到齐。 */
  status: string | null;
  /** 该状态是否终态（由 `run-notify.isTerminalRunStatus` 判定后传入）。 */
  terminal: boolean;
}

export function steerAvailability(context: SteerContext): SteerAvailability {
  if (!context.hasRun) return "no-run";
  if (context.terminal) return "terminal";
  if (context.busy) return "busy";
  return "available";
}

/** 引导输入不可用时给用户看的原因（i18n key；`available` 无对应文案）。 */
export const STEER_BLOCKED_KEYS: Record<Exclude<SteerAvailability, "available">, string> = {
  "no-run": "run.steerNoRun",
  terminal: "run.steerTerminal",
  busy: "run.steerBusy",
};

/** 提交成功后的确认文案（i18n key）。 */
export const STEER_ACCEPTED_KEY = "run.steerAccepted";

/**
 * 服务端是否以「版本陈旧/状态已变」拒绝了这次写入。
 *
 * `run/steer` 与 `run/answer-decision` 一样靠 CAS：调用方必须**重新读取**
 * `run/get` 再提交，而不是本地自增版本。结构化判定（不依赖具体错误类实例）
 * 让本函数在跨包/跨 realm 的场景也可用。
 */
export function isStaleRunWrite(error: unknown): boolean {
  if (typeof error !== "object" || error === null) return false;
  const code = (error as { code?: unknown }).code;
  return code === "conflict";
}
