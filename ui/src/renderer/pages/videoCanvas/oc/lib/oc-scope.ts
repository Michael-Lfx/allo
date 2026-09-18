const PORTAL_HOST_CLASS = "oc-portal-host";
const SHELL_SELECTOR = ".oc-root:not(.oc-portal-host)";

let themeObserver: MutationObserver | null = null;

function findShell(): HTMLElement | null {
    return document.querySelector<HTMLElement>(SHELL_SELECTOR);
}

function syncHostTheme(host: HTMLElement): void {
    host.classList.toggle("dark", Boolean(findShell()?.classList.contains("dark")));
}

export function getOcPortalHost(): HTMLElement {
    let host = document.querySelector<HTMLElement>(`.${PORTAL_HOST_CLASS}`);
    if (!host) {
        host = document.createElement("div");
        host.className = `oc-root ${PORTAL_HOST_CLASS}`;
        document.body.appendChild(host);

        themeObserver?.disconnect();
        themeObserver = null;
        const shell = findShell();
        if (shell) {
            themeObserver = new MutationObserver(() => syncHostTheme(host as HTMLElement));
            themeObserver.observe(shell, { attributes: true, attributeFilter: ["class"] });
        }
    }
    syncHostTheme(host);
    return host;
}
