import { describe, expect, test } from "bun:test";

import { transcriptToSrtEntries } from "@oc/lib/timeline/transcript-to-srt";

describe("transcriptToSrtEntries", () => {
    test("splits sentences and spans the given duration", () => {
        const entries = transcriptToSrtEntries("你好。这是第二句！还没完？", 9000);
        expect(entries.map((entry) => entry.text)).toEqual(["你好。", "这是第二句！", "还没完？"]);
        expect(entries[0].startMs).toBe(0);
        expect(entries.at(-1)?.endMs).toBe(9000);
        expect(entries.every((entry) => entry.endMs > entry.startMs)).toBe(true);
    });

    test("returns empty for blank ASR text", () => {
        expect(transcriptToSrtEntries("   ")).toEqual([]);
    });
});
