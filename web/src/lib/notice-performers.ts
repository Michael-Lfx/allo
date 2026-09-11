/**
 * Channel-specific performers for the global-effect gate (W8 余项 / D4=A).
 *
 * The gate decides *who* may fire; a performer decides *whether the effect can
 * go out at all* and returns `false` when its channel refuses (no
 * `Notification`, permission not granted, no audio context). A refused
 * performer never marks the key as handled, so a peer tab that can deliver the
 * notice still gets the chance.
 *
 * Nothing here touches a global directly — the browser wiring (and the i18n
 * translator) is injected, so the decisions are unit-testable with fakes.
 */

import type { GlobalEffectNotice, GlobalEffectPerformer } from "./global-effects";

export interface NotificationLike {
  close?: () => void;
  /** Clicking the notification brings the delivering tab forward (AC-8). */
  onclick?: (() => void) | null;
}

export interface NotificationConstructorLike {
  new (title: string, options?: { body?: string; tag?: string }): NotificationLike;
}

export interface OscillatorLike {
  type: string;
  frequency: { setValueAtTime(value: number, startTime: number): void };
  connect(node: unknown): void;
  start(when: number): void;
  stop(when: number): void;
}

export interface GainLike {
  gain: {
    setValueAtTime(value: number, startTime: number): void;
    exponentialRampToValueAtTime(value: number, startTime: number): void;
  };
  connect(node: unknown): void;
}

export interface AudioContextLike {
  state?: string;
  currentTime: number;
  destination: unknown;
  createOscillator(): OscillatorLike;
  createGain(): GainLike;
  resume?: () => Promise<void> | void;
  close?: () => Promise<void> | void;
}

export interface NoticeRuntime {
  /** Resolves an i18n key (the real wiring passes `i18n.t`). */
  translate: (key: string, params?: Record<string, unknown>) => string;
  notification?: NotificationConstructorLike | null;
  /** `Notification.permission` at perform time (defaults to `"granted"`). */
  notificationPermission?: () => string;
  createAudioContext?: (() => AudioContextLike | null) | null;
  /** Timer used to close the audio context after the chime (injectable). */
  schedule?: (run: () => void, delayMs: number) => void;
  /** Brings the delivering tab to the front when the notice is clicked. */
  focusWindow?: (() => void) | null;
}

/** Short two-step chime; kept in code so no asset has to be shipped. */
const CHIME_DURATION_MS = 360;
const CHIME_CLOSE_DELAY_MS = 800;

export function createDesktopNotificationPerformer(runtime: NoticeRuntime): GlobalEffectPerformer {
  return {
    perform: (notice: GlobalEffectNotice): boolean => {
      try {
        const Ctor = runtime.notification;
        if (!Ctor) return false;
        const permission = runtime.notificationPermission?.() ?? "granted";
        // The permission is never requested from here: requesting it needs a user
        // gesture, and a prompt on a background event would be worse than silence.
        if (permission !== "granted") return false;
        const title = runtime.translate(notice.titleKey, notice.params);
        const body = runtime.translate(notice.messageKey, notice.params);
        const instance = new Ctor(title, { body, tag: notice.key });
        // AC-8: the notice is actionable — clicking it raises the delivering tab.
        const focusWindow = runtime.focusWindow ?? null;
        if (focusWindow) {
          instance.onclick = () => {
            try {
              focusWindow();
            } catch {
              // A tab that cannot be focused must not break the click handler.
            }
            instance.close?.();
          };
        }
        return true;
      } catch {
        return false;
      }
    },
  };
}

export function createChimePerformer(runtime: NoticeRuntime): GlobalEffectPerformer {
  return {
    perform: (_notice: GlobalEffectNotice): boolean => {
      try {
        const context = runtime.createAudioContext?.() ?? null;
        if (!context) return false;
        const start = context.currentTime;
        const gain = context.createGain();
        gain.gain.setValueAtTime(0.0001, start);
        gain.gain.exponentialRampToValueAtTime(0.16, start + 0.02);
        gain.gain.exponentialRampToValueAtTime(0.0001, start + CHIME_DURATION_MS / 1000);
        const oscillator = context.createOscillator();
        oscillator.type = "sine";
        oscillator.frequency.setValueAtTime(880, start);
        oscillator.frequency.setValueAtTime(1320, start + 0.12);
        oscillator.connect(gain);
        gain.connect(context.destination);
        oscillator.start(start);
        oscillator.stop(start + CHIME_DURATION_MS / 1000);
        // Autoplay policy: a suspended context plays as soon as the tab is
        // allowed to; the scheduled tone is not lost in that case.
        if (context.state === "suspended") void context.resume?.();
        const schedule = runtime.schedule ?? ((run: () => void, delayMs: number) => void setTimeout(run, delayMs));
        schedule(() => void context.close?.(), CHIME_CLOSE_DELAY_MS);
        return true;
      } catch {
        return false;
      }
    },
  };
}

/**
 * The background-Run reminder is one effect with two channels: the notification
 * carries the text, the chime makes it audible. It counts as delivered when
 * either channel went out, so a denied notification permission still leaves an
 * audible reminder (and vice versa).
 */
export function createRunReminderPerformer(runtime: NoticeRuntime): GlobalEffectPerformer {
  const notification = createDesktopNotificationPerformer(runtime);
  const chime = createChimePerformer(runtime);
  return {
    perform: (notice: GlobalEffectNotice): boolean => {
      const notified = notification.perform(notice);
      const chimed = chime.perform(notice);
      return notified || chimed;
    },
  };
}
