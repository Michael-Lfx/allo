import type { ModelProtocol } from "@oc/lib/model-protocols";
import { MINIMAX_H3_DURATION_DEFAULT, MINIMAX_H3_DURATION_MAX, MINIMAX_H3_DURATION_MIN } from "@oc/lib/minimax-h3-video";
import { WAN3_DURATION_DEFAULT, WAN3_DURATION_MAX, WAN3_DURATION_MIN } from "@oc/lib/wan3-video";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { storeVqualityForUi } from "@oc/lib/canvas-video-resolution";
import {
    DEFAULT_MINIMAX_H3_RESOLUTION,
    DEFAULT_VIDEO_RESOLUTION,
    DEFAULT_WAN3_RESOLUTION,
    isMiniMaxH3VideoModel,
    isWan3VideoModel,
    MINIMAX_H3_RESOLUTIONS,
    WAN3_RESOLUTIONS,
    videoModelCapabilities,
} from "@renderer/services/videoModelCapabilities";
import { DEFAULT_SEEDANCE_ASPECT_RATIO, SEEDANCE_ASPECT_RATIOS } from "@renderer/pages/videoGeneration/aspectRatios";
import {
    isSeedanceFastModel,
    isSeedanceVideoModel,
    SEEDANCE_REFERENCE_LIMITS,
} from "@oc/lib/seedance-video";

/** Canvas maps video/audio refs to these ops; Seedance + MiniMax-H3 both accept them. */
export const VIDEO_REFERENCE_OPERATIONS = ["text_to_video", "image_to_video", "reference_to_video", "extend", "audio_to_video"] as const;
const BASIC_VIDEO_OPERATIONS = ["text_to_video", "image_to_video"] as const;

export type ModelCapabilityConfig = {
    version: number;
    video?: VideoCapabilityConfig;
    image?: ImageCapabilityConfig;
};

export type VideoCapabilityConfig = {
    references: {
        promptMaxChars: number;
        maxImages: number;
        maxImageBytes: number;
        maxVideos: number;
        maxVideoBytes: number;
        maxVideoDurationSeconds: number;
        maxAudios: number;
        maxAudioBytes: number;
        maxAudioDurationSeconds: number;
    };
    duration: {
        selection: "range" | "enum";
        min?: number;
        max?: number;
        step?: number;
        values?: number[];
        default: number;
    };
    ratios: string[];
    defaultRatio: string;
    resolutions: string[];
    defaultResolution: string;
    generateAudio: { supported: boolean; default: boolean };
    watermark: { supported: boolean; default: boolean };
    operations: string[];
    defaultOperation: string;
};

export type ImageAspectIcon = "square" | "landscape" | "portrait" | "auto";

export type ImageAspectOption = {
    value: string;
    label: string;
    width: number;
    height: number;
    size?: string;
    icon: ImageAspectIcon;
};

export type ImageCapabilityConfig = {
    aspects: ImageAspectOption[];
    defaultSize: string;
    qualities: string[];
    defaultQuality: string;
    transparentBackground: boolean;
    customPixels: boolean;
    maxCount: number;
};

export type CapabilityLookupConfig = {
    channels: Array<{
        id: string;
        models: string[];
        modelCosts?: Array<{ model: string; capabilityConfig?: ModelCapabilityConfig; protocol?: ModelProtocol }>;
    }>;
};

export type CanvasMediaSpecValues = {
    size?: string;
    quality?: string;
    seconds?: string;
    vquality?: string;
    generateAudio?: string;
    watermark?: string;
    transparentBackground?: string;
};

export type CanvasMediaSpecPatch = {
    size: string;
    quality: string;
    seconds: string;
    vquality: string;
    generateAudio: string;
    watermark: string;
    transparentBackground: string;
};

function modelBlob(model: string) {
    return model.toLowerCase().replace(/[_.\s/]/g, "-");
}

function splitModelRef(model: string) {
    const separator = model.indexOf("::");
    return {
        channelId: separator >= 0 ? model.slice(0, separator) : "",
        modelName: separator >= 0 ? model.slice(separator + 2) : model,
    };
}

function isKlingVideoModel(model: string) {
    return modelBlob(model).includes("kling");
}

function isGrokImagineVideoModel(model: string) {
    const blob = modelBlob(model);
    return blob.includes("grok") && blob.includes("video");
}

