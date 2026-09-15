export const CAMERA_BODY_IDS = ["dxl2", "alexa35", "venice2", "vraptor", "hasselblad", "imax"] as const;
export const CAMERA_LENS_IDS = ["signature-prime", "cooke-s4", "master-prime", "primo", "summilux", "k35", "anamorphic"] as const;
export const CAMERA_FOCAL_VALUES = ["18", "21", "24", "27", "35", "40", "50", "65", "75", "85", "100", "135"] as const;
export const CAMERA_APERTURE_VALUES = ["1.4", "1.8", "2", "2.8", "4", "5.6", "8", "11"] as const;
export const CAMERA_ANGLE_VALUES = ["auto", "eye", "low", "high", "bird"] as const;
export const CAMERA_SHOT_VALUES = ["auto", "close", "medium", "wide"] as const;

export type CameraBodyId = (typeof CAMERA_BODY_IDS)[number];
export type CameraLensId = (typeof CAMERA_LENS_IDS)[number];
export type CameraFocalValue = (typeof CAMERA_FOCAL_VALUES)[number];
export type CameraApertureValue = (typeof CAMERA_APERTURE_VALUES)[number];
export type CameraAngleValue = (typeof CAMERA_ANGLE_VALUES)[number];
export type CameraShotValue = (typeof CAMERA_SHOT_VALUES)[number];

export type CameraBodyOption = {
    id: CameraBodyId;
    name: string;
    prompt: string;
    tone: "panavision" | "arri" | "sony" | "red" | "hasselblad" | "imax";
};

export type CameraLensOption = {
    id: CameraLensId;
    name: string;
    short: string;
    prompt: string;
    tone: "arri" | "cooke" | "zeiss" | "panavision" | "leica" | "canon" | "anamorphic";
};

export const CAMERA_BODIES: CameraBodyOption[] = [
    { id: "dxl2", name: "Panavision DXL2", prompt: "Panavision DXL2 电影机", tone: "panavision" },
    { id: "alexa35", name: "ARRI Alexa 35", prompt: "ARRI Alexa 35 电影机", tone: "arri" },
    { id: "venice2", name: "Sony Venice 2", prompt: "Sony Venice 2 全画幅电影机", tone: "sony" },
    { id: "vraptor", name: "RED V-Raptor", prompt: "RED V-Raptor 电影机", tone: "red" },
    { id: "hasselblad", name: "Hasselblad X2D", prompt: "Hasselblad X2D 中画幅", tone: "hasselblad" },
    { id: "imax", name: "IMAX 65mm", prompt: "IMAX 65mm 胶片机", tone: "imax" },
];

export const CAMERA_LENSES: CameraLensOption[] = [
    { id: "signature-prime", name: "Arri Signature Prime", short: "Signature", prompt: "Arri Signature Prime", tone: "arri" },
    { id: "cooke-s4", name: "Cooke S4/i", short: "Cooke", prompt: "Cooke S4/i", tone: "cooke" },
    { id: "master-prime", name: "Zeiss Master Prime", short: "Master", prompt: "Zeiss Master Prime", tone: "zeiss" },
    { id: "primo", name: "Panavision Primo", short: "Primo", prompt: "Panavision Primo", tone: "panavision" },
    { id: "summilux", name: "Leica Summilux", short: "Summilux", prompt: "Leica Summilux", tone: "leica" },
    { id: "k35", name: "Canon K35", short: "K35", prompt: "Canon K35", tone: "canon" },
    { id: "anamorphic", name: "Panavision C-Series", short: "C-Series", prompt: "Panavision C-Series 变形宽银幕镜头", tone: "anamorphic" },
];

export type CanvasCameraRig = {
    enabled: boolean;
    body: CameraBodyId;
    lens: CameraLensId;
    focal: CameraFocalValue;
    aperture: CameraApertureValue;
    angle: CameraAngleValue;
    shot: CameraShotValue;
};

export type CanvasCameraRigPatch = {
    cameraEnabled?: string;
    cameraBody?: string;
    cameraLens?: string;
    cameraFocal?: string;
    cameraAperture?: string;
    cameraAngle?: string;
    cameraShot?: string;
};

export const DEFAULT_CAMERA_RIG: CanvasCameraRig = {
    enabled: false,
    body: "dxl2",
    lens: "signature-prime",
    focal: "35",
    aperture: "4",
    angle: "auto",
    shot: "auto",
};

