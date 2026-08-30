

// context/ThemeContext.tsx - Unified Theme Management Context 统一主题管理上下文
import type { PropsWithChildren } from 'react';
import React, { createContext, useContext, useEffect } from 'react';
import type { Theme, ThemePreference } from '@renderer/hooks/system/useTheme';
import useTheme from '@renderer/hooks/system/useTheme';
import type { ColorScheme } from '@renderer/hooks/ui/useColorScheme';
import useColorScheme from '@renderer/hooks/ui/useColorScheme';
import useFontScale from '@renderer/hooks/ui/useFontScale';
import { configService } from '@/common/config/configService';
import { application } from '@/common/adapter/ipcBridge';

/**
 * Theme context value interface 主题上下文值接口
 * Separates light/dark mode from color schemes 分离明暗模式和配色方案
 */
export interface ThemeContextValue {
  // Resolved light/dark mode, already applied to the DOM. Components that only
  // need to style for the current scheme read this and are unaffected by the
  // 'system' preference. 明暗模式（已解析并应用到 DOM）
  theme: Theme;

  // The user's chosen preference — may be 'system'. Only the theme switcher
  // needs this (to know which of the three options to highlight). 用户偏好（可为 system）
  themePreference: ThemePreference;
  setThemePreference: (preference: ThemePreference) => Promise<void>;

  // Color scheme 配色方案
  colorScheme: ColorScheme;
  setColorScheme: (scheme: ColorScheme) => Promise<void>;

  // Font scaling 字体缩放
  fontScale: number;
  setFontScale: (scale: number) => Promise<void>;
}

export const ThemeContext = createContext<ThemeContextValue | null>(null);

/**
 * Theme provider component 主题提供者组件
 * Manages both light/dark mode and color schemes 同时管理明暗模式和配色方案
 */
export const ThemeProvider: React.FC<PropsWithChildren> = ({ children }) => {
  const [theme, themePreference, setThemePreference] = useTheme();
  const [colorScheme, setColorScheme] = useColorScheme();
  const [fontScale, setFontScale] = useFontScale();

  // Restore OS-level keep-awake on boot (defaults to ON when unset).
  useEffect(() => {
    (async () => {
      try {
        await configService.whenReady();
        const enabled = configService.get('system.keepAwake') ?? true;
        await application.applyKeepAwake.invoke({ enabled });
      } catch {
        /* 非桌面环境无此 command,忽略 */
      }
    })();
  }, []);

  return (
    <ThemeContext.Provider
      value={{ theme, themePreference, setThemePreference, colorScheme, setColorScheme, fontScale, setFontScale }}
    >
      {children}
    </ThemeContext.Provider>
  );
};

/**
 * Hook to access theme context 访问主题上下文的 Hook
 * @throws {Error} If used outside of ThemeProvider 如果在 ThemeProvider 外使用会抛出错误
 */
export const useThemeContext = () => {
  const context = useContext(ThemeContext);
  if (!context) {
    throw new Error('useThemeContext must be used within ThemeProvider');
  }
  return context;
};
