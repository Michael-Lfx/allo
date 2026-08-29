import { Color, Euler, Quaternion } from "three";

import type { DirectorBoneKeyframe, DirectorHumanoidBone, DirectorKeyframe, DirectorPose, DirectorQuat, DirectorTransform, DirectorVec3 } from "@oc/types/director";
import { DIRECTOR_KEYFRAME_EPSILON, resolveDirectorKeyframeProgress } from "./director-scene-create";

export {
    DIRECTOR_ACTOR_COLORS,
    DIRECTOR_DEFAULT_ACTOR_URL,
    DIRECTOR_KEYFRAME_EPSILON,
    createDirectorActor,
    createDirectorBillboard,
    createDirectorCamera,
    createDirectorLight,
    createDirectorModel,
    createDirectorObject,
    createDirectorScene,
    directorFocalLengthToFov,
    directorIdentityTransform,
    directorTransformPathLength,
    finiteDirectorTransformKeyframes,
    removeDirectorBoneKeyframe,
    removeDirectorKeyframe,
    removeDirectorSceneKeyframe,
    resolveDirectorKeyframeProgress,
    setDirectorSceneKeyframeEasing,
    touchDirectorScene,
    upsertDirectorBoneKeyframe,
    upsertDirectorKeyframe,
} from "./director-scene-create";

export function interpolateDirectorTransform(base: DirectorTransform, keyframes: DirectorKeyframe[], time: number): DirectorTransform {
    if (!keyframes.length) return base;
    const previous = [...keyframes].reverse().find((item) => item.time <= time) || keyframes[0];
    const next = keyframes.find((item) => item.time >= time) || keyframes[keyframes.length - 1];
    if (previous.id === next.id) return previous.transform;
    const progress = resolveDirectorKeyframeProgress((time - previous.time) / Math.max(next.time - previous.time, DIRECTOR_KEYFRAME_EPSILON), previous.easing);
    const rotation = new Quaternion().setFromEuler(new Euler(...previous.transform.rotation)).slerp(new Quaternion().setFromEuler(new Euler(...next.transform.rotation)), progress);
    return {
        position: lerpVec3(previous.transform.position, next.transform.position, progress),
        rotation: new Euler().setFromQuaternion(rotation).toArray().slice(0, 3) as DirectorVec3,
        scale: lerpVec3(previous.transform.scale, next.transform.scale, progress),
    };
}

export function interpolateDirectorBoneRotation(base: DirectorQuat, keyframes: DirectorBoneKeyframe[], time: number): DirectorQuat {
    if (!keyframes.length) return base;
    const previous = [...keyframes].reverse().find((item) => item.time <= time) || keyframes[0];
    const next = keyframes.find((item) => item.time >= time) || keyframes[keyframes.length - 1];
    if (previous.id === next.id) return previous.rotation;
    const progress = resolveDirectorKeyframeProgress((time - previous.time) / Math.max(next.time - previous.time, DIRECTOR_KEYFRAME_EPSILON), previous.easing);
    return new Quaternion(...previous.rotation).slerp(new Quaternion(...next.rotation), progress).toArray() as DirectorQuat;
}

export function directorBoneLabel(bone: string) {
    return ({
        hips: "骨盆", spine: "脊柱", chest: "胸腔", neck: "颈部", head: "头部",
        leftShoulder: "左肩", leftUpperArm: "左上臂", leftLowerArm: "左前臂", leftHand: "左手",
        rightShoulder: "右肩", rightUpperArm: "右上臂", rightLowerArm: "右前臂", rightHand: "右手",
        leftUpperLeg: "左大腿", leftLowerLeg: "左小腿", leftFoot: "左脚",
        rightUpperLeg: "右大腿", rightLowerLeg: "右小腿", rightFoot: "右脚",
        leftThumb1: "左拇指·根", leftThumb2: "左拇指·中", leftThumb3: "左拇指·尖",
        leftIndex1: "左食指·根", leftIndex2: "左食指·中", leftIndex3: "左食指·尖",
        leftMiddle1: "左中指·根", leftMiddle2: "左中指·中", leftMiddle3: "左中指·尖",
        leftRing1: "左无名指·根", leftRing2: "左无名指·中", leftRing3: "左无名指·尖",
        leftPinky1: "左小指·根", leftPinky2: "左小指·中", leftPinky3: "左小指·尖",
        rightThumb1: "右拇指·根", rightThumb2: "右拇指·中", rightThumb3: "右拇指·尖",
        rightIndex1: "右食指·根", rightIndex2: "右食指·中", rightIndex3: "右食指·尖",
        rightMiddle1: "右中指·根", rightMiddle2: "右中指·中", rightMiddle3: "右中指·尖",
        rightRing1: "右无名指·根", rightRing2: "右无名指·中", rightRing3: "右无名指·尖",
        rightPinky1: "右小指·根", rightPinky2: "右小指·中", rightPinky3: "右小指·尖",
    } as Record<string, string>)[bone] || bone;
}

