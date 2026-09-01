import { canvasT } from "@oc/lib/canvas/canvas-i18n";

export type CanvasHomeLaunchSkill = {
  id: string;
  label: string;
  description: string;
  stylePrompt: string;
};

export type CanvasHomeLaunchPreferences = {
  automatic: boolean;
  aspectRatio: string;
  resolution: string;
  fps: number;
  targetDurationSecs: number;
  imageModel?: string;
  videoModel?: string;
};

export type CanvasHomeLaunchSidecar = {
  schema: 1;
  intent: "creation" | "generate";
  autoGenerate: boolean;
  autoAgent: boolean;
  agentBriefSent: boolean;
  prompt: string;
  requirement?: string;
  mediaKind: "image" | "video";
  skill?: CanvasHomeLaunchSkill;
  preferences: CanvasHomeLaunchPreferences;
  referenceMediaIds: string[];
  createdAt: string;
};

export function isCanvasHomeAgentLaunch(launch: { intent?: "creation" | "generate"; autoAgent?: boolean }) {
  return launch.autoAgent === true && launch.intent !== "generate";
}

export function readHomeLaunchSidecar(creative: unknown): CanvasHomeLaunchSidecar | undefined {
  if (!creative || typeof creative !== "object" || !("homeLaunch" in creative)) return undefined;
  const raw = (creative as { homeLaunch?: unknown }).homeLaunch;
  if (!raw || typeof raw !== "object") return undefined;
  const launch = raw as Partial<CanvasHomeLaunchSidecar>;
  if (typeof launch.prompt !== "string" || !launch.prompt.trim()) return undefined;
  return {
    schema: 1,
    intent: launch.intent === "generate" ? "generate" : "creation",
    autoGenerate: launch.autoGenerate === true,
    autoAgent: launch.autoAgent === true,
    agentBriefSent: launch.agentBriefSent === true,
    prompt: launch.prompt.trim(),
    requirement: typeof launch.requirement === "string" ? launch.requirement : undefined,
    mediaKind: launch.mediaKind === "image" ? "image" : "video",
    skill: launch.skill,
    preferences: launch.preferences || {
      automatic: true,
      aspectRatio: "16:9",
      resolution: "1080p",
      fps: 24,
      targetDurationSecs: 5,
    },
    referenceMediaIds: Array.isArray(launch.referenceMediaIds)
      ? launch.referenceMediaIds.filter((id): id is string => typeof id === "string")
      : [],
    createdAt: typeof launch.createdAt === "string" ? launch.createdAt : new Date().toISOString(),
  };
}

export function shouldAutoStartHomeAgent(launch: CanvasHomeLaunchSidecar | undefined) {
  return Boolean(launch && launch.autoAgent && !launch.agentBriefSent && launch.intent !== "generate" && launch.prompt);
}

export function homeAgentAutoStartFromCreative(creative: unknown): { prompt: string; modelContext: string } | null {
  const launch = readHomeLaunchSidecar(creative);
  if (!shouldAutoStartHomeAgent(launch) || !launch) return null;
  return {
    prompt: launch.prompt,
    modelContext: buildCanvasHomeAgentContext({
      mediaKind: launch.mediaKind,
      skill: launch.skill,
      preferences: launch.preferences,
      requirement: launch.requirement,
      references: launch.referenceMediaIds,
    }),
  };
}

export function buildCanvasHomeAgentContext(launch: {
  mediaKind: "image" | "video";
  skill?: CanvasHomeLaunchSkill;
  preferences: CanvasHomeLaunchPreferences;
  requirement?: string;
  references?: unknown[];
}) {
  const mediaKind = launch.mediaKind === "image" ? "image" : "video";
  const pipeline = mediaKind === "image"
    ? canvasT("videoCanvas.agent.homeLaunchPipelineImage", "提示词 → 图片")
    : canvasT("videoCanvas.agent.homeLaunchPipelineVideo", "提示词 → 关键帧图 → 视频");
  const model = mediaKind === "image"
    ? launch.preferences.imageModel
    : launch.preferences.videoModel;
  const extras = [
    launch.skill?.label
      ? canvasT("videoCanvas.agent.homeLaunchStyle", "视觉风格：{{label}}（{{description}}）", {
          label: launch.skill.label,
          description: launch.skill.description || "",
        })
      : "",
    launch.skill?.stylePrompt
      ? canvasT("videoCanvas.agent.homeLaunchStylePrompt", "风格提示：{{prompt}}", { prompt: launch.skill.stylePrompt })
      : "",
    canvasT("videoCanvas.agent.homeLaunchPrefs", "媒介：{{media}}；画幅：{{ratio}}；分辨率：{{resolution}}；时长：{{duration}}秒；模型：{{model}}", {
      media: mediaKind === "image"
        ? canvasT("videoCanvas.agent.homeLaunchMediaImage", "图片")
        : canvasT("videoCanvas.agent.homeLaunchMediaVideo", "视频"),
      ratio: launch.preferences.aspectRatio || "16:9",
      resolution: launch.preferences.resolution || "1080p",
      duration: String(launch.preferences.targetDurationSecs || 5),
      model: model || canvasT("videoCanvas.agent.homeLaunchModelAuto", "按画布默认"),
    }),
    launch.requirement?.trim()
      ? canvasT("videoCanvas.agent.homeLaunchRequirement", "附加说明：{{text}}", { text: launch.requirement.trim() })
      : "",
    launch.references?.length
      ? canvasT("videoCanvas.agent.homeLaunchRefs", "参考图已作为画布图片节点，请接入流水线作为角色或画面参考。")
      : "",
  ].filter(Boolean);
  return [
    canvasT(
      "videoCanvas.agent.homeLaunchBrief",
      "这是从视频生成首页「创作模式」发起的首轮任务，等同于用户在画布 Agent 对话框发送需求。画布目前几乎为空。用户消息已含画布快照，直接用 canvas_create_workflow 搭建可执行流水线（{{pipeline}}），节点必须带真实提示词，套用下列风格与生成参数，autoRun 为 true，并 canvas_wait_generation 等到完成。禁止只放空配置卡或风格技能卡。不要先 canvas_get_context。",
      { pipeline },
    ),
    ...extras,
  ].join("\n");
}
