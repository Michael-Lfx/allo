const PORTAL_HOST_CLASS = "oc-portal-host";
const CANVAS_SHELL_SELECTOR = ".oc-root.oc-canvas:not(.oc-portal-host)";
const SHELL_SELECTOR = ".oc-root:not(.oc-portal-host)";

let themeObserver: MutationObserver | null = null;
let observedShell: HTMLElement | null = null;

function isDisplayed(el: HTMLElement): boolean {
    return el.getClientRects().length > 0;
}

function findShell(): HTMLElement | null {
    const canvases = Array.from(document.querySelectorAll<HTMLElement>(CANVAS_SHELL_SELECTOR));
    for (let i = canvases.length - 1; i >= 0; i -= 1) {
        const canvas = canvases[i];
        if (canvas && isDisplayed(canvas)) return canvas;
    }
    const nestedCanvas = canvases[canvases.length - 1];
    if (nestedCanvas) return nestedCanvas;
    return document.querySelector<HTMLElement>(SHELL_SELECTOR);
}

function syncHostTheme(host: HTMLElement): void {
    host.classList.toggle("dark", Boolean(findShell()?.classList.contains("dark")));
}

function observeActiveShell(host: HTMLElement): void {
    const shell = findShell();
    if (shell === observedShell) {
        return;
    }
    themeObserver?.disconnect();
    themeObserver = null;
    observedShell = shell;
    if (!shell) {
        return;
    }
    themeObserver = new MutationObserver(() => syncHostTheme(host));
    themeObserver.observe(shell, { attributes: true, attributeFilter: ["class"] });
}

export function getOcPortalHost(): HTMLElement {
    let host = document.querySelector<HTMLElement>(`.${PORTAL_HOST_CLASS}`);
    if (!host) {
        host = document.createElement("div");
        host.className = `oc-root oc-canvas ${PORTAL_HOST_CLASS}`;
        document.body.appendChild(host);
    }
    observeActiveShell(host);
    syncHostTheme(host);
    return host;
}

export function disposeOcPortalHost(): void {
    themeObserver?.disconnect();
    themeObserver = null;
    observedShell = null;
    const host = document.querySelector<HTMLElement>(`.${PORTAL_HOST_CLASS}`);
    if (host && host.childElementCount === 0) {
        host.classList.remove("dark");
    }
}
