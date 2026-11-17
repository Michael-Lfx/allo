import type { VoiceCapabilityReport } from '@common/adapter/voiceCompanion';

/**
 * Options for the automatic readiness check that runs right after the local
 * voice models become ready (for example when a download finishes).
 */
export interface AutoCheckOptions {
  /** Runs one readiness check. */
  runSelfCheck: () => Promise<VoiceCapabilityReport>;
  /**
   * Called when another check is already running (`self_check_in_progress`):
   * wait for the stored report instead of failing the whole round. Receives the
   * remaining time budget so the caller can stop polling in time.
   */
  waitForFreshReport?: (
    previousCheckedAtMs: number | null | undefined,
    remainingMs: number,
  ) => Promise<VoiceCapabilityReport | null>;
  previousCheckedAtMs?: number | null;
  /** Total attempts, including the first one. Defaults to 3. */
  attempts?: number;
  /** Delay between attempts in milliseconds. Defaults to 2000. */
  retryDelayMs?: number;
  /** Hard deadline for the whole check, including waiting for background checks. */
  totalBudgetMs?: number;
  sleep?: (milliseconds: number) => Promise<void>;
  /** Injectable clock for deterministic budget tests. Defaults to Date.now. */
  now?: () => number;
  onReport?: (report: VoiceCapabilityReport) => void;
  isCancelled?: () => boolean;
}

export interface AutoCheckResult {
  report: VoiceCapabilityReport | null;
  attemptsUsed: number;
  ready: boolean;
  cancelled: boolean;
  /** True when the deadline stopped further attempts. */
  budgetExhausted: boolean;
}

/**
 * Decides whether the page should run the automatic readiness check.
 * The check is meant for "right after the pack became ready" and for pages that
 * have no trustworthy ready report yet; a page mount that already has a ready
 * report must stay read-only instead of re-probing the daemon on every visit.
 */
export function shouldAutoCheckVoiceCapabilities(input: {
  modelStatus: string | null;
  voiceEnabled: boolean;
  becameReady: boolean;
  capabilitiesLoaded: boolean;
  hasReadyReport: boolean;
}): boolean {
  if (input.modelStatus !== 'Ready' || !input.voiceEnabled) return false;
  if (input.becameReady) return true;
  if (!input.capabilitiesLoaded) return false;
  return !input.hasReadyReport;
}

const now = () => Date.now();

const defaultSleep = (milliseconds: number) =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, milliseconds);
  });

/**
 * Runs the automatic readiness check with a small, bounded retry budget.
 *
 * The daemon accepts its first readiness check only after the audio pipeline
 * and the dialogue router are up, which can trail the model-ready event by a
 * few seconds. Retrying here keeps the page in its "checking" state instead of
 * flashing "not ready" and forcing the user to click the manual check. A hard
 * total deadline keeps a wedged daemon from holding the loading state forever.
 */
export async function autoCheckVoiceCapabilities(options: AutoCheckOptions): Promise<AutoCheckResult> {
  const attempts = Math.max(1, options.attempts ?? 3);
  const retryDelayMs = Math.max(0, options.retryDelayMs ?? 2000);
  const totalBudgetMs = Math.max(0, options.totalBudgetMs ?? 20_000);
  const sleep = options.sleep ?? defaultSleep;
  const cancelled = () => options.isCancelled?.() === true;
  const clock = options.now ?? (() => Date.now());
  const startedAt = clock();
  const remainingMs = () => Math.max(0, totalBudgetMs - (clock() - startedAt));

  let report: VoiceCapabilityReport | null = null;
  let attemptsUsed = 0;
  let budgetExhausted = false;

  for (let attempt = 0; attempt < attempts; attempt += 1) {
    if (cancelled()) {
      return { report, attemptsUsed, ready: report?.can_start_full_call === true, cancelled: true, budgetExhausted };
    }
    if (remainingMs() <= 0) {
      budgetExhausted = true;
      break;
    }
    attemptsUsed += 1;
    try {
      report = await options.runSelfCheck();
      options.onReport?.(report);
      if (report.can_start_full_call) {
        return { report, attemptsUsed, ready: true, cancelled: false, budgetExhausted: false };
      }
    } catch (error) {
      const code = (error as { code?: string } | null | undefined)?.code;
      if (code === 'self_check_in_progress' && options.waitForFreshReport) {
        const fresh = await options.waitForFreshReport(options.previousCheckedAtMs, remainingMs());
        if (fresh) {
          report = fresh;
          options.onReport?.(fresh);
          if (fresh.can_start_full_call) {
            return { report, attemptsUsed, ready: true, cancelled: false, budgetExhausted: false };
          }
        }
        // The wait already consumed time; start the next attempt right away.
        continue;
      }
    }
    if (attempt + 1 < attempts) {
      const wait = Math.min(retryDelayMs, remainingMs());
      if (wait <= 0) {
        budgetExhausted = true;
        break;
      }
      await sleep(wait);
    }
  }

  return {
    report,
    attemptsUsed,
    ready: report?.can_start_full_call === true,
    cancelled: cancelled(),
    budgetExhausted,
  };
}