function isGrokImagineImageModel(model: string) {
    const blob = modelBlob(model);
    return blob.includes("grok") && blob.includes("imagine") && !blob.includes("video");
}

function isSoraVideoModel(model: string) {
    return modelBlob(model).includes("sora");
}

function isVeoVideoModel(model: string) {
    return modelBlob(model).includes("veo");
}

function isHailuoVideoModel(model: string) {
    const blob = modelBlob(model);
    return blob.includes("hailuo") && !isMiniMaxH3VideoModel(model);
}

function isJimengVideoModel(model: string) {
    const blob = modelBlob(model);
    return blob.includes("jimeng") || blob.includes("cogvideo");
}

function isSeedreamImageModel(model: string) {
    const blob = modelBlob(model);
    return blob.includes("seedream") || blob.includes("doubao-seedream");
}

function isGptImageModel(model: string) {
    const blob = modelBlob(model);
    return blob.includes("gpt-image") || blob.includes("dall-e") || blob.includes("dalle");
}

function basicVideoRefs(overrides: Partial<VideoCapabilityConfig["references"]> = {}): VideoCapabilityConfig["references"] {
    return {
        promptMaxChars: 1000,
        maxImages: 9,
        maxImageBytes: 30 * 1024 * 1024,
        maxVideos: 3,
        maxVideoBytes: 50 * 1024 * 1024,
        maxVideoDurationSeconds: 15,
        maxAudios: 3,
        maxAudioBytes: 15 * 1024 * 1024,
        maxAudioDurationSeconds: 15,
        ...overrides,
    };
}

function minimaxH3VideoCapability(): VideoCapabilityConfig {
    return {
        references: {
            promptMaxChars: 7000,
            maxImages: 9,
            maxImageBytes: 30 * 1024 * 1024,
            maxVideos: 3,
            maxVideoBytes: 50 * 1024 * 1024,
            maxVideoDurationSeconds: 15,
            maxAudios: 3,
            maxAudioBytes: 15 * 1024 * 1024,
            maxAudioDurationSeconds: 15,
        },
        duration: {
            selection: "range",
            min: MINIMAX_H3_DURATION_MIN,
            max: MINIMAX_H3_DURATION_MAX,
            step: 1,
            default: MINIMAX_H3_DURATION_DEFAULT,
        },
        ratios: [...SEEDANCE_ASPECT_RATIOS],
        defaultRatio: DEFAULT_SEEDANCE_ASPECT_RATIO,
        resolutions: [...MINIMAX_H3_RESOLUTIONS],
        defaultResolution: DEFAULT_MINIMAX_H3_RESOLUTION,
        generateAudio: { supported: false, default: false },
        watermark: { supported: false, default: false },
        operations: [...VIDEO_REFERENCE_OPERATIONS],
        defaultOperation: "text_to_video",
    };
}

function wan3VideoCapability(): VideoCapabilityConfig {
    return {
        references: {
            promptMaxChars: 2000,
            maxImages: 9,
            maxImageBytes: 30 * 1024 * 1024,
            maxVideos: 3,
            maxVideoBytes: 50 * 1024 * 1024,
            maxVideoDurationSeconds: 30,
            maxAudios: 3,
            maxAudioBytes: 15 * 1024 * 1024,
            maxAudioDurationSeconds: 30,
        },
        duration: {
            selection: "range",
            min: WAN3_DURATION_MIN,
            max: WAN3_DURATION_MAX,
            step: 1,
            default: WAN3_DURATION_DEFAULT,
        },
        ratios: [...SEEDANCE_ASPECT_RATIOS],
        defaultRatio: DEFAULT_SEEDANCE_ASPECT_RATIO,
        resolutions: [...WAN3_RESOLUTIONS],
        defaultResolution: DEFAULT_WAN3_RESOLUTION,
        generateAudio: { supported: true, default: true },
        watermark: { supported: true, default: false },
        operations: [...VIDEO_REFERENCE_OPERATIONS],
        defaultOperation: "text_to_video",
    };
}

