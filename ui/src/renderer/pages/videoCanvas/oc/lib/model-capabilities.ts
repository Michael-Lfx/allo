import type { ModelProtocol } from "@oc/lib/model-protocols";
import { MINIMAX_H3_DURATION_DEFAULT, MINIMAX_H3_DURATION_MAX, MINIMAX_H3_DURATION_MIN } from "@oc/lib/minimax-h3-video";
import {
    DEFAULT_MINIMAX_H3_RESOLUTION,
    DEFAULT_VIDEO_RESOLUTION,
    isMiniMaxH3VideoModel,
    MINIMAX_H3_RESOLUTIONS,
    videoModelCapabilities,
} from "@renderer/services/videoModelCapabilities";
import { DEFAULT_SEEDANCE_ASPECT_RATIO, SEEDANCE_ASPECT_RATIOS } from "@renderer/pages/videoGeneration/aspectRatios";
import { isSeedanceFastModel, isSeedanceVideoModel } from "@oc/lib/seedance-video";

export type ModelCapabilityConfig = {
    version: number;
    video?: VideoCapabilityConfig;
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
        // Prefer MiniMax `aigc_watermark` via backend; do not expose Ark watermark / generate_audio.
        generateAudio: { supported: false, default: false },
        watermark: { supported: false, default: false },
        operations: ["text_to_video", "image_to_video"],
        defaultOperation: "text_to_video",
    };
}

export function defaultModelCapabilityConfig(protocol?: ModelProtocol): ModelCapabilityConfig {
    const video: VideoCapabilityConfig = {
        references: {
            promptMaxChars: 1000,
            maxImages: 9,
            maxImageBytes: 30 * 1024 * 1024,
            maxVideos: 0,
            maxVideoBytes: 0,
            maxVideoDurationSeconds: 0,
            maxAudios: 0,
            maxAudioBytes: 0,
            maxAudioDurationSeconds: 0,
        },
        duration: { selection: "range", min: 1, max: 15, step: 1, default: 6 },
        ratios: [...SEEDANCE_ASPECT_RATIOS],
        defaultRatio: DEFAULT_SEEDANCE_ASPECT_RATIO,
        resolutions: ["480p", "720p", "1080p", "2160p"],
        defaultResolution: "720p",
        generateAudio: { supported: false, default: false },
        watermark: { supported: false, default: false },
        operations: ["text_to_video", "image_to_video"],
        defaultOperation: "text_to_video",
    };
    if (protocol === "volcengine-jimeng-video") video.duration = { selection: "enum", values: [5, 10], default: 5 };
    if (protocol === "gemini-veo") {
        video.duration = { selection: "enum", values: [4, 6, 8], default: 6 };
        video.resolutions = ["720p", "1080p"];
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
    if (protocol === "volcengine-ark-video") video.watermark = { supported: true, default: false };
    return { version: 1, video };
}

export function modelCapabilityConfigFor(config: { channels: Array<{ id: string; models: string[]; modelCosts?: Array<{ model: string; capabilityConfig?: ModelCapabilityConfig; protocol?: ModelProtocol }> }> }, model: string) {
    const separator = model.indexOf("::");
    const channelId = separator >= 0 ? model.slice(0, separator) : "";
    const modelName = separator >= 0 ? model.slice(separator + 2) : model;
    if (isMiniMaxH3VideoModel(modelName) || isMiniMaxH3VideoModel(model)) {
        return { version: 1, video: minimaxH3VideoCapability() };
    }
    const channel = config.channels.find((item) => item.id === channelId) || config.channels.find((item) => item.models.includes(modelName));
    const cost = channel?.modelCosts?.find((item) => item.model === modelName);
    const base = cost?.capabilityConfig || defaultModelCapabilityConfig(cost?.protocol);
    // Overlay per-model resolution allow-list (Seedance fast/mini drop 1080p, etc.).
    if (base.video && (isSeedanceVideoModel(modelName) || isSeedanceFastModel(modelName))) {
        const caps = videoModelCapabilities(modelName);
        return {
            ...base,
            video: {
                ...base.video,
                resolutions: caps.resolutions.map(String),
                defaultResolution: caps.resolutions.includes(DEFAULT_VIDEO_RESOLUTION)
                    ? DEFAULT_VIDEO_RESOLUTION
                    : (caps.resolutions[caps.resolutions.length - 1] || DEFAULT_VIDEO_RESOLUTION),
            },
        };
    }
    return base;
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
    return profile.ratios.includes(value || "") ? value! : profile.defaultRatio || profile.ratios[0] || "";
}

export function resolveVideoResolutionValue(profile: VideoCapabilityConfig, value: string | undefined) {
    return matchProfileResolution(profile.resolutions, value) || profile.defaultResolution || profile.resolutions[0] || "";
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
