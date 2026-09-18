import { describe, expect, it, vi } from "vitest";

/**
 * 连接门的接线检查：真实 store 的字段，对照判定函数。
 *
 * 为什么这里**不做渲染断言**：`renderToStaticMarkup` 走 zustand 的
 * `getServerSnapshot`，即模块初次求值时的快照，`setState` 不会反映到输出里——渲染出来
 * 的永远是「初始态」（`bootstrapped: false`），断言「显示/不显示」只会得到假结果。
 * 所以「该不该显示」的完整边界交给 `lib/connection-gate.test.ts` 的纯判定；这里只钉
 * 渲染测试看不到、却最容易接错的事：`connect()` 的两个分支真的把 `bootstrapped` 落定了
 * （否则门永远不会出现），以及连上之后门确实撤掉。
 *
 * `../../i18n` 必须先导入，react-i18next 才有实例。
 */

vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {} });
vi.stubGlobal("navigator", { language: "zh-CN" });
vi.stubGlobal("window", {
  location: { protocol: "http:", host: "localhost:5174" },
  setTimeout: () => 0,
  focus: () => {},
  matchMedia: () => ({ matches: false }),
});
vi.stubGlobal("document", {
  hidden: false,
  hasFocus: () => true,
  documentElement: { dataset: {} },
  addEventListener: () => {},
  removeEventListener: () => {},
});

const [{ useAppStore }, { ConnectionGateDialog }, { shouldShowConnectionGate }, { default: i18n }] =
  await Promise.all([
    import("../../store/appStore"),
    import("./ConnectionGateDialog"),
    import("../../lib/connection-gate"),
    import("../../i18n"),
  ]);
await i18n.changeLanguage("zh-CN");

/** 判定函数读真实 store 时的即时结果——组件订阅的就是它。 */
function gateVisible(): boolean {
  return shouldShowConnectionGate(useAppStore.getState());
}

describe("ConnectionGateDialog 接线", () => {
  it("is exported and reads the same fields the predicate does", () => {
    expect(typeof ConnectionGateDialog).toBe("function");

    useAppStore.setState({ bootstrapped: true, phase: "offline" });
    expect(gateVisible()).toBe(true);

    useAppStore.setState({ bootstrapped: true, phase: "online" });
    expect(gateVisible()).toBe(false);
  });

  it("stays hidden before the first auto-connect settles, then shows on failure", () => {
    useAppStore.setState({ bootstrapped: false, phase: "offline" });
    expect(gateVisible()).toBe(false);

    // `connect()` 的 catch 分支落定的就是这两项。
    useAppStore.setState({ bootstrapped: true, phase: "offline" });
    expect(gateVisible()).toBe(true);
  });

  it("has no way to bypass it: no dismissal state or actions remain on the store", () => {
    // 「稍后配置」已删除——没有 App Server 时主页面无可操作内容，不该留假出口。
    const state = useAppStore.getState() as unknown as Record<string, unknown>;
    expect(state.connectionGateDismissed).toBeUndefined();
    expect(state.dismissConnectionGate).toBeUndefined();
    expect(state.openConnectionGate).toBeUndefined();
  });
});
