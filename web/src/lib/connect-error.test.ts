import { describe, expect, it } from "vitest";

import { ProtocolError, RequestTimeoutError, TransportError } from "@flowy-agent-store/protocol";

import { connectionFailureKey, connectionFailureMessage, isConnectionFailureKey } from "./connect-error";

/**
 * 连接失败文案的分类。
 *
 * 关键区分：我们自己的英文技术串要换成 i18n key（否则连接对话框里会露出一段英文，
 * 就像 `app-server connection closed before opening`），而宿主自己的报错必须原样保留
 * ——翻译权威描述只会破坏它。
 */

describe("connectionFailureKey", () => {
  it("turns our own transport failures into keys", () => {
    // 服务没起/端口不对：真实场景里最常见的那条。
    const closed = new TransportError("connect", "app-server connection closed before opening");
    expect(connectionFailureKey(closed)).toBe("connection.failConnect");
    expect(isConnectionFailureKey(connectionFailureKey(closed)!)).toBe(true);
    expect(connectionFailureMessage(closed)).toBe("connection.failConnect");

    const timedOut = new TransportError("connect", "app-server connect timed out after 8000ms", {
      retryable: true,
    });
    expect(connectionFailureKey(timedOut)).toBe("connection.failTimeout");

    expect(connectionFailureKey(new RequestTimeoutError("conversation/send", 30_000))).toBe("connection.failTimeout");
    expect(connectionFailureKey(new ProtocolError("version_mismatch", "x"))).toBe("connection.failProtocol");
  });

  it("uses the transport key for non-connect phases", () => {
    // 已经连上之后断掉：不是「地址不对」，而是链路中断。
    expect(connectionFailureKey(new TransportError("close", "app-server connection closed (code 1006)"))).toBe("connection.failTransport");
    expect(connectionFailureKey(new TransportError("send", "app-server transport is not connected"))).toBe("connection.failTransport");
  });

  it("leaves the host's own error text alone", () => {
    // 宿主的话是权威描述：必须原样保留，不能被换成 key。
    const host = new Error("config.toml is not valid TOML: expected a table");
    expect(connectionFailureKey(host)).toBeNull();
    expect(connectionFailureMessage(host)).toBe("config.toml is not valid TOML: expected a table");
    expect(isConnectionFailureKey(connectionFailureMessage(host))).toBe(false);
  });

  it("does not mistake host prose for one of our keys", () => {
    expect(isConnectionFailureKey(null)).toBe(false);
    expect(isConnectionFailureKey(undefined)).toBe(false);
    expect(isConnectionFailureKey("connection dropped by peer")).toBe(false);
    // 前缀之外不算——避免把宿主恰好以 connection 开头的话误当 key。
    expect(isConnectionFailureKey("connection.reset")).toBe(false);
  });
});
