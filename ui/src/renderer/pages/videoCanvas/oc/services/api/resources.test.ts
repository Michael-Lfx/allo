import { afterEach, describe, expect, test } from "bun:test";

import { getResource } from "./resources";

describe("getResource lookup hints", () => {
    const originalFetch = globalThis.fetch;
    let fetchCalls = 0;

    afterEach(() => {
        fetchCalls = 0;
        globalThis.fetch = originalFetch;
    });

    test("skips HEAD when node mimeType and bytes are provided", async () => {
        globalThis.fetch = (async () => {
            fetchCalls += 1;
            throw new Error("HEAD/GET should not run when lookup hints are present");
        }) as typeof fetch;

        const resource = await getResource(`hint-${crypto.randomUUID()}`, {
            mimeType: "image/png",
            bytes: 2048,
        });

        expect(resource.provider).toBe("allo");
        expect(resource.mimeType).toBe("image/png");
        expect(resource.size).toBe(2048);
        expect(resource.kind).toBe("image");
        expect(fetchCalls).toBe(0);
    });
});
