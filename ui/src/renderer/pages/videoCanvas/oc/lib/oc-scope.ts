const PORTAL_HOST_CLASS = "oc-portal-host";
const SHELL_SELECTOR = ".oc-root:not(.oc-portal-host)";

let themeObserver: MutationObserver | null = null;
let observedShell: HTMLElement | null = null;

function findShell(): HTMLElement | null {
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
}
