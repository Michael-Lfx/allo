import type { CanvasVideoEditOperation } from "@oc/types/canvas";

export type CanvasVideoInputSummary = {
    textCount?: number;
    imageCount: number;
    videoCount: number;
    audioCount: number;
};

export function defaultVideoOperation(inputSummary: CanvasVideoInputSummary): CanvasVideoEditOperation {
    if (inputSummary.audioCount > 0 && inputSummary.imageCount === 0 && inputSummary.videoCount === 0) return "audio_to_video";
    if (inputSummary.videoCount > 0) return "extend";
    if (inputSummary.imageCount >= 3) return "reference_to_video";
    if (inputSummary.imageCount > 0) return "image_to_video";
    return "text_to_video";
}
