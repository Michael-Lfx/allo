/** 测量时关掉滚动条，避免 clientWidth 抖动导致文本重排、高度来回振荡。 */
export function measureElementScrollHeight(element: HTMLElement): number {
    const previousHeight = element.style.height;
    const previousOverflow = element.style.overflow;
    element.style.overflow = "hidden";
    element.style.height = "0px";
    const height = element.scrollHeight;
    element.style.height = previousHeight;
    element.style.overflow = previousOverflow;
    return height;
}

export function contentSizeShouldNotify(previous: number | null, next: number, epsilon = 1) {
    if (!Number.isFinite(next) || next < 0) return false;
    if (previous === null) return true;
    return Math.abs(previous - next) >= epsilon;
}

export function resizeObserverWidthChanged(previousWidth: number | null, nextWidth: number) {
    if (previousWidth === null) return true;
    return Math.abs(previousWidth - nextWidth) >= 1;
}
