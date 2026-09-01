import { describe, expect, test } from "bun:test";
import i18n from "i18next";

import {
  buildCanvasHomeAgentContext,
  homeAgentAutoStartFromCreative,
  isCanvasHomeAgentLaunch,
  readHomeLaunchSidecar,
  shouldAutoStartHomeAgent,
} from "./home-agent-launch";

i18n.init({ lng: "zh-CN", fallbackLng: "zh-CN", resources: { "zh-CN": { translation: {} } }, initImmediate: false });

const preferences = {
  automatic: true,
  aspectRatio: "16:9",
  resolution: "1080p",
  fps: 24,
  targetDurationSecs: 5,
  videoModel: "demo-video",
};

describe("home agent launch", () => {
  test("creation autoAgent launches the canvas Agent, generate does not", () => {
    expect(isCanvasHomeAgentLaunch({ intent: "creation", autoAgent: true })).toBe(true);
    expect(isCanvasHomeAgentLaunch({ intent: "generate", autoAgent: true })).toBe(false);
    expect(isCanvasHomeAgentLaunch({ intent: "creation" })).toBe(false);
  });

  test("homeAgentAutoStartFromCreative builds a sendable first turn", () => {
    const autoStart = homeAgentAutoStartFromCreative({
      homeLaunch: {
        autoAgent: true,
        prompt: "噜噜跳舞",
        preferences,
      },
    });
    expect(autoStart?.prompt).toBe("噜噜跳舞");
    expect(autoStart?.modelContext).toContain("canvas_create_workflow");
    expect(homeAgentAutoStartFromCreative({
      homeLaunch: { autoAgent: true, prompt: "噜噜跳舞", agentBriefSent: true, preferences },
    })).toBeNull();
  });

  test("shouldAutoStartHomeAgent requires an unsent creation brief", () => {
    const launch = readHomeLaunchSidecar({
      homeLaunch: {
        autoAgent: true,
        prompt: "噜噜跳舞",
        preferences,
      },
    });
    expect(shouldAutoStartHomeAgent(launch)).toBe(true);
    expect(shouldAutoStartHomeAgent(launch && { ...launch, agentBriefSent: true })).toBe(false);
  });

  test("buildCanvasHomeAgentContext asks for a runnable video workflow instead of empty cards", () => {
    const context = buildCanvasHomeAgentContext({
      mediaKind: "video",
      skill: {
        id: "cinematic",
        label: "电影写实",
        description: "纪实光影 · 叙事镜头",
        stylePrompt: "cinematic lighting",
      },
      preferences,
      requirement: "竖屏也可",
    });
    expect(context).toContain("canvas_create_workflow");
    expect(context).toContain("电影写实");
    expect(context).toContain("16:9");
    expect(context).toContain("demo-video");
    expect(context).toContain("竖屏也可");
    expect(context).toContain("canvas_wait_generation");
    expect(context).not.toContain("请先 canvas_get_context");
  });
});
