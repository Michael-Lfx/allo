import { useThemeContext } from '@renderer/hooks/context/ThemeContext';
import { canvasThemes } from '@oc/lib/canvas-theme';
import '@oc/styles/quiet-chrome.css';

export function useQuietChromeTheme() {
  const { theme } = useThemeContext();
  return canvasThemes[theme === 'dark' ? 'dark' : 'light'];
}