export function directorPoseLabel(pose: DirectorPose) {
    return ({ neutral: "自然", stand: "站立", t_pose: "T 型", walk: "行走", run: "跑步", sit: "坐姿", squat: "蹲下", kneel_single: "单膝跪", kneel_double: "双膝跪", hands_hips: "叉腰", lean: "倚靠", bow: "鞠躬", think: "思考", fight: "格斗", kick: "踢球", throw: "投掷", push: "推进", wave: "招手", reach: "伸手", arms_crossed: "抱臂", phone: "看手机" } as Record<DirectorPose, string>)[pose];
}

export function directorPoseBoneDeltas(pose: DirectorPose): Partial<Record<DirectorHumanoidBone, DirectorQuat>> {
    // Soldier 的左右上臂局部 Z 轴方向一致，正向旋转才会把两侧手臂从 T Pose 放下。
    const armsDown = { leftUpperArm: poseQuaternion(0, 0, 1.28), rightUpperArm: poseQuaternion(0, 0, 1.28) };
    const poses: Record<DirectorPose, Partial<Record<DirectorHumanoidBone, DirectorQuat>>> = {
        neutral: armsDown,
        stand: armsDown,
        t_pose: {},
        walk: { ...armsDown, leftUpperArm: poseQuaternion(0.36, 0, 1.2), rightUpperArm: poseQuaternion(-0.36, 0, 1.2), leftUpperLeg: poseQuaternion(-0.32, 0, 0), rightUpperLeg: poseQuaternion(0.32, 0, 0) },
        run: { ...armsDown, leftUpperArm: poseQuaternion(0.75, 0, 1.05), rightUpperArm: poseQuaternion(-0.75, 0, 1.05), leftLowerArm: poseQuaternion(-0.7, 0, 0), rightLowerArm: poseQuaternion(-0.7, 0, 0), leftUpperLeg: poseQuaternion(-0.65, 0, 0), rightUpperLeg: poseQuaternion(0.55, 0, 0), rightLowerLeg: poseQuaternion(0.8, 0, 0) },
        sit: { ...armsDown, leftUpperLeg: poseQuaternion(-1.35, 0, 0), rightUpperLeg: poseQuaternion(-1.35, 0, 0), leftLowerLeg: poseQuaternion(1.25, 0, 0), rightLowerLeg: poseQuaternion(1.25, 0, 0) },
        squat: { ...armsDown, hips: poseQuaternion(0.25, 0, 0), leftUpperLeg: poseQuaternion(-0.75, 0, 0), rightUpperLeg: poseQuaternion(-0.75, 0, 0), leftLowerLeg: poseQuaternion(1.2, 0, 0), rightLowerLeg: poseQuaternion(1.2, 0, 0) },
        kneel_single: { ...armsDown, leftUpperLeg: poseQuaternion(-0.95, 0, 0), leftLowerLeg: poseQuaternion(1.45, 0, 0), rightUpperLeg: poseQuaternion(-0.35, 0, 0), rightLowerLeg: poseQuaternion(0.75, 0, 0) },
        kneel_double: { ...armsDown, leftUpperLeg: poseQuaternion(-0.55, 0, 0), rightUpperLeg: poseQuaternion(-0.55, 0, 0), leftLowerLeg: poseQuaternion(1.45, 0, 0), rightLowerLeg: poseQuaternion(1.45, 0, 0) },
        hands_hips: { leftUpperArm: poseQuaternion(0, 0, 0.75), rightUpperArm: poseQuaternion(0, 0, 0.75), leftLowerArm: poseQuaternion(-0.1, 0.2, -1.5), rightLowerArm: poseQuaternion(-0.1, -0.2, 1.5) },
        lean: { ...armsDown, hips: poseQuaternion(0, 0, 0.18), spine: poseQuaternion(0, 0, -0.12), head: poseQuaternion(0, 0, -0.08) },
        bow: { ...armsDown, hips: poseQuaternion(0.5, 0, 0), spine: poseQuaternion(0.28, 0, 0), head: poseQuaternion(-0.18, 0, 0) },
        think: { ...armsDown, rightUpperArm: poseQuaternion(-0.25, 0, 0.55), rightLowerArm: poseQuaternion(-1.35, 0, 0.3), head: poseQuaternion(0.05, -0.22, 0) },
        fight: { leftUpperArm: poseQuaternion(-0.65, 0, 0.7), rightUpperArm: poseQuaternion(-0.55, 0, 0.65), leftLowerArm: poseQuaternion(-1.2, 0, 0), rightLowerArm: poseQuaternion(-1.25, 0, 0), chest: poseQuaternion(0, 0.2, 0), leftUpperLeg: poseQuaternion(-0.15, 0, 0), rightUpperLeg: poseQuaternion(0.2, 0, 0) },
        kick: { ...armsDown, leftUpperArm: poseQuaternion(0.3, 0, 1.1), rightUpperArm: poseQuaternion(-0.3, 0, 1.1), rightUpperLeg: poseQuaternion(-1.1, 0, 0), rightLowerLeg: poseQuaternion(0.35, 0, 0) },
        throw: { leftUpperArm: poseQuaternion(-0.35, 0.2, 0.35), rightUpperArm: poseQuaternion(-1.2, 0, 0.25), rightLowerArm: poseQuaternion(-1.05, 0, 0), chest: poseQuaternion(0, -0.3, 0) },
        push: { leftUpperArm: poseQuaternion(-0.9, 0, 0.3), rightUpperArm: poseQuaternion(-0.9, 0, 0.3), leftLowerArm: poseQuaternion(-0.35, 0, 0), rightLowerArm: poseQuaternion(-0.35, 0, 0), chest: poseQuaternion(0.15, 0, 0) },
        wave: { ...armsDown, rightUpperArm: poseQuaternion(0, 0, -0.35), rightLowerArm: poseQuaternion(0, 0, -1.45), rightHand: poseQuaternion(0, 0, -0.3) },
        reach: { ...armsDown, rightUpperArm: poseQuaternion(-1.35, 0, -0.05), rightLowerArm: poseQuaternion(-0.1, 0, 0) },
        arms_crossed: { leftUpperArm: poseQuaternion(-0.65, 0, 0.65), rightUpperArm: poseQuaternion(-0.65, 0, 0.65), leftLowerArm: poseQuaternion(-1.2, 0.15, -0.4), rightLowerArm: poseQuaternion(-1.2, -0.15, 0.4) },
        phone: { ...armsDown, leftUpperArm: poseQuaternion(-0.45, 0, 0.95), rightUpperArm: poseQuaternion(-0.45, 0, 0.95), leftLowerArm: poseQuaternion(-1.1, 0, 0.15), rightLowerArm: poseQuaternion(-1.1, 0, -0.15), head: poseQuaternion(0.28, 0, 0) },
    };
    return poses[pose];
}

export function directorColorLabel(value: string) {
    const hsl = { h: 0, s: 0, l: 0 };
    new Color(value).getHSL(hsl);
    if (hsl.l >= 0.86 && hsl.s <= 0.2) return "白色";
    if (hsl.l <= 0.18) return "黑色";
    if (hsl.s <= 0.16) return hsl.l >= 0.52 ? "浅灰色" : "深灰色";
    const hue = hsl.h * 360;
    if (hue < 18 || hue >= 345) return "红色";
    if (hue < 48) return "橙色";
    if (hue < 72) return "黄色";
    if (hue < 165) return "绿色";
    if (hue < 195) return "青色";
    if (hue < 255) return "蓝色";
    if (hue < 290) return "紫色";
    return "粉色";
}

function poseQuaternion(x: number, y: number, z: number): DirectorQuat {
    return new Quaternion().setFromEuler(new Euler(x, y, z)).toArray() as DirectorQuat;
}

function lerpVec3(from: DirectorVec3, to: DirectorVec3, progress: number): DirectorVec3 {
    return from.map((value, index) => value + (to[index] - value) * progress) as DirectorVec3;
}