function seedanceVideoCapability(model: string): VideoCapabilityConfig {
    const caps = videoModelCapabilities(model);
    return {
        references: {
            promptMaxChars: 1000,
            maxImages: SEEDANCE_REFERENCE_LIMITS.images,
            maxImageBytes: SEEDANCE_REFERENCE_LIMITS.imageMaxBytes,
            maxVideos: SEEDANCE_REFERENCE_LIMITS.videos,
            maxVideoBytes: SEEDANCE_REFERENCE_LIMITS.videoMaxBytes,
            maxVideoDurationSeconds: 15,
            maxAudios: SEEDANCE_REFERENCE_LIMITS.audios,
            maxAudioBytes: SEEDANCE_REFERENCE_LIMITS.audioMaxBytes,
            maxAudioDurationSeconds: 15,
        },
        duration: { selection: "range", min: caps.durationMin, max: caps.durationMax, step: 1, default: caps.durationDefault },
        ratios: [...SEEDANCE_ASPECT_RATIOS],
        defaultRatio: DEFAULT_SEEDANCE_ASPECT_RATIO,
        resolutions: caps.resolutions.map(String),
        defaultResolution: caps.resolutions.includes(DEFAULT_VIDEO_RESOLUTION)
            ? DEFAULT_VIDEO_RESOLUTION
            : (caps.resolutions[caps.resolutions.length - 1] || DEFAULT_VIDEO_RESOLUTION),
        generateAudio: { supported: true, default: true },
        watermark: { supported: true, default: false },
        operations: [...VIDEO_REFERENCE_OPERATIONS],
        defaultOperation: "text_to_video",
    };
}

function klingVideoCapability(): VideoCapabilityConfig {
    return {
        references: basicVideoRefs({ maxImages: 2, maxVideos: 0, maxAudios: 0 }),
        duration: { selection: "enum", values: [5, 10], default: 5 },
        ratios: ["16:9", "9:16", "1:1"],
        defaultRatio: "16:9",
        resolutions: ["720p", "1080p"],
        defaultResolution: "720p",
        generateAudio: { supported: false, default: false },
        watermark: { supported: false, default: false },
        operations: [...BASIC_VIDEO_OPERATIONS],
        defaultOperation: "text_to_video",
    };
}

function grokImagineVideoCapability(): VideoCapabilityConfig {
    return {
        references: basicVideoRefs({ maxImages: 1, maxVideos: 0, maxAudios: 0, maxVideoDurationSeconds: 12 }),
        duration: { selection: "range", min: 5, max: 12, step: 1, default: 6 },
        ratios: ["1:1", "16:9", "9:16", "4:3", "3:4", "3:2", "2:3"],
        defaultRatio: "16:9",
        resolutions: ["480p", "720p", "1080p"],
        defaultResolution: "720p",
        generateAudio: { supported: false, default: false },
        watermark: { supported: false, default: false },
        operations: [...BASIC_VIDEO_OPERATIONS],
        defaultOperation: "text_to_video",
    };
}

function soraVideoCapability(): VideoCapabilityConfig {
    return {
        references: basicVideoRefs({ maxImages: 1, maxVideos: 0, maxAudios: 0 }),
        duration: { selection: "enum", values: [4, 8, 12], default: 8 },
        ratios: ["16:9", "9:16"],
        defaultRatio: "16:9",
        resolutions: ["720p"],
        defaultResolution: "720p",
        generateAudio: { supported: false, default: false },
        watermark: { supported: false, default: false },
        operations: [...BASIC_VIDEO_OPERATIONS],
        defaultOperation: "text_to_video",
    };
}

function veoVideoCapability(): VideoCapabilityConfig {
    return {
        references: basicVideoRefs({ maxImages: 2, maxVideos: 0, maxAudios: 0 }),
        duration: { selection: "enum", values: [4, 6, 8], default: 6 },
        ratios: ["16:9", "9:16", "1:1"],
        defaultRatio: "16:9",
        resolutions: ["720p", "1080p"],
        defaultResolution: "720p",
        generateAudio: { supported: false, default: false },
        watermark: { supported: false, default: false },
        operations: [...BASIC_VIDEO_OPERATIONS],
        defaultOperation: "text_to_video",
    };
}

function hailuoVideoCapability(): VideoCapabilityConfig {
    return {
        references: basicVideoRefs({ maxImages: 1, maxVideos: 0, maxAudios: 0 }),
        duration: { selection: "enum", values: [6, 10], default: 6 },
        ratios: ["16:9", "9:16", "1:1"],
        defaultRatio: "16:9",
        resolutions: ["720p", "1080p"],
        defaultResolution: "720p",
        generateAudio: { supported: false, default: false },
        watermark: { supported: false, default: false },
        operations: [...BASIC_VIDEO_OPERATIONS],
        defaultOperation: "text_to_video",
    };
}

