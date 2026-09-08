import { describe, expect, test } from "bun:test";

import { formatSubtitleClock, parseSubtitleClock, serializeVttEntries } from "./srt-parser";

describe("subtitle clocks", () => {
    test("round-trips mm:ss.mmm", () => {
        expect(formatSubtitleClock(65_250)).toBe("01:05.250");
        expect(parseSubtitleClock("01:05.250", 0)).toBe(65_250);
        expect(parseSubtitleClock("1:05,25", 0)).toBe(65_250);
        expect(parseSubtitleClock("bad", 12)).toBe(12);
    });
});

describe("serializeVttEntries", () => {
    test("writes a WebVTT file with dotted milliseconds", () => {
        const vtt = serializeVttEntries([
            { index: 1, startMs: 0, endMs: 1200, text: "Hello" },
            { index: 2, startMs: 1500, endMs: 2400, text: "World" },
        ]);
        expect(vtt.startsWith("WEBVTT\n\n")).toBe(true);
        expect(vtt).toContain("00:00:00.000 --> 00:00:01.200\nHello");
        expect(vtt).toContain("00:00:01.500 --> 00:00:02.400\nWorld");
    });
});
