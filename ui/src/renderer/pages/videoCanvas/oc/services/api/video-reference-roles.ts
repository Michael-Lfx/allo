export type VideoImageRole = "first_frame" | "last_frame" | "reference_image";

export type VideoReferenceImage = {
    id: string;
};

export type ResolvedVideoImageReference<T extends VideoReferenceImage> = {
    image: T;
    role: VideoImageRole;
};

type VideoReferenceOptions = {
    videoEditOperation?: string;
    videoStartFrameNodeId?: string;
    videoEndFrameNodeId?: string;
};

type VideoReferenceContext = {
    videoCount?: number;
    audioCount?: number;
};

/**
 * Seedance forbids mixing first/last_frame with extra reference_image slots.
 * Unlabeled three-or-more stills still go as reference_to_video so middle frames are not dropped.
 * Named first/last on one or two stills stay image_to_video — do not infer the fork from count alone when roles are labeled.
 */
export function shouldSubmitVideoImagesAsReferences(options?: VideoReferenceOptions, imageCount = 0) {
    if (options?.videoEditOperation === "reference_to_video") return true;
    const named = Boolean(options?.videoStartFrameNodeId?.trim() || options?.videoEndFrameNodeId?.trim());
    if (named) return imageCount >= 3;
    return imageCount >= 3;
}

export function videoFramePatchFromStillRole(imageNodeId: string | undefined, stillRole?: string) {
    if (!imageNodeId) return {};
    if (stillRole === "last") {
        return { videoEditOperation: "image_to_video" as const, videoStartFrameNodeId: undefined, videoEndFrameNodeId: imageNodeId };
    }
    if (stillRole === "reference") {
        return { videoEditOperation: "reference_to_video" as const, videoStartFrameNodeId: undefined, videoEndFrameNodeId: undefined };
    }
    return { videoEditOperation: "image_to_video" as const, videoStartFrameNodeId: imageNodeId, videoEndFrameNodeId: undefined };
}

export function videoEditOperationForKeyframeCount(count: number): "image_to_video" | "reference_to_video" | undefined {
    if (count <= 0) return undefined;
    return count >= 3 ? "reference_to_video" : "image_to_video";
}

export function resolveVideoImageReferences<T extends VideoReferenceImage>(
    images: T[],
    options?: VideoReferenceOptions,
    context: VideoReferenceContext = {},
): ResolvedVideoImageReference<T>[] {
    const operation = options?.videoEditOperation?.trim();
    if (shouldSubmitVideoImagesAsReferences(options, images.length) || operation === "reference_to_video") {
        return images.map((image) => ({ image, role: "reference_image" as const }));
    }

    const startFrameId = options?.videoStartFrameNodeId?.trim();
    const endFrameId = options?.videoEndFrameNodeId?.trim();
    if (startFrameId || endFrameId) {
        return images.map((image) => ({
            image,
            role: image.id === startFrameId ? "first_frame" : image.id === endFrameId ? "last_frame" : "reference_image",
        }));
    }

    if (context.videoCount || context.audioCount) {
        return images.map((image) => ({ image, role: "reference_image" as const }));
    }

    return images.map((image, index) => ({
        image,
        role: index === 0 ? "first_frame" : index === 1 && images.length === 2 ? "last_frame" : "reference_image",
    }));
}

export function hasExplicitVideoFrames(options?: VideoReferenceOptions) {
    return options?.videoEditOperation !== "reference_to_video" && Boolean(options?.videoStartFrameNodeId || options?.videoEndFrameNodeId);
}
