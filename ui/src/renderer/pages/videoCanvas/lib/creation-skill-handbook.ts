import type { CreationSpec } from "./creation-ir";

export function creationSkillHandbook(input: {
  lookLabel?: string;
  stylePrompt?: string;
  spec: Pick<CreationSpec, "aspectRatio" | "resolution" | "durationSecs" | "mediaKind" | "imageModel" | "videoModel">;
}): string {
  const model = input.spec.mediaKind === "image" ? input.spec.imageModel : input.spec.videoModel;
  const look = input.lookLabel?.trim();
  const style = input.stylePrompt?.trim();
  return [
    "# Creation skill handbook",
    "",
    "Look / style is a visual slot only. It does not name a character, location, or identity.",
    look ? `Selected look: ${look}.` : "No look selected.",
    style ? `Style prompt: ${style}` : "",
    "",
    "## Binding spec",
    `- Aspect: ${input.spec.aspectRatio}`,
    `- Resolution: ${input.spec.resolution}`,
    `- Duration: ${input.spec.durationSecs}s`,
    `- Medium: ${input.spec.mediaKind}`,
    `- Model: ${model || "canvas default"}`,
    "",
    "## Loop",
    "1. storyboard_inspect — read shots and gaps. Do not invent a second script node.",
    "2. subject_inspect — keep labeled subjects; do not replace faces or locations.",
    "3. spec_inspect / spec_apply — honor aspect, duration, resolution, model.",
    "4. storyboard_apply — patch existing Script rows (plot, duration, prompts).",
    "5. canvas_apply — only for graph fallback (stills/videos/edges) when rows already exist.",
    "6. canvas_run — submit and wait. Never claim done while the queue is busy.",
    "",
    "## Stop gates",
    "- Empty storyboard: write shots into the existing Script node, then stop if still empty.",
    "- Missing subject still: attach the labeled image node; do not generate a replacement identity.",
    "- Missing model: spec_apply the model from Spec, then continue.",
    "- Shot status loading/running: skip that shot; humans may edit idle shots in parallel.",
    "",
    "## Prompts",
    "- Still prompt describes the locked frame, lighting, and subject continuity.",
    "- Motion prompt describes camera and performance only; do not restyle the character.",
    look ? `- Apply look "${look}" as lighting/grade/lens, not as a new cast.` : "",
    "",
    "## Edit roles",
    "Label stills as first / last / reference. Do not infer first+last vs multi-reference from image count.",
  ].filter((line) => line !== "").join("\n");
}
