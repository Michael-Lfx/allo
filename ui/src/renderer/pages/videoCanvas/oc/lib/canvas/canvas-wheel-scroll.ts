export type ScrollSnapshot = {
    overflowX: string;
    overflowY: string;
    scrollWidth: number;
    clientWidth: number;
    scrollHeight: number;
    clientHeight: number;
};

export type WheelScrollPlan = {
    index: number;
    top: number;
    left: number;
};

const SCROLLABLE_OVERFLOW = new Set(["auto", "scroll", "overlay"]);

function isScrollableOverflow(value: string): boolean {
    return SCROLLABLE_OVERFLOW.has(value);
}

/**
 * Pick the nearest ancestor that can actually scroll.
 * `overflow: hidden` clips the canvas but must not swallow the page wheel.
 */
export function planWheelScroll(chain: ScrollSnapshot[], deltaX: number, deltaY: number): WheelScrollPlan | null {
    if (!Number.isFinite(deltaX) || !Number.isFinite(deltaY)) return null;
    if (deltaX === 0 && deltaY === 0) return null;
    const vertical = Math.abs(deltaY) >= Math.abs(deltaX);
    for (let index = 0; index < chain.length; index += 1) {
        const node = chain[index];
        const canY = isScrollableOverflow(node.overflowY) && node.scrollHeight > node.clientHeight;
        const canX = isScrollableOverflow(node.overflowX) && node.scrollWidth > node.clientWidth;
        if (!(vertical ? canY : canX || canY)) continue;
        return {
            index,
            top: canY ? deltaY : 0,
            left: canX ? deltaX : 0,
        };
    }
    return null;
}

export function forwardWheelToScrollableAncestor(start: HTMLElement | null, deltaX: number, deltaY: number): boolean {
    const elements: HTMLElement[] = [];
    const chain: ScrollSnapshot[] = [];
    let node = start?.parentElement ?? null;
    while (node) {
        const style = getComputedStyle(node);
        elements.push(node);
        chain.push({
            overflowX: style.overflowX,
            overflowY: style.overflowY,
            scrollWidth: node.scrollWidth,
            clientWidth: node.clientWidth,
            scrollHeight: node.scrollHeight,
            clientHeight: node.clientHeight,
        });
        node = node.parentElement;
    }
    const plan = planWheelScroll(chain, deltaX, deltaY);
    if (!plan) return false;
    elements[plan.index]?.scrollBy({ top: plan.top, left: plan.left, behavior: "auto" });
    return true;
}
