/**
 * Model-canonical video resolution for canvas → Flowy create APIs.
 * Re-exports the shared normalizer so canvas call sites stay explicit.
 */
import { modelOptionName } from "@oc/stores/use-config-store";
import {
  normalizeVideoResolution as normalizeForModel,
  type VideoResolution,
} from "@renderer/services/videoModelCapabilities";

export function canonicalizeVideoResolution(model: string, value: string | number | undefined): VideoResolution {
  return normalizeForModel(modelOptionName(model || ""), String(value ?? "").trim() || "720p");
}
