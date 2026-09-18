import { describe, expect, test } from "bun:test";

import { parseDurationSeconds, resolveWorkflowStoryboardRows } from "./canvas-agent-workflow-script";

describe("resolveWorkflowStoryboardRows", () => {
    test("splits labeled scenes and fits durations to the clip length", () => {
        const rows = resolveWorkflowStoryboardRows({
            content: "场景1（5s）：清晨小猫醒来。\n场景2（5s）：正午窗台晒太阳。\n场景3（5s）：夜晚窝里睡觉。",
            imageCount: 3,
            totalDurationSeconds: 6,
        });
        expect(rows).toHaveLength(3);
        expect(rows.map((row) => row.plotDescription).join(" ")).toContain("清晨小猫醒来");
        expect(rows.map((row) => row.plotDescription).join(" ")).toContain("夜晚窝里睡觉");
        expect(rows.reduce((sum, row) => sum + row.durationSeconds, 0)).toBe(6);
        expect(rows.every((row) => row.imageGenerationPrompt)).toBe(true);
    });

    test("uses explicit shots when provided", () => {
        const rows = resolveWorkflowStoryboardRows({
            shots: [
                { plotDescription: "起床", dialogue: "喵", durationSeconds: 2, imagePrompt: "清晨窗边的小猫" },
                { plotDescription: "午睡", durationSeconds: 4 },
            ],
            totalDurationSeconds: 6,
        });
        expect(rows).toHaveLength(2);
        expect(rows[0]?.plotDescription).toBe("起床");
        expect(rows[0]?.dialogue).toBe("喵");
        expect(rows[0]?.imageGenerationPrompt).toBe("清晨窗边的小猫");
        expect(rows.map((row) => row.durationSeconds)).toEqual([2, 4]);
    });

    test("aligns a single paragraph to the keyframe count", () => {
        const rows = resolveWorkflowStoryboardRows({
            content: "小猫的一天。先醒来，再玩耍，最后睡觉。",
            imageCount: 3,
            totalDurationSeconds: 6,
        });
        expect(rows).toHaveLength(3);
        expect(rows.reduce((sum, row) => sum + row.durationSeconds, 0)).toBe(6);
    });
});

describe("parseDurationSeconds", () => {
    test("reads numbers and leading numeric strings", () => {
        expect(parseDurationSeconds(6)).toBe(6);
        expect(parseDurationSeconds("6")).toBe(6);
        expect(parseDurationSeconds("6s")).toBe(6);
        expect(parseDurationSeconds("nope")).toBeUndefined();
    });
});
