import { describe, expect, test } from "bun:test";

import { fileFromAssetBlob } from "./canvas-asset-space-media";

describe("fileFromAssetBlob", () => {
    test("uses the path basename and infers a mime when the blob is generic", () => {
        const file = fileFromAssetBlob(new Blob(["abc"], { type: "application/octet-stream" }), "idea2video/final_video.mp4", "clip", "video");
        expect(file.name).toBe("final_video.mp4");
        expect(file.type).toBe("video/mp4");
    });
});
