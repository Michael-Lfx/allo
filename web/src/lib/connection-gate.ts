/**
 * 连接门的显示判定。
 *
 * 规则只有一条：**只要还没连上 App Server，就把连接配置挡在前面**——离线是，连接中也
 * 是。
 *
 * 为什么连接中也留着：连接门自己就有进度表达（状态行「正在建立连接」+ 按钮变成
 * 「连接中…」）。撤掉它再盖一层全屏 loading，画面会闪一下，而且重连时全屏遮罩与门
 * 交替出现更乱；留着门，视线和焦点都不用重新找。
 *
 * 为什么没有「已跳过」这一态：没有 App Server 时主页面里没有任何一件事是能做的，留一个
 * 关得掉的出口等于把人放在一屏点不动的控件前面（`16` 的「不做假开关」）。
 *
 * 放在 `lib/` 而不是组件里：这是纯判定，只有类型依赖、不碰 store 也不碰 DOM，因此可以
 * 直接按输入断言边界（组件侧只负责订阅与渲染）。
 */

import type { ConnectionPhase } from "../ui/connection";

/** 判定所需的全部状态。 */
export interface ConnectionGateFacts {
  /** 首次自动连接是否已经落定（成功或失败）。 */
  bootstrapped: boolean;
  phase: ConnectionPhase;
}

/**
 * 连接门是否应当显示。
 *
 * `bootstrapped` 只为一件事存在：页面首次绘制时 `phase` 还是初始的 `offline`，而自动
 * 连接要到 mount 之后的 effect 才发起；那一段由 `LoadingOverlay` 全屏表示，没有这个
 * 判据每次刷新都会先闪一下门。
 */
export function shouldShowConnectionGate(state: ConnectionGateFacts): boolean {
  return state.bootstrapped && state.phase !== "online";
}
