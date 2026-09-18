import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

function publicFile(relativePath: string) {
    return readFileSync(join(process.cwd(), "public", relativePath));
}

function ascii(bytes: Buffer, start: number, length: number) {
    return bytes.subarray(start, start + length).toString("ascii");
}

describe("canvas static assets", () => {
    test("emotion and face-detector files are real binaries, not SPA HTML", () => {
        const glb = publicFile("canvas/models/facecap.glb");
        const tflite = publicFile("canvas/models/blaze-face-full-range-sparse.tflite");
        const wasm = publicFile("three/basis/basis_transcoder.wasm");
        const js = publicFile("three/basis/basis_transcoder.js");

        expect(ascii(glb, 0, 4)).toBe("glTF");
        expect(ascii(tflite, 4, 4)).toBe("TFL3");
        expect(wasm.subarray(0, 4).equals(Buffer.from([0, 0x61, 0x73, 0x6d]))).toBe(true);
        expect(js.includes("BASIS")).toBe(true);
        expect(ascii(glb, 0, 9).toLowerCase().startsWith("<!doctype")).toBe(false);
    });
});
