import { theme as antTheme, type ThemeConfig } from 'antd';

/** Ant Design theme aligned with open-ai-canvas canvas chrome. */
export function getVideoCanvasAntTheme(dark: boolean): ThemeConfig {
  const elevated = dark ? '#141414' : '#ffffff';
  const subtle = dark ? 'rgba(255,255,255,.06)' : 'rgba(17,24,39,.04)';
  return {
    algorithm: dark ? antTheme.darkAlgorithm : antTheme.defaultAlgorithm,
    token: {
      colorPrimary: dark ? '#8f9bd6' : '#4f6ee8',
      borderRadius: 8,
      colorBgContainer: elevated,
      colorBgElevated: elevated,
      colorBorder: dark ? 'rgba(255,255,255,.12)' : '#e2e4e8',
      colorText: dark ? '#ededed' : '#111827',
      colorTextSecondary: dark ? '#a3a3a3' : '#6b7280',
    },
    components: {
      Modal: {
        contentBg: elevated,
        headerBg: 'transparent',
        footerBg: 'transparent',
      },
      Popover: {
        colorBgElevated: elevated,
      },
      Dropdown: {
        colorBgElevated: elevated,
      },
      Select: {
        optionSelectedBg: subtle,
        optionActiveBg: subtle,
      },
      Segmented: {
        trackBg: subtle,
        itemSelectedBg: elevated,
        itemSelectedColor: dark ? '#fafafa' : '#171717',
      },
      Tooltip: {
        colorBgSpotlight: dark ? '#1f1f1f' : '#111827',
      },
    },
  };
}
