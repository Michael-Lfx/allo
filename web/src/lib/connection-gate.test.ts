import { describe, expect, it } from "vitest";

import { shouldShowConnectionGate, type ConnectionGateFacts } from "./connection-gate";

/**
 * 连接门的判定边界。
 *
 * 这扇门是阻断式的（没有任何关闭出口），所以「什么时候**不**该弹」比「什么时候弹」
 * 更容易出错，值得逐条钉住：
 * - 自动连接在途不弹（`bootstrapped === false`），否则每次刷新都先闪一下（那一段由
 *   `LoadingOverlay` 表示）；
 * - 离线就弹，**包括掉线**——曾经连上过也一样，那时主页面同样取不到任何数据；
 * - 连接中也弹：门自己就有进度表达，删掉顶部横幅之后它也是重连唯一的进度指示；
 * - 连上即撤，这是唯一的出口。
 */

const base: ConnectionGateFacts = {
  bootstrapped: true,
  phase: "offline",
};

describe("shouldShowConnectionGate", () => {
  it("waits for the first auto-connect to settle", () => {
    expect(shouldShowConnectionGate({ ...base, bootstrapped: false })).toBe(false);
    // Settled and offline: that is exactly when the gate belongs on screen.
    expect(shouldShowConnectionGate(base)).toBe(true);
  });

  it("shows whenever the app is offline — a dropped link included", () => {
    // 掉线与首连失败在这里没有区别：都取不到会话，都该看到配置。
    expect(shouldShowConnectionGate(base)).toBe(true);
    expect(shouldShowConnectionGate({ bootstrapped: true, phase: "offline" })).toBe(true);
  });

  it("stays on screen while connecting, so a reconnect always shows progress", () => {
    // 删掉顶部横幅后，重连没有别的进度指示了——这一段必须由门承担。
    expect(shouldShowConnectionGate({ ...base, phase: "connecting" })).toBe(true);
  });

  it("is dismissed by a successful connection and nothing else", () => {
    expect(shouldShowConnectionGate({ ...base, phase: "online" })).toBe(false);
    // 没有任何「已跳过」状态可以绕过它。
    expect(shouldShowConnectionGate(base)).toBe(true);
    expect(shouldShowConnectionGate({ ...base, phase: "connecting" })).toBe(true);
  });
});