function jimengVideoCapability(): VideoCapabilityConfig {
    return {
        references: basicVideoRefs({ maxImages: 1, maxVideos: 0, maxAudios: 0 }),
        duration: { selection: "enum", values: [5, 10], default: 5 },
        ratios: [...SEEDANCE_ASPECT_RATIOS],
        defaultRatio: DEFAULT_SEEDANCE_ASPECT_RATIO,
        resolutions: ["720p", "1080p"],
        defaultResolution: "720p",
        generateAudio: { supported: false, default: false },
        watermark: { supported: false, default: false },
        operations: [...BASIC_VIDEO_OPERATIONS],
        defaultOperation: "text_to_video",
    };
}

function dedicatedVideoCapability(model: string): VideoCapabilityConfig | undefined {
    if (isMiniMaxH3VideoModel(model)) return minimaxH3VideoCapability();
    if (isWan3VideoModel(model)) return wan3VideoCapability();
    if (isSeedanceVideoModel(model) || isSeedanceFastModel(model)) return seedanceVideoCapability(model);
    if (isKlingVideoModel(model)) return klingVideoCapability();
    if (isGrokImagineVideoModel(model)) return grokImagineVideoCapability();
    if (isSoraVideoModel(model)) return soraVideoCapability();
    if (isVeoVideoModel(model)) return veoVideoCapability();
    if (isHailuoVideoModel(model)) return hailuoVideoCapability();
    if (isJimengVideoModel(model)) return jimengVideoCapability();
    return undefined;
}

export function defaultModelCapabilityConfig(protocol?: ModelProtocol): ModelCapabilityConfig {
    const caps = videoModelCapabilities("");
    const video: VideoCapabilityConfig = {
        references: basicVideoRefs(),
        duration: { selection: "range", min: caps.durationMin, max: caps.durationMax, step: 1, default: caps.durationDefault },
        ratios: [...SEEDANCE_ASPECT_RATIOS],
        defaultRatio: DEFAULT_SEEDANCE_ASPECT_RATIO,
        resolutions: caps.resolutions.map(String),
        defaultResolution: "720p",
        generateAudio: { supported: false, default: false },
        watermark: { supported: false, default: false },
        operations: [...VIDEO_REFERENCE_OPERATIONS],
        defaultOperation: "text_to_video",
    };
    if (protocol === "volcengine-jimeng-video") {
        Object.assign(video, jimengVideoCapability());
    }
    if (protocol === "gemini-veo") {
        Object.assign(video, veoVideoCapability());
    }
    if (protocol === "xai-video") {
        Object.assign(video, grokImagineVideoCapability());
    }
    if (protocol === "volcengine-ark-video" || protocol === "newapi-channel-1" || protocol === "newapi-channel-2") {
        video.references.maxVideos = 3;
        video.references.maxAudios = 3;
        video.references.maxVideoBytes = 200 * 1024 * 1024;
        video.references.maxAudioBytes = 15 * 1024 * 1024;
        video.references.maxVideoDurationSeconds = 15;
        video.references.maxAudioDurationSeconds = 15;
        video.generateAudio = { supported: true, default: true };
    }
    if (protocol === "volcengine-ark-video") {
        const seedanceCaps = videoModelCapabilities("seedance");
        video.duration = { selection: "range", min: seedanceCaps.durationMin, max: seedanceCaps.durationMax, step: 1, default: seedanceCaps.durationDefault };
        video.resolutions = seedanceCaps.resolutions.map(String);
        video.watermark = { supported: true, default: false };
    }
    return { version: 1, video };
}

export function modelCapabilityConfigFor(config: CapabilityLookupConfig, model: string) {
    const { channelId, modelName } = splitModelRef(model);
    const dedicated = dedicatedVideoCapability(modelName) || dedicatedVideoCapability(model);
    if (dedicated) return { version: 1, video: dedicated };
    const channel = config.channels.find((item) => item.id === channelId) || config.channels.find((item) => item.models.includes(modelName));
    const cost = channel?.modelCosts?.find((item) => item.model === modelName);
    return mergeVideoCapabilityConfig(cost?.capabilityConfig, cost?.protocol);
}

