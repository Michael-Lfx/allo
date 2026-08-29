import { nanoid } from "nanoid";

import type { DirectorBoneKeyframe, DirectorBoneTrack, DirectorCamera, DirectorHumanoidBone, DirectorKeyframe, DirectorKeyframeDeleteTarget, DirectorKeyframeEasing, DirectorLight, DirectorObject, DirectorQuat, DirectorScene, DirectorTransform, DirectorVec3 } from "@oc/types/director";

export const DIRECTOR_DEFAULT_ACTOR_URL = "https://cdn.jsdelivr.net/gh/mrdoob/three.js@r185/examples/models/gltf/Xbot.glb";
export const DIRECTOR_ACTOR_COLORS = ["#f1f3f5", "#202329", "#2f7de1", "#d84949", "#dfae3f", "#34a276"] as const;

export const directorIdentityTransform = (position: DirectorVec3 = [0, 0, 0]): DirectorTransform => ({ position, rotation: [0, 0, 0], scale: [1, 1, 1] });

export function createDirectorScene(title = "未命名场景"): DirectorScene {
    const now = new Date().toISOString();
    const camera = createDirectorCamera();
    const shotId = nanoid();
    return {
        id: nanoid(),
        version: 1,
        title,
        background: "#d8dde3",
        environmentIntensity: 0.7,
        gridVisible: true,
        objects: [createDirectorActor("演员 1", [0, 0, 0])],
        cameras: [camera],
        lights: [createDirectorLight("directional", "主光", [4, 6, 4], 2.4), createDirectorLight("directional", "轮廓光", [-4, 3, -2], 1.1), createDirectorLight("ambient", "环境光", [0, 0, 0], 0.65)],
        shots: [{ id: shotId, name: "镜头 1", cameraId: camera.id, duration: 5, fps: 24, shotSize: "medium", cameraMove: "static", prompt: "" }],
        activeShotId: shotId,
        createdAt: now,
        updatedAt: now,
    };
}

export function createDirectorObject(primitive: DirectorObject["primitive"] = "box", name = "新对象", position: DirectorVec3 = [0, 0.5, 0], color = "#8795a5"): DirectorObject {
    return {
        id: nanoid(),
        name,
        kind: "primitive",
        primitive,
        transform: directorIdentityTransform(position),
        color,
        visible: true,
        castShadow: true,
        receiveShadow: true,
        pose: primitive === "character" ? "stand" : undefined,
        keyframes: [],
    };
}

export function createDirectorActor(name = "演员", position: DirectorVec3 = [0, 0, 0], color: string = DIRECTOR_ACTOR_COLORS[0]): DirectorObject {
    return {
        ...createDirectorObject("box", name, position, color),
        kind: "actor",
        primitive: undefined,
        url: DIRECTOR_DEFAULT_ACTOR_URL,
        mimeType: "model/gltf-binary",
        pose: "stand",
        rig: { status: "unmapped", boneMap: {}, animationNames: [] },
        motionClips: [],
        boneOverrides: {},
        boneTracks: [],
    };
}

export function createDirectorModel(input: Pick<DirectorObject, "name" | "storageKey" | "url" | "mimeType" | "assetId">): DirectorObject {
    return { ...createDirectorObject("box", input.name, [0, 0, 0]), ...input, kind: "model", primitive: undefined };
}

export function createDirectorBillboard(name: string, url: string, storageKey?: string, sourceNodeId?: string): DirectorObject {
    return { ...createDirectorObject("plane", name, [0, 1.1, 0], "#ffffff"), kind: "billboard", url, storageKey, sourceNodeId, transform: { position: [0, 1.1, 0], rotation: [0, 0, 0], scale: [1.6, 0.9, 1] } };
}

export function createDirectorCamera(name = "主摄影机"): DirectorCamera {
    return { id: nanoid(), name, transform: directorIdentityTransform([4.8, 2.7, 6.8]), target: [0, 1, 0], focalLength: 35, fov: 50, aperture: 2.8, focusDistance: 5, near: 0.05, far: 500, keyframes: [] };
}

