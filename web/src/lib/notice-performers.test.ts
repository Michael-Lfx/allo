import { describe, expect, it } from "vitest";

import type { GlobalEffectNotice } from "./global-effects";
import {
  createChimePerformer,
  createDesktopNotificationPerformer,
  createRunReminderPerformer,
  type AudioContextLike,
  type NoticeRuntime,
} from "./notice-performers";

const NOTICE: GlobalEffectNotice = {
  key: "run:r1:terminal:failed",
  kind: "run-reminder",
  titleKey: "notify.runTerminalTitle",
  messageKey: "toast.runFailed",
};

/** Translator stand-in: records what was asked for and returns a marker text. */
function fakeTranslate() {
  const calls: { key: string; params?: Record<string, unknown> }[] = [];
  return {
    calls,
    translate: (key: string, params?: Record<string, unknown>) => {
      calls.push({ key, params });
      return `[${key}]`;
    },
  };
}

function fakeNotification(permission = "granted") {
  const constructed: {
    title: string;
    options?: { body?: string; tag?: string };
    instance: { onclick?: (() => void) | null; closed: boolean };
  }[] = [];
  class FakeNotification {
    static permission = permission;
    onclick: (() => void) | null = null;
    closed = false;
    constructor(title: string, options?: { body?: string; tag?: string }) {
      constructed.push({ title, options, instance: this });
    }
    close(): void {
      this.closed = true;
    }
  }
  return { constructed, Ctor: FakeNotification };
}

function fakeAudioContext(state = "running") {
  const calls: string[] = [];
  const context: AudioContextLike = {
    state,
    currentTime: 4,
    destination: { name: "destination" },
    createOscillator: () => ({
      type: "",
      frequency: {
        setValueAtTime: (value, start) => calls.push(`freq:${value}@${start}`),
      },
      connect: () => calls.push("osc.connect"),
      start: (when) => calls.push(`start@${when}`),
      stop: (when) => calls.push(`stop@${when}`),
    }),
    createGain: () => ({
      gain: {
        setValueAtTime: (value, start) => calls.push(`gain:${value}@${start}`),
        exponentialRampToValueAtTime: (value, start) => calls.push(`ramp:${value}@${start}`),
      },
      connect: () => calls.push("gain.connect"),
    }),
    resume: () => {
      calls.push("resume");
    },
    close: () => {
      calls.push("close");
    },
  };
  return { calls, context };
}

describe("createDesktopNotificationPerformer", () => {
  it("fires when the permission is granted, resolving text through the translator", () => {
    const { calls, translate } = fakeTranslate();
    const { constructed, Ctor } = fakeNotification("granted");
    const performer = createDesktopNotificationPerformer({
      translate,
      notification: Ctor,
      notificationPermission: () => Ctor.permission,
    });

    expect(performer.perform(NOTICE)).toBe(true);
    expect(constructed.map(({ title, options }) => ({ title, options }))).toEqual([
      { title: "[notify.runTerminalTitle]", options: { body: "[toast.runFailed]", tag: NOTICE.key } },
    ]);
    expect(calls.map((call) => call.key)).toEqual(["notify.runTerminalTitle", "toast.runFailed"]);
  });

  it("stays silent when the permission was never granted (no prompt is raised)", () => {
    const { translate } = fakeTranslate();
    const { constructed, Ctor } = fakeNotification("default");
    const performer = createDesktopNotificationPerformer({
      translate,
      notification: Ctor,
      notificationPermission: () => "default",
    });

    expect(performer.perform(NOTICE)).toBe(false);
    expect(constructed).toHaveLength(0);
  });

  it("stays silent when the browser has no Notification API", () => {
    const { translate } = fakeTranslate();
    const performer = createDesktopNotificationPerformer({ translate, notification: null });
    expect(performer.perform(NOTICE)).toBe(false);
  });

  it("swallows a constructor throw", () => {
    const { translate } = fakeTranslate();
    class Exploding {
      constructor() {
        throw new Error("no notification for you");
      }
    }
    const performer = createDesktopNotificationPerformer({
      translate,
      notification: Exploding,
      notificationPermission: () => "granted",
    });
    expect(performer.perform(NOTICE)).toBe(false);
  });

  it("raises the delivering tab when the notice is clicked", () => {
    const { translate } = fakeTranslate();
    const { constructed, Ctor } = fakeNotification();
    let focused = 0;
    const performer = createDesktopNotificationPerformer({
      translate,
      notification: Ctor,
      notificationPermission: () => "granted",
      focusWindow: () => {
        focused += 1;
      },
    });

    expect(performer.perform(NOTICE)).toBe(true);
    const instance = constructed[0].instance;
    expect(instance.onclick).toBeTypeOf("function");
    instance.onclick?.();
    expect(focused).toBe(1);
    expect(instance.closed).toBe(true);
  });

  it("keeps the click handler harmless when focusing is refused", () => {
    const { translate } = fakeTranslate();
    const { constructed, Ctor } = fakeNotification();
    const performer = createDesktopNotificationPerformer({
      translate,
      notification: Ctor,
      notificationPermission: () => "granted",
      focusWindow: () => {
        throw new Error("focus refused");
      },
    });

    expect(performer.perform(NOTICE)).toBe(true);
    const instance = constructed[0].instance;
    expect(() => instance.onclick?.()).not.toThrow();
    expect(instance.closed).toBe(true);
  });
});

