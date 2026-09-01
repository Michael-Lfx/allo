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

export function resolveVideoImageReferences<T extends VideoReferenceImage>(
    images: T[],
    options?: VideoReferenceOptions,
    context: VideoReferenceContext = {},
): ResolvedVideoImageReference<T>[] {
    const operation = options?.videoEditOperation?.trim();
    if (operation === "reference_to_video") {
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
