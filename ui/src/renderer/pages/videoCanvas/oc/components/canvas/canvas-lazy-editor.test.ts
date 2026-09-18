import { describe, expect, test } from "bun:test";

import { retryDynamicImport } from "@oc/components/canvas/canvas-lazy-editor";

describe("retryDynamicImport", () => {
    test("retries a failed loader then resolves", async () => {
        let calls = 0;
        const value = await retryDynamicImport(async () => {
            calls += 1;
            if (calls < 2) throw new TypeError("Failed to fetch dynamically imported module: canvas-script-editor.tsx");
            return { ok: true };
        });
        expect(calls).toBe(2);
        expect(value).toEqual({ ok: true });
    });
});