function mergeVideoCapabilityConfig(stored: ModelCapabilityConfig | undefined, protocol?: ModelProtocol): ModelCapabilityConfig {
    const fallback = defaultModelCapabilityConfig(protocol);
    if (!stored?.video) return fallback;
    const fallbackVideo = fallback.video!;
    const refs = stored.video.references;
    const fallbackRefs = fallbackVideo.references;
    return {
        ...stored,
        video: {
            ...fallbackVideo,
            ...stored.video,
            references: {
                ...fallbackRefs,
                ...refs,
                maxVideos: refs.maxVideos > 0 ? refs.maxVideos : fallbackRefs.maxVideos,
                maxAudios: refs.maxAudios > 0 ? refs.maxAudios : fallbackRefs.maxAudios,
                maxVideoBytes: refs.maxVideoBytes > 0 ? refs.maxVideoBytes : fallbackRefs.maxVideoBytes,
                maxAudioBytes: refs.maxAudioBytes > 0 ? refs.maxAudioBytes : fallbackRefs.maxAudioBytes,
                maxVideoDurationSeconds: refs.maxVideoDurationSeconds > 0 ? refs.maxVideoDurationSeconds : fallbackRefs.maxVideoDurationSeconds,
                maxAudioDurationSeconds: refs.maxAudioDurationSeconds > 0 ? refs.maxAudioDurationSeconds : fallbackRefs.maxAudioDurationSeconds,
            },
            operations: [...new Set([...fallbackVideo.operations, ...(stored.video.operations || [])])],
        },
    };
}

export function normalizeVideoValue(profile: VideoCapabilityConfig, value: { seconds?: string; ratio?: string; resolution?: string }) {
    const duration = profile.duration.selection === "enum"
        ? (profile.duration.values || []).includes(Number(value.seconds)) ? Number(value.seconds) : profile.duration.default
        : normalizeRangeDuration(profile, Number(value.seconds));
    const ratio = resolveVideoRatioValue(profile, value.ratio);
    const resolution = resolveVideoResolutionValue(profile, value.resolution);
    return { seconds: String(duration), ratio, resolution };
}

export function resolveVideoRatioValue(profile: VideoCapabilityConfig, value: string | undefined) {
    return profile.ratios.includes(value || "") ? value! : closestVideoRatio(profile, value) || profile.defaultRatio || profile.ratios[0] || "";
}

function closestVideoRatio(profile: VideoCapabilityConfig, value: string | undefined) {
    const ratio = parseRatioNumber(value);
    if (!ratio || !profile.ratios.length) return "";
    return profile.ratios.reduce((best, item) => {
        const candidate = parseRatioNumber(item);
        const bestValue = parseRatioNumber(best);
        if (!candidate) return best;
        if (!bestValue) return item;
        return Math.abs(candidate - ratio) < Math.abs(bestValue - ratio) ? item : best;
    }, profile.ratios[0] || "");
}

function parseRatioNumber(value: string | undefined) {
    const raw = String(value || "").trim();
    if (!raw) return 0;
    const colon = raw.match(/^(\d+(?:\.\d+)?):(\d+(?:\.\d+)?)/);
    if (colon) return Number(colon[1]) / Math.max(1, Number(colon[2]));
    const pixels = raw.match(/^(\d+)x(\d+)$/i);
    if (pixels) return Number(pixels[1]) / Math.max(1, Number(pixels[2]));
    return 0;
}

export function resolveVideoResolutionValue(profile: VideoCapabilityConfig, value: string | undefined) {
    return matchProfileResolution(profile.resolutions, value)
        || closestProfileResolution(profile.resolutions, value)
        || profile.defaultResolution
        || profile.resolutions[0]
        || "";
}

function closestProfileResolution(allowed: string[], value: string | undefined) {
    const height = parseResolutionHeight(value);
    if (!height) return undefined;
    const ranked = allowed
        .map((item) => ({ item, height: parseResolutionHeight(item) }))
        .filter((item) => item.height > 0);
    if (!ranked.length) return undefined;
    return ranked.reduce((best, item) => (Math.abs(item.height - height) < Math.abs(best.height - height) ? item : best)).item;
}

function parseResolutionHeight(value: string | undefined) {
    const lower = String(value || "").trim().toLowerCase().replace(/[_\s]/g, "");
    if (!lower) return 0;
    if (lower === "2k") return 1440;
    if (lower === "4k" || lower === "2160" || lower === "2160p") return 2160;
    const numeric = Number(lower.replace(/p$/i, ""));
    return Number.isFinite(numeric) && numeric > 0 ? numeric : 0;
}

