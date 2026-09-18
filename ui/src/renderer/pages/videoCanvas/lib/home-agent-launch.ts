import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canonicalizeVideoResolution } from "@oc/lib/canvas-video-resolution";
import { isMiniMaxH3ResolutionToken } from "@oc/lib/video-generation-options";
import { isMiniMaxH3VideoModel, isWan3VideoModel } from "@renderer/services/videoModelCapabilities";
import { encodeChannelModel, isChannelModelValue } from "@oc/stores/use-config-store";
import { readCreationIr, type CreationIR } from "./creation-ir";

const ALLO_MEDIA_CHANNEL_ID = "allo-media";

export type HomeLaunchConfigPatch = {
  size?: string;
  videoSeconds?: string;
  vquality?: string;
  videoModel?: string;
  imageModel?: string;
};

export type CanvasHomeLaunchSkill = {
  id: string;
  label: string;
  description: string;
  stylePrompt: string;
  /** Lookbook / v2 id when this skill is a visual look. Falls back to `id`. */
  stylePresetId?: string;
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

export type CanvasHomeAgentAutoStart = {
  prompt: string;
  meta: string;
  modelContext: string;
};

export function homeAgentAutoStartFromCreative(creative: unknown): CanvasHomeAgentAutoStart | null {
  const launch = readHomeLaunchSidecar(creative);
  if (!shouldAutoStartHomeAgent(launch) || !launch) return null;
  const ir = readCreationIr(creative);
  return {
    prompt: buildCanvasHomeUserBrief(launch),
    meta: buildCanvasHomeUserMeta(launch),
    modelContext: buildCanvasHomeAgentContext({
      mediaKind: launch.mediaKind,
      skill: launch.skill,
      preferences: launch.preferences,
      requirement: launch.requirement,
      references: launch.referenceMediaIds,
      ir,
    }),
  };
}

export function buildCanvasHomeUserBrief(launch: {
  prompt: string;
  mediaKind: "image" | "video";
  skill?: CanvasHomeLaunchSkill;
  preferences: CanvasHomeLaunchPreferences;
  requirement?: string;
}) {
  return [
    launch.prompt.trim(),
    buildCanvasHomeUserMeta(launch),
    launch.requirement?.trim()
      ? canvasT("videoCanvas.agent.homeLaunchRequirement", "附加说明：{{text}}", { text: launch.requirement.trim() })
      : "",
  ].filter(Boolean).join("\n");
}

export function buildCanvasHomeUserMeta(launch: {
  mediaKind: "image" | "video";
  skill?: CanvasHomeLaunchSkill;
  preferences: CanvasHomeLaunchPreferences;
}) {
  const mediaKind = launch.mediaKind === "image" ? "image" : "video";
  const model = mediaKind === "image" ? launch.preferences.imageModel : launch.preferences.videoModel;
  return [
    mediaKind === "image"
      ? canvasT("videoCanvas.agent.homeLaunchMediaImage", "图片")
      : canvasT("videoCanvas.agent.homeLaunchMediaVideo", "视频"),
    launch.skill?.label?.trim() || "",
    launch.preferences.aspectRatio || "16:9",
    launch.preferences.resolution || "1080p",
    mediaKind === "video" ? `${launch.preferences.targetDurationSecs || 5}秒` : "",
    model?.trim() || "",
  ].filter(Boolean).join(" · ");
}

export function canvasConfigPatchFromHomeLaunch(preferences: CanvasHomeLaunchPreferences | undefined): HomeLaunchConfigPatch {
  if (!preferences) return {};
  const patch: HomeLaunchConfigPatch = {};
  if (preferences.aspectRatio?.trim()) patch.size = preferences.aspectRatio.trim();
  if (preferences.targetDurationSecs) patch.videoSeconds = String(preferences.targetDurationSecs);
  const videoModel = preferences.videoModel?.trim() || "";
  if (preferences.resolution?.trim()) patch.vquality = storedVqualityFromHomeLaunch(preferences);
  if (videoModel) patch.videoModel = encodeHomeMediaModel(videoModel);
  if (preferences.imageModel?.trim()) patch.imageModel = encodeHomeMediaModel(preferences.imageModel.trim());
  return patch;
}

function encodeHomeMediaModel(model: string) {
  return isChannelModelValue(model) ? model : encodeChannelModel(ALLO_MEDIA_CHANNEL_ID, model);
}

export function storedVqualityFromHomeLaunch(preferences: CanvasHomeLaunchPreferences) {
  const videoModel = preferences.videoModel || "";
  const canonical = canonicalizeVideoResolution(videoModel, preferences.resolution);
  return isMiniMaxH3VideoModel(videoModel) || isWan3VideoModel(videoModel) || isMiniMaxH3ResolutionToken(canonical)
    ? canonical
    : String(canonical).replace(/p$/i, "");
}

export function buildCanvasHomeAgentContext(launch: {
  mediaKind: "image" | "video";
  skill?: CanvasHomeLaunchSkill;
  preferences: CanvasHomeLaunchPreferences;
  requirement?: string;
  references?: unknown[];
  ir?: CreationIR | null;
}) {
  const mediaKind = launch.mediaKind === "image" ? "image" : "video";
  const model = mediaKind === "image"
    ? launch.preferences.imageModel
    : launch.preferences.videoModel;
  const subjects = launch.ir?.subjects ?? [];
  const shots = launch.ir?.shots ?? [];
  const extras = [
    launch.skill?.label
      ? canvasT("videoCanvas.agent.homeLaunchLookSlot", "画风槽：{{label}}。只约束视觉，不代表角色身份。", {
          label: launch.skill.label,
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
    subjects.length
      ? canvasT("videoCanvas.agent.homeLaunchSubjects", "已标注主体：{{list}}。保持面孔与场景，不要另造替换身份。", {
          list: subjects.map((subject) => `${subject.name}（${subject.kind}${subject.nodeId ? `/${subject.nodeId}` : ""}）`).join("、"),
        })
      : launch.references?.length
        ? canvasT("videoCanvas.agent.homeLaunchRefs", "参考图已作为画布图片节点。请接到分镜或画面上，不要丢弃。")
        : "",
    shots.length
      ? canvasT("videoCanvas.agent.homeLaunchStoryboard", "分镜已写入 Script 节点（{{count}} 镜）。把镜头写进已有 rows，不要另起一份空分镜。", {
          count: String(shots.length),
        })
      : "",
  ].filter(Boolean);
  return [
    canvasT(
      "videoCanvas.agent.homeLaunchBrief",
      "这是从视频生成首页「创作模式」发起的首轮任务。画布已放入有约束力的 Video Spec、已标注主体（如有）和一份分镜脚本节点。请先 canvas_get_skill 读手册，再用 storyboard_inspect / storyboard_apply 写现有 Script rows，用 spec_inspect / spec_apply 守住规格；canvas_apply 只作图结构兜底（必须含 nodes 和 edges，禁止只传 description）。然后 canvas_run 提交并等待。Look 只是视觉槽。节点要有真实提示词。队列未空时不要说已完成。",
    ),
    ...extras,
  ].join("\n");
}
