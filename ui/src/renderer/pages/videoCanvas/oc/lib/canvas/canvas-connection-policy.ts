import { modelCapabilityConfigFor } from "@oc/lib/model-capabilities";
import { getNodeGenerationMode, getNodeInputKind } from "@oc/lib/canvas/node-registry";
import type { AiConfig } from "@oc/stores/use-config-store";
import { type CanvasConnection, type CanvasNodeData } from "@oc/types/canvas";

type ConnectionCandidate = Pick<CanvasConnection, "fromNodeId" | "toNodeId">;
type CanvasConnectionPolicyOptions = {
  // 仅跳过参考素材数量上限，媒体类型不兼容仍然拒绝。
  ignoreCapacity?: boolean;
};

export type ModelInputSummary = {
  textCount: number;
  imageCount: number;
  videoCount: number;
  audioCount: number;
  characterCount: number;
};

/**
 * Ported from open-ai-canvas connection policy (v1.0.50+).
 * Uses allo model capability profiles for reference capacity checks.
 */
export function canvasConnectionError(
  config: AiConfig,
  nodes: CanvasNodeData[],
  connections: CanvasConnection[],
  candidate: ConnectionCandidate,
  options: CanvasConnectionPolicyOptions = {},
) {
  const target = nodes.find((node) => node.id === candidate.toNodeId);
  if (!target) return "找不到连线目标节点";
  const mode = getNodeGenerationMode(target);
  if (!mode) return "";
  const input = connectionInputSummary(target.id, nodes, connections, candidate);
  const visualInputCount = input.imageCount + input.characterCount;

  if (mode === "image") {
    if (input.videoCount > 0) return "图片生成节点不能连接参考视频";
    if (input.audioCount > 0) return "图片生成节点不能连接参考音频";
    return options.ignoreCapacity ? "" : capacityError(config, mode, "image", visualInputCount, "参考图");
  }
  if (mode === "video") {
    return options.ignoreCapacity
      ? ""
      : capacityError(config, mode, "image", visualInputCount, "参考图") ||
          capacityError(config, mode, "video", input.videoCount, "参考视频") ||
          capacityError(config, mode, "audio", input.audioCount, "参考音频");
  }
  if (mode === "text" && input.audioCount > 0) return "文本生成节点不能连接参考音频";
  if (mode === "audio" && input.characterCount > 1) return "角色配音一次只能连接一个角色卡";
  if (mode === "audio" && (input.imageCount > 0 || input.videoCount > 0 || input.audioCount > 0)) {
    return "音频生成节点只接受文本或单个角色卡输入";
  }
  return "";
}

export function connectionInputSummary(
  targetNodeId: string,
  nodes: CanvasNodeData[],
  connections: CanvasConnection[],
  candidate?: ConnectionCandidate,
): ModelInputSummary {
  const sourceIds = new Set(
    [...connections, ...(candidate ? [{ id: "candidate", ...candidate }] : [])]
      .filter((connection) => connection.toNodeId === targetNodeId)
      .map((connection) => connection.fromNodeId),
  );
  const input: ModelInputSummary = { textCount: 0, imageCount: 0, videoCount: 0, audioCount: 0, characterCount: 0 };
  sourceIds.forEach((sourceId) => {
    const source = nodes.find((node) => node.id === sourceId);
    if (!source) return;
    const inputKind = getNodeInputKind(source.type);
    if (!inputKind) return;
    if (source.metadata?.workflowKind === "character") input.characterCount += 1;
    else input[`${inputKind}Count`] += 1;
  });
  return input;
}

function capacityError(
  config: AiConfig,
  capability: "image" | "video",
  kind: "image" | "video" | "audio",
  count: number,
  label: string,
) {
  const maximum = maxInputCapacity(config, capability, kind);
  if (maximum === null || count <= maximum) return "";
  const unit = kind === "image" ? "张" : "个";
  return maximum > 0 ? `已配置模型最多支持 ${maximum} ${unit}${label}` : `已配置模型均不支持${label}`;
}

function maxInputCapacity(config: AiConfig, capability: "image" | "video", kind: "image" | "video" | "audio") {
  const model =
    capability === "image"
      ? config.imageModel || config.model
      : config.videoModel || config.model;
  if (!model) return null;
  // Allo capability profiles are currently video-centric; image refs fall back to a safe default.
  const profile = modelCapabilityConfigFor(config, model);
  const refs = profile.video?.references;
  if (capability === "image") {
    if (kind !== "image") return 0;
    return refs?.maxImages ?? 9;
  }
  if (kind === "image") return refs?.maxImages ?? null;
  if (kind === "video") return refs?.maxVideos ?? null;
  return refs?.maxAudios ?? null;
}
