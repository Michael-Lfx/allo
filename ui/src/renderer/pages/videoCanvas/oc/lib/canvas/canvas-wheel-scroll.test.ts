import { describe, expect, test } from "bun:test";

import { planWheelScroll, type ScrollSnapshot } from "./canvas-wheel-scroll";

function box(partial: Partial<ScrollSnapshot> & Pick<ScrollSnapshot, "overflowX" | "overflowY">): ScrollSnapshot {
    return {
        scrollWidth: 100,
        clientWidth: 100,
        scrollHeight: 100,
        clientHeight: 100,
        ...partial,
    };
}

describe("planWheelScroll", () => {
    test("skips clipped canvas layers and scrolls the page", () => {
        const chain = [
            box({ overflowX: "hidden", overflowY: "hidden", scrollHeight: 800, clientHeight: 400 }),
            box({ overflowX: "visible", overflowY: "visible", scrollHeight: 900, clientHeight: 400 }),
            box({ overflowX: "hidden", overflowY: "auto", scrollHeight: 1600, clientHeight: 700 }),
        ];

        expect(planWheelScroll(chain, 0, 120)).toEqual({ index: 2, top: 120, left: 0 });
    });

    test("uses the nearest vertical scroller and leaves the one below it", () => {
        const chain = [
            box({ overflowX: "hidden", overflowY: "auto", scrollHeight: 900, clientHeight: 400 }),
            box({ overflowX: "hidden", overflowY: "auto", scrollHeight: 2000, clientHeight: 800 }),
        ];

        expect(planWheelScroll(chain, 0, -80)?.index).toBe(0);
    });

    test("does not stop on a horizontal strip when the wheel is vertical", () => {
        const chain = [
            box({ overflowX: "auto", overflowY: "hidden", scrollWidth: 900, clientWidth: 300 }),
            box({ overflowX: "hidden", overflowY: "scroll", scrollHeight: 1200, clientHeight: 500 }),
        ];

        expect(planWheelScroll(chain, 0, 100)).toEqual({ index: 1, top: 100, left: 0 });
    });

    test("ignores an empty wheel and non-finite deltas", () => {
        const chain = [box({ overflowX: "auto", overflowY: "auto", scrollHeight: 400, clientHeight: 100 })];
        expect(planWheelScroll(chain, 0, 0)).toBeNull();
        expect(planWheelScroll(chain, Number.NaN, 10)).toBeNull();
    });
});