/** 35mm 全画幅水平视角换算。摄影机检查器与场景模板共用，避免两处各写一份光学。 */
export function directorFocalLengthToFov(focalLength: number) {
    return (2 * Math.atan(36 / (2 * Math.max(1, focalLength))) * 180) / Math.PI;
}

export function createDirectorLight(type: DirectorLight["type"], name: string, position: DirectorVec3, intensity = 1): DirectorLight {
    return { id: nanoid(), name, type, transform: directorIdentityTransform(position), color: "#ffffff", intensity, angle: Math.PI / 4, penumbra: 0.35, castShadow: type !== "ambient" };
}

export function touchDirectorScene(scene: DirectorScene): DirectorScene {
    return { ...scene, updatedAt: new Date().toISOString() };
}

/** 关键帧命中容差：upsert 与 remove 必须同判据，否则会出现「记录能覆盖但删不掉」。 */
export const DIRECTOR_KEYFRAME_EPSILON = 0.001;

export function upsertDirectorKeyframe(keyframes: DirectorKeyframe[], time: number, transform: DirectorTransform) {
    const current = keyframes.find((item) => Math.abs(item.time - time) < DIRECTOR_KEYFRAME_EPSILON);
    const next = current ? keyframes.map((item) => (item.id === current.id ? { ...item, transform } : item)) : [...keyframes, { id: nanoid(), time, transform }];
    return next.toSorted((a, b) => a.time - b.time);
}

export function upsertDirectorBoneKeyframe(tracks: DirectorBoneTrack[], bone: DirectorHumanoidBone, time: number, rotation: DirectorQuat) {
    const track = tracks.find((item) => item.bone === bone);
    const nextKeyframes = upsertBoneKeyframe(track?.keyframes || [], time, rotation);
    return track ? tracks.map((item) => item.bone === bone ? { ...item, keyframes: nextKeyframes } : item) : [...tracks, { bone, keyframes: nextKeyframes }];
}

function upsertBoneKeyframe(keyframes: DirectorBoneKeyframe[], time: number, rotation: DirectorQuat) {
    const current = keyframes.find((item) => Math.abs(item.time - time) < DIRECTOR_KEYFRAME_EPSILON);
    const next = current ? keyframes.map((item) => item.id === current.id ? { ...item, rotation } : item) : [...keyframes, { id: nanoid(), time, rotation }];
    return next.toSorted((a, b) => a.time - b.time);
}

/** 按 id 删除对象 transform 关键帧；id 不存在时返回原数组引用。 */
export function removeDirectorKeyframe(keyframes: DirectorKeyframe[], keyframeId: string): DirectorKeyframe[] {
    if (!keyframes.some((item) => item.id === keyframeId)) return keyframes;
    return keyframes.filter((item) => item.id !== keyframeId);
}

/**
 * 按 id 删除某骨骼轨道上的关键帧。
 * 轨道被删空后整条移除，否则时间轴会留下永远为空的子轨道。
 */
export function removeDirectorBoneKeyframe(tracks: DirectorBoneTrack[], bone: DirectorHumanoidBone, keyframeId: string): DirectorBoneTrack[] {
    const track = tracks.find((item) => item.bone === bone);
    if (!track?.keyframes.some((item) => item.id === keyframeId)) return tracks;

    const nextKeyframes = track.keyframes.filter((item) => item.id !== keyframeId);
    if (!nextKeyframes.length) return tracks.filter((item) => item.bone !== bone);
    return tracks.map((item) => (item.bone === bone ? { ...item, keyframes: nextKeyframes } : item));
}

/**
 * 时间轴删除关键帧的唯一入口：按轨道类型分派到对应领域函数。
 *
 * 未命中（对象/摄影机/骨骼/关键帧任一不存在）时返回同一个 scene 引用，
 * 调用方据此跳过历史与保存，避免「点了没删掉却多一次修订」。
 */
