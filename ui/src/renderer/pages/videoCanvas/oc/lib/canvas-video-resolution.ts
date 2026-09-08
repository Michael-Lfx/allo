/**
 * Model-canonical video resolution for canvas → Flowy create APIs.
 * Re-exports the shared normalizer so canvas call sites stay explicit.
 */
import { modelOptionName } from "@oc/stores/use-config-store";
import { isMiniMaxH3ResolutionToken } from "@oc/lib/video-generation-options";
import {
  isMiniMaxH3VideoModel,
  isWan3VideoModel,
  normalizeVideoResolution as normalizeForModel,
  type VideoResolution,
} from "@renderer/services/videoModelCapabilities";

export function canonicalizeVideoResolution(model: string, value: string | number | undefined): VideoResolution {
  return normalizeForModel(modelOptionName(model || ""), String(value ?? "").trim() || "720p");
}

/** Persist UI `vquality`: MiniMax/Wan keep canonical tokens; Seedance/generic keep bare heights for legacy pills. */
export function storeVqualityForUi(model: string, value: string) {
  const canonical = canonicalizeVideoResolution(model, value);
  if (isMiniMaxH3VideoModel(model) || isWan3VideoModel(model) || isMiniMaxH3ResolutionToken(canonical)) return canonical;
  return String(canonical).replace(/p$/i, "");
}
