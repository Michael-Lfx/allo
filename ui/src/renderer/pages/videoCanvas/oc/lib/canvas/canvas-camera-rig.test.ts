import { describe, expect, test } from "bun:test";

import {
    cameraRigFingerprint,
    compactCameraRigToken,
    compileCameraRigPrompt,
    DEFAULT_CAMERA_RIG,
    readCameraRig,
    stepCameraIndex,
} from "./canvas-camera-rig";

describe("canvas camera rig", () => {
    test("defaults to cinema gear with the camera off", () => {
        expect(readCameraRig(undefined)).toEqual(DEFAULT_CAMERA_RIG);
        expect(DEFAULT_CAMERA_RIG).toMatchObject({ enabled: false, body: "dxl2", lens: "signature-prime", focal: "35", aperture: "4" });
        expect(cameraRigFingerprint(DEFAULT_CAMERA_RIG)).toBeUndefined();
        expect(compactCameraRigToken(DEFAULT_CAMERA_RIG)).toBe("");
        expect(compileCameraRigPrompt("一只猫", DEFAULT_CAMERA_RIG)).toBe("一只猫");
    });

    test("migrates legacy chip focals without dropping the look", () => {
        expect(readCameraRig({ cameraLens: "50", cameraAperture: "nope" })).toEqual({
            ...DEFAULT_CAMERA_RIG,
            enabled: true,
            focal: "50",
        });
        expect(readCameraRig({ cameraBody: "full-frame" }).body).toBe("venice2");
        expect(readCameraRig({ cameraEnabled: "false", cameraLens: "50" }).enabled).toBe(false);
    });

    test("compiles cinema language without rewriting the user prompt", () => {
        const prompt = compileCameraRigPrompt("废土风超市门口", {
            enabled: true,
            body: "venice2",
            lens: "signature-prime",
            focal: "50",
            aperture: "1.4",
            angle: "eye",
            shot: "medium",
        });
        expect(prompt.startsWith("废土风超市门口")).toBe(true);
        expect(prompt).toContain("【摄像机】");
        expect(prompt).toContain("Sony Venice 2");
        expect(prompt).toContain("Arri Signature Prime 50mm");
        expect(prompt).toContain("f/1.4");
        expect(prompt).toContain("平视");
        expect(prompt).toContain("中景");
    });

    test("skips injection when the camera is off or the prompt already has a camera block", () => {
        const existing = "一只猫\n【摄像机】已指定 35mm";
        expect(compileCameraRigPrompt(existing, { ...DEFAULT_CAMERA_RIG, enabled: true, focal: "85" })).toBe(existing);
        expect(compileCameraRigPrompt("一只猫", { ...DEFAULT_CAMERA_RIG, enabled: false, focal: "85" })).toBe("一只猫");
    });

    test("compact token and fingerprint only apply when the camera is on", () => {
        expect(compactCameraRigToken({ ...DEFAULT_CAMERA_RIG, enabled: true, focal: "50", aperture: "1.4" })).toBe("50mm · f/1.4");
        expect(cameraRigFingerprint({ ...DEFAULT_CAMERA_RIG, enabled: true, focal: "85" })).toEqual({
            cameraEnabled: "true",
            cameraBody: "dxl2",
            cameraLens: "signature-prime",
            cameraFocal: "85",
            cameraAperture: "4",
        });
    });

    test("wheel indexes wrap", () => {
        expect(stepCameraIndex(0, 6, -1)).toBe(5);
        expect(stepCameraIndex(5, 6, 1)).toBe(0);
    });
});