/** Match stored `vquality` against profile allow-list (case / `p` suffix tolerant). */
function matchProfileResolution(allowed: string[], value: string | undefined): string | undefined {
    const raw = String(value || "").trim();
    if (!raw) return undefined;
    const exact = allowed.find((item) => item === raw);
    if (exact) return exact;
    const needle = raw.toLowerCase().replace(/[_\s]/g, "");
    const needleBare = needle.replace(/p$/i, "");
    return allowed.find((item) => {
        const candidate = item.toLowerCase().replace(/[_\s]/g, "");
        if (candidate === needle) return true;
        return candidate.replace(/p$/i, "") === needleBare;
    });
}

function normalizeRangeDuration(profile: VideoCapabilityConfig, value: number) {
    const min = profile.duration.min || 1;
    const max = profile.duration.max || min;
    const step = profile.duration.step || 1;
    const candidate = Number.isFinite(value) ? Math.floor(value) : profile.duration.default;
    const clamped = Math.min(max, Math.max(min, candidate));
    const maxStep = Math.max(0, Math.floor((max - min) / step));
    return min + Math.min(maxStep, Math.max(0, Math.round((clamped - min) / step))) * step;
}

export function videoDurationOptions(profile: VideoCapabilityConfig) {
    if (profile.duration.selection === "enum") return profile.duration.values || [];
    const min = profile.duration.min || 1;
    const max = profile.duration.max || min;
    const step = profile.duration.step || 1;
    return Array.from({ length: Math.floor((max - min) / step) + 1 }, (_, index) => min + index * step);
}

export function videoDurationAllowed(profile: VideoCapabilityConfig, value: number) {
    if (profile.duration.selection === "enum") return (profile.duration.values || []).includes(value);
    const min = profile.duration.min || 1;
    const max = profile.duration.max || min;
    const step = profile.duration.step || 1;
    return value >= min && value <= max && (value - min) % step === 0;
}

const DEFAULT_IMAGE_QUALITIES = ["auto", "high", "medium", "low"];

function aspect(value: string, label: string, width: number, height: number, icon: ImageAspectIcon, size?: string): ImageAspectOption {
    return size ? { value, label, width, height, icon, size } : { value, label, width, height, icon };
}

function defaultImageCapability(protocol?: ModelProtocol): ImageCapabilityConfig {
    return {
        aspects: [
            aspect("1:1", "1:1", 1024, 1024, "square"),
            aspect("3:2", "3:2", 1536, 1024, "landscape"),
            aspect("2:3", "2:3", 1024, 1536, "portrait"),
            aspect("4:3", "4:3", 1360, 1024, "landscape"),
            aspect("3:4", "3:4", 1024, 1360, "portrait"),
            aspect("16:9", "16:9", 1824, 1024, "landscape"),
            aspect("9:16", "9:16", 1024, 1824, "portrait"),
            aspect("auto", "auto", 0, 0, "auto"),
        ],
        defaultSize: "1:1",
        qualities: [...DEFAULT_IMAGE_QUALITIES],
        defaultQuality: "auto",
        transparentBackground: protocol === "openai-image",
        customPixels: true,
        maxCount: 15,
    };
}

function seedreamImageCapability(): ImageCapabilityConfig {
    return {
        aspects: [
            aspect("1:1", "1:1(2K)", 2048, 2048, "square"),
            aspect("16:9", "16:9(2K)", 2816, 1584, "landscape"),
            aspect("9:16", "9:16(2K)", 1584, 2816, "portrait"),
            aspect("4:3", "4:3(2K)", 2368, 1776, "landscape"),
            aspect("3:4", "3:4(2K)", 1776, 2368, "portrait"),
            aspect("21:9", "21:9(2K)", 3136, 1344, "landscape"),
        ],
        defaultSize: "16:9",
        qualities: [],
        defaultQuality: "auto",
        transparentBackground: false,
        customPixels: false,
        maxCount: 1,
    };
}

function gptImageCapability(): ImageCapabilityConfig {
    return {
        aspects: [
            aspect("1:1", "1:1", 1024, 1024, "square", "1024x1024"),
            aspect("3:2", "3:2", 1536, 1024, "landscape", "1536x1024"),
            aspect("2:3", "2:3", 1024, 1536, "portrait", "1024x1536"),
            aspect("auto", "auto", 0, 0, "auto"),
        ],
        defaultSize: "1024x1024",
        qualities: [...DEFAULT_IMAGE_QUALITIES],
        defaultQuality: "auto",
        transparentBackground: true,
        customPixels: false,
        maxCount: 1,
    };
}

