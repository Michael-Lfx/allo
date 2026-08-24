import { readFileSync } from "node:fs";
import { describe, expect, test } from "bun:test";

import { createDirectorScene } from "./director-scene-create";

const source = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");

describe("director scene create factories", () => {
    test("does not import three", () => {
        expect(source("./director-scene-create.ts").includes('from "three"')).toBe(false);
    });

    test("createDirectorScene keeps the default actor, camera, and lights", () => {
        const scene = createDirectorScene("镜头 1");
        expect(scene.title).toBe("镜头 1");
        expect(scene.version).toBe(1);
        expect(scene.objects).toHaveLength(1);
        expect(scene.objects[0]?.kind).toBe("actor");
        expect(scene.objects[0]?.name).toBe("演员 1");
        expect(scene.cameras).toHaveLength(1);
        expect(scene.lights.map((light) => light.name)).toEqual(["主光", "轮廓光", "环境光"]);
        expect(scene.shots).toHaveLength(1);
        expect(scene.activeShotId).toBe(scene.shots[0]?.id);
        expect(scene.cameras[0]?.id).toBe(scene.shots[0]?.cameraId);
    });
});
