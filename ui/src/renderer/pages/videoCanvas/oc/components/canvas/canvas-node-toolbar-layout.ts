/** Primary node-strip ids by node kind. Overflow still receives the rest of the registry. */
export type NodeDockKindInput = {
    isCharacterReference: boolean;
    isImage: boolean;
    hasImage: boolean;
    isVideo: boolean;
    hasVideo: boolean;
    isAudio: boolean;
    hasAudio: boolean;
    isEditableText: boolean;
    isDrawing: boolean;
};

export function resolveNodeDockPrimaryIds(input: NodeDockKindInput, imageVisibleIds: string[]): string[] {
    if (input.isCharacterReference) return ["delete", "info"];
    if (input.isImage && !input.hasImage) return ["uploadImage", "edit", "info", "delete"];
    if (input.isImage) return imageVisibleIds;
    if (input.isVideo && !input.hasVideo) return ["uploadVideo", "edit", "info", "delete"];
    if (input.isVideo) return ["delete", "download", "edit", "subtitles", "timeline", "extractFrames", "uploadVideo"];
    if (input.isAudio && !input.hasAudio) return ["uploadAudio", "info", "delete"];
    if (input.isAudio) return ["delete", "download", "timeline", "uploadAudio"];
    if (input.isEditableText) return ["delete", "edit", "editText", "decreaseFont", "increaseFont", "generateImage", "saveAsset"];
    if (input.isDrawing) return ["openDrawing", "info", "delete"];
    return ["delete", "info", "config"];
}
