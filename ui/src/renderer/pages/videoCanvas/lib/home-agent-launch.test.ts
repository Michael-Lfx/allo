import { describe, expect, test } from "bun:test";
import i18n from "i18next";

import {
  buildCanvasHomeAgentContext,
  buildCanvasHomeUserBrief,
  canvasConfigPatchFromHomeLaunch,
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
    expect(autoStart?.prompt).toContain("噜噜跳舞");
    expect(autoStart?.prompt).toContain("16:9");
    expect(autoStart?.prompt).toContain("5秒");
    expect(autoStart?.prompt).toContain("demo-video");
    expect(autoStart?.meta).toContain("16:9");
    expect(autoStart?.meta).toContain("demo-video");
    expect(autoStart?.modelContext).toContain("canvas_apply");
    expect(autoStart?.modelContext).not.toContain("画布几乎为空");
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
    expect(context).toContain("canvas_apply");
    expect(context).toContain("电影写实");
    expect(context).toContain("16:9");
    expect(context).toContain("demo-video");
    expect(context).toContain("竖屏也可");
    expect(context).toContain("canvas_run");
    expect(context).toContain("必须含 nodes");
    expect(context).not.toContain("必须是：分镜脚本");
    expect(context).not.toContain("请先 canvas_get_context");
    expect(context).not.toContain("媒介路径参考");
    expect(context).not.toContain("接入流水线");
  });

  test("buildCanvasHomeUserBrief shows homepage constraints in the chat bubble", () => {
    const brief = buildCanvasHomeUserBrief({
      prompt: "小猫的一天",
      mediaKind: "video",
      skill: { id: "anime", label: "二次元", description: "", stylePrompt: "anime" },
      preferences: { ...preferences, targetDurationSecs: 6, aspectRatio: "16:9", resolution: "720p" },
      requirement: "温馨日常",
    });
    expect(brief).toContain("小猫的一天");
    expect(brief).toContain("二次元");
    expect(brief).toContain("16:9");
    expect(brief).toContain("720p");
    expect(brief).toContain("6秒");
    expect(brief).toContain("demo-video");
    expect(brief).toContain("温馨日常");
  });

  test("canvasConfigPatchFromHomeLaunch encodes media models and duration", () => {
    const patch = canvasConfigPatchFromHomeLaunch({
      ...preferences,
      targetDurationSecs: 6,
      aspectRatio: "16:9",
      resolution: "720p",
    });
    expect(patch.size).toBe("16:9");
    expect(patch.videoSeconds).toBe("6");
    expect(patch.videoModel).toBe("allo-media::demo-video");
    expect(patch.vquality).toBeTruthy();
  });
});
