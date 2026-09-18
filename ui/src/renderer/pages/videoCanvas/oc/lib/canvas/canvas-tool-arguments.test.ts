import { describe, expect, test } from "bun:test";

import { encodeToolArguments, parseToolArguments } from "@oc/lib/canvas/canvas-tool-arguments";

function throws(fn: () => unknown) {
    try {
        fn();
        return false;
    } catch {
        return true;
    }
}

describe("parseToolArguments", () => {
    test("parses a JSON object payload", () => {
        expect(parseToolArguments('{"id":"n1","count":2}')).toEqual({ id: "n1", count: 2 });
    });

    test("accepts an already-parsed object from OpenAI-compatible proxies", () => {
        expect(parseToolArguments({ nodes: [{ ref: "script", kind: "text", title: "剧本" }] })).toEqual({
            nodes: [{ ref: "script", kind: "text", title: "剧本" }],
        });
    });

    test("unwraps markdown fences and argument prefixes", () => {
        expect(parseToolArguments("```json\n{\"prompt\":\"一只猫\"}\n```")).toEqual({ prompt: "一只猫" });
        expect(parseToolArguments('arguments: {"prompt":"一只猫"}')).toEqual({ prompt: "一只猫" });
    });

    test("repairs trailing commas and truncated workflow payloads", () => {
        expect(parseToolArguments('{"nodes":[{"ref":"a","kind":"text","title":"剧本",}],}')).toEqual({
            nodes: [{ ref: "a", kind: "text", title: "剧本" }],
        });
        expect(parseToolArguments('{"nodes":[{"ref":"a","kind":"text","title":"剧本","content":"开场')).toEqual({
            nodes: [{ ref: "a", kind: "text", title: "剧本", content: "开场" }],
        });
    });

    test("parses double-encoded object strings", () => {
        expect(parseToolArguments(JSON.stringify(JSON.stringify({ prompt: "一只猫" })))).toEqual({ prompt: "一只猫" });
    });

    test("treats empty input as an empty object", () => {
        expect(parseToolArguments("")).toEqual({});
        expect(parseToolArguments(null)).toEqual({});
    });

    test("rejects non-object JSON that cannot be repaired into an object", () => {
        expect(throws(() => parseToolArguments("[1,2]"))).toBe(true);
        expect(throws(() => parseToolArguments("not json"))).toBe(true);
        expect(throws(() => parseToolArguments('"text"'))).toBe(true);
    });
});

describe("encodeToolArguments", () => {
    test("keeps strings and serializes objects", () => {
        expect(encodeToolArguments('{"a":1}')).toBe('{"a":1}');
        expect(encodeToolArguments({ a: 1 })).toBe('{"a":1}');
        expect(encodeToolArguments(undefined)).toBe("{}");
    });
});