const CAMERA_ALREADY = /【摄像机】/;
const LEGACY_FOCAL = new Set(["24", "35", "50", "85", "135"]);
const LEGACY_BODY: Record<string, CameraBodyId> = {
    cine: "dxl2",
    "full-frame": "venice2",
    "medium-format": "hasselblad",
};

export function parseCameraEnum<T extends readonly string[]>(value: string | undefined, allowed: T, fallback: T[number]): T[number] {
    return allowed.includes(value as T[number]) ? (value as T[number]) : fallback;
}

export function readCameraRig(source?: CanvasCameraRigPatch | null): CanvasCameraRig {
    const legacyFocal = LEGACY_FOCAL.has(source?.cameraLens || "") ? (source!.cameraLens as CameraFocalValue) : undefined;
    const lensIsSeries = CAMERA_LENS_IDS.includes(source?.cameraLens as CameraLensId);
    const migratedBody = LEGACY_BODY[source?.cameraBody || ""];
    const hasLegacySelection = Boolean(legacyFocal || migratedBody);
    return {
        enabled: source?.cameraEnabled === "true" || (source?.cameraEnabled == null && hasLegacySelection),
        body: parseCameraEnum(migratedBody || source?.cameraBody, CAMERA_BODY_IDS, DEFAULT_CAMERA_RIG.body),
        lens: parseCameraEnum(lensIsSeries ? source?.cameraLens : undefined, CAMERA_LENS_IDS, DEFAULT_CAMERA_RIG.lens),
        focal: parseCameraEnum(source?.cameraFocal || legacyFocal, CAMERA_FOCAL_VALUES, DEFAULT_CAMERA_RIG.focal),
        aperture: parseCameraEnum(source?.cameraAperture, CAMERA_APERTURE_VALUES, DEFAULT_CAMERA_RIG.aperture),
        angle: parseCameraEnum(source?.cameraAngle, CAMERA_ANGLE_VALUES, "auto"),
        shot: parseCameraEnum(source?.cameraShot, CAMERA_SHOT_VALUES, "auto"),
    };
}

export function cameraRigIsActive(rig: CanvasCameraRig) {
    return rig.enabled;
}

export function cameraBodyOption(id: CameraBodyId) {
    return CAMERA_BODIES.find((item) => item.id === id) || CAMERA_BODIES[0];
}

export function cameraLensOption(id: CameraLensId) {
    return CAMERA_LENSES.find((item) => item.id === id) || CAMERA_LENSES[0];
}

export function cameraRigFingerprint(rig: CanvasCameraRig): Record<string, string> | undefined {
    if (!rig.enabled) return undefined;
    const next: Record<string, string> = {
        cameraEnabled: "true",
        cameraBody: rig.body,
        cameraLens: rig.lens,
        cameraFocal: rig.focal,
        cameraAperture: rig.aperture,
    };
    if (rig.angle !== "auto") next.cameraAngle = rig.angle;
    if (rig.shot !== "auto") next.cameraShot = rig.shot;
    return next;
}

export function compactCameraRigToken(rig: CanvasCameraRig) {
    if (!rig.enabled) return "";
    return `${rig.focal}mm · f/${rig.aperture}`;
}

export function compileCameraRigPrompt(prompt: string, rig: CanvasCameraRig) {
    const trimmed = prompt.trim();
    if (!rig.enabled || CAMERA_ALREADY.test(trimmed)) return trimmed;
    const body = cameraBodyOption(rig.body);
    const lens = cameraLensOption(rig.lens);
    const bits = [`${body.prompt}，${lens.prompt} ${rig.focal}mm，f/${rig.aperture}`];
    if (rig.angle !== "auto") bits.push(ANGLE_PROMPT[rig.angle]);
    if (rig.shot !== "auto") bits.push(SHOT_PROMPT[rig.shot]);
    return `${trimmed}\n【摄像机】${bits.join("，")}。焦段、光圈与景深保持真实镜头光学自洽。`;
}

export function stepCameraIndex(index: number, length: number, delta: number) {
    return (index + delta + length) % length;
}

const ANGLE_PROMPT: Record<Exclude<CameraAngleValue, "auto">, string> = {
    eye: "平视机位",
    low: "低机位仰拍",
    high: "高机位俯拍",
    bird: "鸟瞰俯视",
};

const SHOT_PROMPT: Record<Exclude<CameraShotValue, "auto">, string> = {
    close: "近景特写",
    medium: "中景",
    wide: "全景远景",
};
