import { describe, expect, test } from "bun:test";

import { directorGridCellRect, directorGridShape, listDirectorGridShots, resolveDirectorShotFraming } from "./director-camera-grid";
import { createDirectorScene } from "./director-scene-create";

describe("director camera grid", () => {
    test("picks 2x2 / 3x3 / 5-col layouts", () => {
        expect(directorGridShape(1)).toEqual({ cols: 1, rows: 1 });
        expect(directorGridShape(2)).toEqual({ cols: 2, rows: 1 });
        expect(directorGridShape(4)).toEqual({ cols: 2, rows: 2 });
        expect(directorGridShape(5)).toEqual({ cols: 3, rows: 2 });
        expect(directorGridShape(9)).toEqual({ cols: 3, rows: 3 });
        expect(directorGridShape(10)).toEqual({ cols: 5, rows: 2 });
    });

    test("places cells left-to-right then top-to-bottom", () => {
        expect(directorGridCellRect(3, { cols: 2, rows: 2 }, 512)).toEqual({ x: 512, y: 512, width: 512, height: 512 });
    });

    test("lists each shot that still has a camera", () => {
        const scene = createDirectorScene("镜头 1");
        const extra = { ...scene.shots[0]!, id: "shot-2", name: "镜头 2" };
        const listed = listDirectorGridShots({ ...scene, shots: [...scene.shots, extra] });
        expect(listed).toHaveLength(2);
        expect(listDirectorGridShots({ ...scene, shots: [{ ...extra, cameraId: "missing" }] })).toHaveLength(0);
    });

    test("resolves a shot framing without mutating the live activeShotId", () => {
        const scene = createDirectorScene("镜头 1");
        const extraShot = { ...scene.shots[0]!, id: "shot-2", name: "镜头 2" };
        const withTwo = { ...scene, shots: [...scene.shots, extraShot], activeShotId: scene.shots[0]!.id };
        const framing = resolveDirectorShotFraming(withTwo, "shot-2");
        expect(framing?.cameraId).toBe(extraShot.cameraId);
        expect(withTwo.activeShotId).toBe(scene.shots[0]!.id);
    });
});