function grokImagineImageCapability(): ImageCapabilityConfig {
    return {
        aspects: [
            aspect("1:1", "1:1", 1024, 1024, "square"),
            aspect("16:9", "16:9", 1824, 1024, "landscape"),
            aspect("9:16", "9:16", 1024, 1824, "portrait"),
            aspect("4:3", "4:3", 1360, 1024, "landscape"),
            aspect("3:4", "3:4", 1024, 1360, "portrait"),
            aspect("3:2", "3:2", 1536, 1024, "landscape"),
            aspect("2:3", "2:3", 1024, 1536, "portrait"),
            aspect("2:1", "2:1", 2048, 1024, "landscape"),
            aspect("1:2", "1:2", 1024, 2048, "portrait"),
        ],
        defaultSize: "16:9",
        qualities: [],
        defaultQuality: "auto",
        transparentBackground: false,
        customPixels: false,
        maxCount: 1,
    };
}

export function imageCapabilityConfigFor(config: CapabilityLookupConfig, model: string): ImageCapabilityConfig {
    const { channelId, modelName } = splitModelRef(model);
    if (isSeedreamImageModel(modelName) || isSeedreamImageModel(model)) return seedreamImageCapability();
    if (isGrokImagineImageModel(modelName) || isGrokImagineImageModel(model)) return grokImagineImageCapability();
    if (isGptImageModel(modelName) || isGptImageModel(model)) return gptImageCapability();
    const channel = config.channels.find((item) => item.id === channelId) || config.channels.find((item) => item.models.includes(modelName));
    const protocol = channel?.modelCosts?.find((item) => item.model === modelName)?.protocol;
    if (protocol === "openai-image") return gptImageCapability();
    if (protocol === "volcengine-ark-image") return seedreamImageCapability();
    return defaultImageCapability(protocol);
}

export function persistImageAspectValue(option: ImageAspectOption) {
    return option.size || option.value;
}

export function resolveImageSizeValue(profile: ImageCapabilityConfig, value: string | undefined) {
    const raw = String(value || "").trim();
    if (!raw) return profile.defaultSize;
    const exact = profile.aspects.find((item) => item.value === raw || item.size === raw || persistImageAspectValue(item) === raw);
    if (exact) return persistImageAspectValue(exact);
    const ratio = raw.split("-")[0];
    const byRatio = profile.aspects.find((item) => item.value === ratio);
    if (byRatio) return persistImageAspectValue(byRatio);
    const closest = closestImageAspect(profile, raw);
    return closest ? persistImageAspectValue(closest) : profile.defaultSize;
}

function closestImageAspect(profile: ImageCapabilityConfig, value: string) {
    const ratio = parseRatioNumber(value) || parseRatioNumber(imageSizeToAspectRatio(value));
    const candidates = profile.aspects.filter((item) => item.icon !== "auto");
    if (!ratio || !candidates.length) return undefined;
    return candidates.reduce((best, item) => {
        const candidate = item.width && item.height ? item.width / item.height : parseRatioNumber(item.value);
        const bestValue = best.width && best.height ? best.width / best.height : parseRatioNumber(best.value);
        if (!candidate) return best;
        if (!bestValue) return item;
        return Math.abs(candidate - ratio) < Math.abs(bestValue - ratio) ? item : best;
    });
}

export function normalizeImageValue(profile: ImageCapabilityConfig, value: { size?: string; quality?: string }) {
    const size = resolveImageSizeValue(profile, value.size);
    const quality = profile.qualities.length
        ? (profile.qualities.includes(value.quality || "") ? value.quality! : profile.defaultQuality)
        : profile.defaultQuality;
    return { size, quality };
}

/** Map stored canvas size (`1024x1024`, `16:9-4k`, `16:9`) to a Flowy `aspect_ratio`. Empty means omit / model default. */
export function imageSizeToAspectRatio(size: string | undefined) {
    const raw = String(size || "").trim();
    if (!raw || raw === "auto") return "";
    if (/^\d+:\d+/.test(raw)) return raw.split("-")[0];
    const pixels = raw.match(/^(\d+)x(\d+)$/i);
    if (!pixels) return "";
    const width = Number(pixels[1]);
    const height = Number(pixels[2]);
    if (!width || !height) return "";
    const ratio = width / height;
    const options: Array<[string, number]> = [
        ["1:1", 1],
        ["3:2", 3 / 2],
        ["2:3", 2 / 3],
        ["4:3", 4 / 3],
        ["3:4", 3 / 4],
        ["16:9", 16 / 9],
        ["9:16", 9 / 16],
        ["21:9", 21 / 9],
        ["2:1", 2],
        ["1:2", 1 / 2],
    ];
    return options.reduce((best, item) => (Math.abs(item[1] - ratio) < Math.abs(best[1] - ratio) ? item : best), options[0])[0];
}

