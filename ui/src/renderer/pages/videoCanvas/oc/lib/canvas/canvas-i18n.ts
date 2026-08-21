import i18n from "i18next";

/**
 * OC 画布文案：走全局 i18n，defaultValue 兜底中文。
 * 注册表等非 React 上下文也可用；组件内请另调 useTranslation() 以订阅语言切换。
 */
export function canvasT(key: string, defaultValue: string, options?: Record<string, unknown>): string {
    return String(i18n.t(key, { defaultValue, ...(options || {}) }));
}
