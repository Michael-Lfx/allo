import type { DirectorActorPresetId, DirectorPose, DirectorVec3 } from "@oc/types/director";

export const DIRECTOR_DEFAULT_ACTOR_URL = "https://cdn.jsdelivr.net/gh/mrdoob/three.js@r185/examples/models/gltf/Xbot.glb";
export const DIRECTOR_SOLDIER_ACTOR_URL = "https://cdn.jsdelivr.net/gh/mrdoob/three.js@r185/examples/models/gltf/Soldier.glb";

export type DirectorActorPreset = {
    id: DirectorActorPresetId;
    name: string;
    url: string;
    scale: DirectorVec3;
    color: string;
    pose: DirectorPose;
};

/**
 * 人体素模身份符号。不新开模型管线：复用 three.js 示例 GLB + 缩放/颜色/姿势，
 * 用来区分男女老少站位，而不是替换可上传的自定义角色。
 */
export const DIRECTOR_ACTOR_PRESETS: readonly DirectorActorPreset[] = [
    { id: "adult_male", name: "成年男", url: DIRECTOR_SOLDIER_ACTOR_URL, scale: [1, 1, 1], color: "#d8dde3", pose: "stand" },
    { id: "adult_female", name: "成年女", url: DIRECTOR_DEFAULT_ACTOR_URL, scale: [0.94, 0.94, 0.94], color: "#f1d5c8", pose: "stand" },
    { id: "child", name: "儿童", url: DIRECTOR_DEFAULT_ACTOR_URL, scale: [0.58, 0.58, 0.58], color: "#dfae3f", pose: "stand" },
    { id: "elder", name: "老人", url: DIRECTOR_SOLDIER_ACTOR_URL, scale: [0.96, 0.94, 0.96], color: "#9aa3ad", pose: "lean" },
];

export function resolveDirectorActorPreset(id?: DirectorActorPresetId): DirectorActorPreset {
    return DIRECTOR_ACTOR_PRESETS.find((item) => item.id === id) || DIRECTOR_ACTOR_PRESETS[0];
}