export type ModelVideoBooleanOptions = {
    videoGenerateAudio: string;
    videoWatermark: string;
};

export function resolveModelVideoBooleanOptions(
    config: CapabilityLookupConfig,
    model: string,
    explicit: Partial<ModelVideoBooleanOptions> = {},
    fallback: Partial<ModelVideoBooleanOptions> = {},
): ModelVideoBooleanOptions {
    const profile = modelCapabilityConfigFor(config, model).video!;
    const pick = (key: keyof ModelVideoBooleanOptions) => explicit[key] || fallback[key];
    return {
        videoGenerateAudio: profile.generateAudio.supported ? pick("videoGenerateAudio") ?? String(profile.generateAudio.default) : "false",
        videoWatermark: profile.watermark.supported ? pick("videoWatermark") ?? String(profile.watermark.default) : "false",
    };
}

export function canvasMediaSpecsForModel(
    config: CapabilityLookupConfig,
    model: string,
    mode: "image" | "video" | "text" | "audio",
    current: CanvasMediaSpecValues = {},
): CanvasMediaSpecPatch {
    if (mode === "image") {
        const image = imageCapabilityConfigFor(config, model);
        const normalized = normalizeImageValue(image, { size: current.size, quality: current.quality });
        return {
            size: normalized.size,
            quality: normalized.quality,
            seconds: current.seconds || "",
            vquality: current.vquality || "",
            generateAudio: "false",
            watermark: "false",
            transparentBackground: image.transparentBackground ? (current.transparentBackground === "true" ? "true" : "false") : "false",
        };
    }
    if (mode === "video") {
        const video = modelCapabilityConfigFor(config, model).video!;
        const normalized = normalizeVideoValue(video, { seconds: current.seconds, ratio: current.size, resolution: current.vquality });
        const booleans = resolveModelVideoBooleanOptions(
            config,
            model,
            { videoGenerateAudio: current.generateAudio, videoWatermark: current.watermark },
        );
        return {
            size: normalized.ratio,
            quality: current.quality || "auto",
            seconds: normalized.seconds,
            vquality: storeVqualityForUi(model, normalized.resolution),
            generateAudio: booleans.videoGenerateAudio,
            watermark: booleans.videoWatermark,
            transparentBackground: "false",
        };
    }
    return {
        size: current.size || "",
        quality: current.quality || "auto",
        seconds: current.seconds || "",
        vquality: current.vquality || "",
        generateAudio: "false",
        watermark: "false",
        transparentBackground: "false",
    };
}

export function canvasModelSpecPatch(
    config: CapabilityLookupConfig,
    model: string,
    mode: "image" | "video" | "text" | "audio",
    current: CanvasMediaSpecValues = {},
): Partial<CanvasMediaSpecPatch> {
    const specs = canvasMediaSpecsForModel(config, model, mode, current);
    if (mode === "image") {
        return {
            size: specs.size,
            quality: specs.quality,
            transparentBackground: specs.transparentBackground,
        };
    }
    if (mode === "video") {
        return {
            size: specs.size,
            seconds: specs.seconds,
            vquality: specs.vquality,
            generateAudio: specs.generateAudio,
            watermark: specs.watermark,
        };
    }
    return {};
}

export function modelPromptLengthError(config: CapabilityLookupConfig, model: string, capability: "text" | "image" | "video", prompt: string) {
    if (capability !== "video") return "";
    const maxChars = modelCapabilityConfigFor(config, model).video?.references.promptMaxChars;
    if (!maxChars || maxChars <= 0) return "";
    const actualChars = Array.from(prompt).length;
    if (actualChars <= maxChars) return "";
    return canvasT(
        "videoCanvas.generation.promptTooLong",
        "当前视频模型提示词最多 {{max}} 个字符，完整提示词为 {{actual}} 个字符。系统不会自动截断，请精简当前输入、连线内容或技能上下文后重试",
        { max: maxChars, actual: actualChars },
    );
}
