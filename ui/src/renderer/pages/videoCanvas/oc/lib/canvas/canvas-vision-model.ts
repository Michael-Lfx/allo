import { modelOptionName, type AiConfig } from "@oc/stores/use-config-store";

/** Chat models whose catalog `extra.input` includes `image` (Flowy multimodal). */
export function canvasVisionModelOptions(config: AiConfig): string[] {
    const chat = config.channels.find((channel) => channel.id === "allo-chat");
    const visionIds = new Set(
        (chat?.modelCosts || [])
            .filter((item) => item.capability === "text" && item.supportsVision)
            .map((item) => item.model),
    );
    return (config.textModels || []).filter((encoded) => visionIds.has(modelOptionName(encoded)));
}

export function canvasModelSupportsVision(config: AiConfig, encodedModel: string): boolean {
    const model = modelOptionName(encodedModel);
    const chat = config.channels.find((channel) => channel.id === "allo-chat");
    return chat?.modelCosts?.some((item) => item.model === model && item.supportsVision) === true;
}

/**
 * Prefer the selected canvas text model when it accepts image input; otherwise
 * the first multimodal chat model from the synced Flowy catalog.
 */
export function resolveCanvasVisionModel(config: AiConfig): string {
    const selected = (config.textModel || config.model || "").trim();
    if (selected && canvasModelSupportsVision(config, selected)) return selected;
    return canvasVisionModelOptions(config)[0] || "";
}
