export type SettingsSection = "channels" | "models" | "preferences" | "prompts" | "storage";

export function settingsPath(section: SettingsSection = "models", continueCreation = false) {
    // allo 模型中心：画布缺省跳转到文本 / 多模态模型配置。
    const params = new URLSearchParams({ section: section === "channels" ? "models" : section });
    if (continueCreation) params.set("continue", "1");
    return `/models?${params.toString()}`;
}

/**
 * 画布深层组件没有路由上下文出口时统一跳转到 allo 模型中心。
 */
export function navigateToSettings(options?: { section?: SettingsSection; continueCreation?: boolean }) {
    const to = settingsPath(options?.section, options?.continueCreation);
    const event = new CustomEvent<{ to: string }>("workspace:navigate", { detail: { to }, cancelable: true });
    if (window.dispatchEvent(event)) {
        // Prefer SPA navigation when a host listener handles the event.
        window.location.assign(to);
    }
}