export function removeDirectorSceneKeyframe(scene: DirectorScene, target: DirectorKeyframeDeleteTarget): DirectorScene {
    if (target.track === "camera") {
        const camera = scene.cameras.find((item) => item.id === target.cameraId);
        if (!camera) return scene;
        const keyframes = removeDirectorKeyframe(camera.keyframes, target.keyframeId);
        if (keyframes === camera.keyframes) return scene;
        return { ...scene, cameras: scene.cameras.map((item) => (item.id === camera.id ? { ...item, keyframes } : item)) };
    }

    const object = scene.objects.find((item) => item.id === target.objectId);
    if (!object) return scene;

    if (target.track === "object-transform") {
        const keyframes = removeDirectorKeyframe(object.keyframes, target.keyframeId);
        if (keyframes === object.keyframes) return scene;
        return { ...scene, objects: scene.objects.map((item) => (item.id === object.id ? { ...item, keyframes } : item)) };
    }

    const tracks = object.boneTracks || [];
    const boneTracks = removeDirectorBoneKeyframe(tracks, target.bone, target.keyframeId);
    if (boneTracks === tracks) return scene;
    return { ...scene, objects: scene.objects.map((item) => (item.id === object.id ? { ...item, boneTracks } : item)) };
}

/**
 * 更新一枚关键帧后续区间的缓动。未命中时返回原 scene 引用，调用方据此跳过历史与保存。
 */
export function setDirectorSceneKeyframeEasing(scene: DirectorScene, target: DirectorKeyframeDeleteTarget, easing: DirectorKeyframeEasing): DirectorScene {
    const update = <T extends { id: string; easing?: DirectorKeyframeEasing }>(keyframes: T[]) => {
        if (!keyframes.some((item) => item.id === target.keyframeId)) return keyframes;
        return keyframes.map((item) => item.id === target.keyframeId ? { ...item, easing } : item);
    };

    if (target.track === "camera") {
        const camera = scene.cameras.find((item) => item.id === target.cameraId);
        if (!camera) return scene;
        const keyframes = update(camera.keyframes);
        if (keyframes === camera.keyframes) return scene;
        return { ...scene, cameras: scene.cameras.map((item) => item.id === camera.id ? { ...item, keyframes } : item) };
    }

    const object = scene.objects.find((item) => item.id === target.objectId);
    if (!object) return scene;
    if (target.track === "object-transform") {
        const keyframes = update(object.keyframes);
        if (keyframes === object.keyframes) return scene;
        return { ...scene, objects: scene.objects.map((item) => item.id === object.id ? { ...item, keyframes } : item) };
    }

    const tracks = object.boneTracks || [];
    const track = tracks.find((item) => item.bone === target.bone);
    if (!track) return scene;
    const keyframes = update(track.keyframes);
    if (keyframes === track.keyframes) return scene;
    const boneTracks = tracks.map((item) => item.bone === target.bone ? { ...item, keyframes } : item);
    return { ...scene, objects: scene.objects.map((item) => item.id === object.id ? { ...item, boneTracks } : item) };
}

/** 缓动属于前一枚关键帧到下一枚关键帧的区间。旧数据未声明时保持线性。 */
export function resolveDirectorKeyframeProgress(progress: number, easing: DirectorKeyframeEasing = "linear") {
    const clamped = Math.max(0, Math.min(1, progress));
    if (easing === "step") return 0;
    if (easing === "smooth") return clamped * clamped * (3 - 2 * clamped);
    return clamped;
}

/** 轨迹渲染只接受有限时间与位置；坏数据不得进入 Three 几何体。 */
export function finiteDirectorTransformKeyframes(keyframes: DirectorKeyframe[]) {
    return keyframes.filter((keyframe) => [keyframe.time, ...keyframe.transform.position].every(Number.isFinite));
}

/** 按时间顺序累计 Transform 关键帧路径长度；非法时间或坐标段忽略，不污染界面统计。 */
export function directorTransformPathLength(keyframes: DirectorKeyframe[]) {
    const sorted = keyframes.toSorted((left, right) => left.time - right.time);
    let length = 0;
    for (let index = 1; index < sorted.length; index += 1) {
        const previousTime = sorted[index - 1].time;
        const currentTime = sorted[index].time;
        const previous = sorted[index - 1].transform.position;
        const current = sorted[index].transform.position;
        if (![previousTime, currentTime, ...previous, ...current].every(Number.isFinite)) continue;
        length += Math.hypot(current[0] - previous[0], current[1] - previous[1], current[2] - previous[2]);
    }
    return length;
}