describe("createChimePerformer", () => {
  it("schedules one tone and never ramps the gain to zero", () => {
    const { calls, translate } = fakeTranslate();
    const { calls: audioCalls, context } = fakeAudioContext();
    const scheduled: (() => void)[] = [];
    const performer = createChimePerformer({
      translate,
      createAudioContext: () => context,
      schedule: (run) => scheduled.push(run),
    });

    expect(performer.perform(NOTICE)).toBe(true);
    expect(audioCalls.filter((call) => call.startsWith("start@"))).toHaveLength(1);
    expect(audioCalls).toContain("freq:880@4");
    expect(audioCalls).toContain("freq:1320@4.12");
    const ramps = audioCalls.filter((call) => call.startsWith("ramp:")).map((call) => Number(call.slice(5).split("@")[0]));
    expect(ramps.length).toBeGreaterThan(0);
    for (const value of ramps) expect(value).toBeGreaterThan(0);
    // The context is closed after the tone, through the injected timer.
    expect(audioCalls).not.toContain("close");
    expect(scheduled).toHaveLength(1);
    scheduled[0]();
    expect(audioCalls).toContain("close");
  });

  it("resumes a suspended context instead of dropping the tone", () => {
    const { translate } = fakeTranslate();
    const { calls, context } = fakeAudioContext("suspended");
    const performer = createChimePerformer({ translate, createAudioContext: () => context, schedule: () => {} });

    expect(performer.perform(NOTICE)).toBe(true);
    expect(calls).toContain("resume");
  });

  it("returns false when no audio context can be created", () => {
    const { translate } = fakeTranslate();
    const performer = createChimePerformer({ translate, createAudioContext: () => null });
    expect(performer.perform(NOTICE)).toBe(false);
  });

  it("returns false when the audio context throws", () => {
    const { translate } = fakeTranslate();
    const performer = createChimePerformer({
      translate,
      createAudioContext: () => {
        throw new Error("no audio");
      },
    });
    expect(performer.perform(NOTICE)).toBe(false);
  });
});

describe("createRunReminderPerformer", () => {
  const runtimeWith = (permission: string, audio: boolean): NoticeRuntime => {
    const { translate } = fakeTranslate();
    const { Ctor } = fakeNotification(permission);
    return {
      translate,
      notification: Ctor,
      notificationPermission: () => Ctor.permission,
      createAudioContext: () => (audio ? fakeAudioContext().context : null),
      schedule: () => {},
    };
  };

  it("counts as delivered when either channel went out", () => {
    // Notification denied, chime available.
    expect(createRunReminderPerformer(runtimeWith("denied", true)).perform(NOTICE)).toBe(true);
    // Notification granted, no audio device.
    expect(createRunReminderPerformer(runtimeWith("granted", false)).perform(NOTICE)).toBe(true);
  });

  it("reports a refusal only when both channels refuse", () => {
    expect(createRunReminderPerformer(runtimeWith("denied", false)).perform(NOTICE)).toBe(false);
  });
});
