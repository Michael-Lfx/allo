import type { TooltipProps } from '@arco-design/web-react';

/**
 * 侧边栏 Tooltip 挂到 document.body。
 * `.layout-sider` 有 overflow-x: hidden，收起态向右弹出的菜单名会被裁成竖条。
 * 关闭侧栏时仍靠 cleanupSiderTooltips 清掉残留节点（issue #987）。
 */
export const getSiderPopupContainer = (_node: HTMLElement): Element => document.body;

const SIDER_TOOLTIP_CLASS = 'sider-tooltip-popup';

export const cleanupSiderTooltips = () => {
  if (typeof document === 'undefined') return;
  // Arco Tooltip occasionally leaves detached popup nodes; remove both scoped and global tooltip popups.
  document.querySelectorAll(`.${SIDER_TOOLTIP_CLASS}, .arco-tooltip-popup`).forEach((node) => node.remove());
};

export type SiderTooltipProps = Pick<
  TooltipProps,
  | 'className'
  | 'trigger'
  | 'disabled'
  | 'unmountOnExit'
  | 'popupHoverStay'
  | 'popupVisible'
  | 'getPopupContainer'
  | 'triggerProps'
>;

export const getSiderTooltipProps = (enabled = false): SiderTooltipProps => {
  const disabled = !enabled;
  return {
    className: SIDER_TOOLTIP_CLASS,
    trigger: (disabled ? [] : 'hover') as 'hover' | 'hover'[],
    disabled,
    unmountOnExit: true,
    popupHoverStay: false,
    // Arco treats `popupVisible` as controlled whenever the key exists, even if
    // the value is `undefined`. Only pass it when we must keep the popup closed.
    ...(disabled ? { popupVisible: false } : {}),
    getPopupContainer: getSiderPopupContainer,
    triggerProps: {
      mouseEnterDelay: 0,
      mouseLeaveDelay: 0,
    },
  };
};
