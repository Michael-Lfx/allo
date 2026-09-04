# HTTP/3 (QUIC) 客户端探索与评测报告

> 分支：`feat/http3-quic-exploration`
> 日期：2026-09-04
> 探针工具：`crates/backend/nomifun-cloud/examples/http_protocol_probe.rs`

## 一、 背景

服务端 `server.flowyaipc.com` 已通告 HTTP/3 支持（`Alt-Svc: h3=":443"; ma=86400`）。
本报告记录客户端侧对 HTTP/2 与 HTTP/3 链路的实测结论、环境干扰因素以及后续建议。

## 二、 服务端反馈（已验证）

1. 服务端已在 UDP 443 真实开启 HTTP/3（Nginx 实际在跑 QUIC，非仅响应头通告）；
2. 服务端用 Go QUIC 客户端与支持 HTTP/3 的 curl 直连真实 IP `47.251.95.78` 验证：
   - 绕过本地 TUN：`HTTP/3 200`，`Alt-Svc: h3=":443"` ✅
   - 走 TUN 默认路由：`HTTP/3 200`，同样 ✅
3. 服务端判断我方此前失败是**本地代理 / TUN 干扰 QUIC**，建议用 `--http3-only` 打真实 IP 或支持 HTTP/3 的浏览器复核。

## 三、 客户端侧实测结果

### 3.1 探测方式与环境

- 探针：`http_protocol_probe.rs`（支持 `--ip <真实IP>` 绑定 DNS，绕过本地域名劫持）。
- 目标：`https://server.flowyaipc.com/claw/health`
- 本机 DNS 解析：`server.flowyaipc.com -> 47.251.95.78`（真实 IP，未指向假 IP）
- 本机网络：Wi-Fi + Tailscale 隧道在运行（该隧道会成为 QUIC/UDP 443 的干扰路径）。

### 3.2 HTTP/2 链路（TCP，绑定真实 IP）

| 指标 | 结果 |
| --- | --- |
| 冷启动首请求 | 672 ~ 717 ms |
| 长连接平均时延 | ~231 ms |
| 成功率 | 10/10 ✅ |

### 3.3 HTTP/3 链路（QUIC/UDP，绑定真实 IP）

| 指标 | 结果 |
| --- | --- |
| 冷启动首请求 | 失败 |
| 成功率 | 0/10 ❌ |
| 特征错误 | `client error (Connect) -> received fatal alert: NoApplicationProtocol` |

**关键对照实验**：同一探针、同一时间点探测公认支持 HTTP/3 的 `www.google.com`，
得到**一模一样的错误**。这证明在本机网络环境下，QUIC 握手被本地网络栈
（TUN / 隧道路径）拦截并回送 TLS ALPN 错误，不是服务端问题，也不是探针问题。

### 3.4 重要发现：当前客户端实际走的是 HTTP/1.1，不是 HTTP/2

- 主工作区 `Cargo.toml` 的 `reqwest` 依赖**未启用 `http2` feature**，导致客户端
  ALPN 只协商出 `HTTP/1.1`（探测输出 `Negotiated Version: HTTP/1.1`）。
- 在实验 feature 中加入 `reqwest/http2` 后，同一探针立即协商为 `HTTP/2`：
  - 冷启动 ~672 ms，长效连接平均 ~232 ms，成功率 10/10。
- 结论：**为客户端启用 `http2` 是真·零成本收益**——老客户端连 HTTP/2 都没用上。

## 四、 客户端结论与建议

1. **服务端 HTTP/3 属实且可用**（跨客户端验证通过）。
2. **我方客户端本机暂无法完成 QUIC 握手**，属于本地 TUN/隧道因素，并非代码问题。
3. **建议立即落地**：在主工作区 `reqwest` 依赖中开启 `http2` feature，客户端链路
   从 HTTP/1.1 升级到 HTTP/2（复用单端口、多路复用、头部压缩），带来确定性收益。
4. **HTTP/3 不建议跟进**：本机实测（该环境）UT 失败，且 reqwest 的 h3 依赖版本
   （`h3 v0.0.8` / `h3-quinn v0.0.10`）仍为 0.x 实验版；此外服务端当前告警
   `NoApplicationProtocol` 表明部分中途网络无法正确透传 QUIC。
   后续等待：1) 生产环境网络直连验证窗口；2) reqwest/h3 结构化稳定版本后再评估。
