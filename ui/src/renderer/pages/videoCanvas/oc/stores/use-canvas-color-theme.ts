import { createContext, createElement, useContext, type ReactNode } from "react";

import { useThemeStore, type ThemeName } from "./use-theme-store";

const CanvasColorThemeScopeContext = createContext<ThemeName | null>(null);

/** Embed a canvas (storyboard) on the app theme without rewriting canvas-mode persistence. */
export function CanvasColorThemeScope({ theme, children }: { theme: ThemeName; children: ReactNode }) {
    return createElement(CanvasColorThemeScopeContext.Provider, { value: theme }, children);
}

export function useCanvasColorTheme(): ThemeName {
    const scoped = useContext(CanvasColorThemeScopeContext);
    const stored = useThemeStore((state) => state.theme);
    return scoped ?? stored;
}
