/**
 * Browser wiring for the global-effect gate (D4=A / W8 余项).
 *
 * This is the only file in the multi-tab stack that touches globals
 * (`navigator.locks`, `localStorage`, `BroadcastChannel`, `Notification`,
 * `AudioContext`) and the i18n singleton. It stays out of the unit tests on
 * purpose: `global-effects.ts` + `notice-performers.ts` are the tested cores,
 * this module only assembles them from what the current browser offers.
 *
 * Every capability is probed independently, so a partial browser (no Web Locks,
 * no BroadcastChannel, notifications denied) degrades along the documented
 * ladder instead of dropping the notice.
 */

import i18n from "../i18n";
import {
  createGlobalEffectGate,
  type BroadcastChannelLike,
  type GlobalEffectGate,
  type StorageLike,
} from "./global-effects";
import {
  createDesktopNotificationPerformer,
  createChimePerformer,
  createRunReminderPerformer,
  type AudioContextLike,
  type NotificationConstructorLike,
  type NoticeRuntime,
} from "./notice-performers";

const CHANNEL_NAME = "allo-global-effects";

function translate(key: string, params?: Record<string, unknown>): string {
  return String(i18n.t(key, { ...(params ?? {}) }));
}

/** `localStorage` throws in some privacy modes — probe it, do not assume it. */
function sharedStorage(): StorageLike | null {
  try {
    const probe = `${CHANNEL_NAME}:probe`;
    globalThis.localStorage?.setItem(probe, "1");
    globalThis.localStorage?.removeItem(probe);
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

function lockManager() {
  return typeof globalThis.navigator !== "undefined" && globalThis.navigator.locks ? globalThis.navigator.locks : null;
}

function broadcastChannel() {
  const Ctor = (globalThis as { BroadcastChannel?: new (name: string) => BroadcastChannel }).BroadcastChannel;
  if (typeof Ctor !== "function") return null;
  try {
    // The DOM's `onmessage` is typed against the full `MessageEvent`; the gate
    // only reads `data`, so the narrower structural type is widened here.
    return new Ctor(CHANNEL_NAME) as unknown as BroadcastChannelLike;
  } catch {
    return null;
  }
}

function notificationConstructor(): NotificationConstructorLike | null {
  const Ctor = (globalThis as { Notification?: unknown }).Notification;
  return typeof Ctor === "function" ? (Ctor as NotificationConstructorLike) : null;
}

function createAudioContext(): AudioContextLike | null {
  const scope = globalThis as {
    AudioContext?: new () => unknown;
    webkitAudioContext?: new () => unknown;
  };
  const Ctor = scope.AudioContext ?? scope.webkitAudioContext;
  if (typeof Ctor !== "function") return null;
  try {
    return new Ctor() as AudioContextLike;
  } catch {
    return null;
  }
}

function noticeRuntime(): NoticeRuntime {
  const notification = notificationConstructor();
  return {
    translate,
    notification,
    notificationPermission: () => {
      const scope = globalThis as { Notification?: { permission?: string } };
      // No Notification API at all counts as refused: the performer returns false
      // and the chime still gets its chance.
      return scope.Notification?.permission ?? "denied";
    },
    createAudioContext,
    // AC-8: clicking a background-Run notice brings the delivering tab forward.
    focusWindow: () => {
      try {
        globalThis.window?.focus();
      } catch {
        // Focusing can be refused (popup policy); the notice still stands.
      }
    },
  };
}

export function createRuntimeGlobalEffectGate(): GlobalEffectGate {
  const runtime = noticeRuntime();
  return createGlobalEffectGate({
    locks: lockManager(),
    storage: sharedStorage(),
    channel: broadcastChannel(),
    performers: {
      "desktop-notification": createDesktopNotificationPerformer(runtime),
      sound: createChimePerformer(runtime),
      "run-reminder": createRunReminderPerformer(runtime),
    },
  });
}

let gate: GlobalEffectGate | null = null;

/** Process-wide gate; created on first use (no module-import side effects). */
export function getGlobalEffectGate(): GlobalEffectGate {
  if (!gate) gate = createRuntimeGlobalEffectGate();
  return gate;
}

/** Drop the gate and its peer-announcement listener (hot reload / tests). */
export function resetGlobalEffectGate(): void {
  gate?.dispose();
  gate = null;
}
