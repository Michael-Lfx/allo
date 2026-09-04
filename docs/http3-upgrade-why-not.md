# 为什么客户端当前无法升级到 HTTP/3

> 日期：2026-09-04 ｜ 关联：[HTTP/3 客户端探索与评测报告](http3-quic-exploration-report.md)

## 结论先行

**可以且应该升级的是服务端（已完成，服务端 HTTP/3 属实可用）；客户端暂不能跟进。**
不是服务端不支持，也不是网络不通，而是 **客户端所用的 HTTP/3 栈（reqwest 0.12
的 `http3` 实验特性）目前存在未修复的 ALPN 缺陷、依赖尚未成熟、且没有平滑回退机制**。

---

## 一、 依赖成熟度：HTTP/3 栈仍是实验特性，未正式可用

| 组件 | 版本/状态 |
| --- | --- |
| reqwest `http3` feature | 实验特性，编译必须加 `RUSTFLAGS="--cfg reqwest_unstable"`（官方强制提示，不带则直接编译失败） |
| 底层 `h3` / `h3-quinn` | v0.0.8 / v0.0.10 —— 仍为 0.x 预发布阶段 |
| `quinn` (QUIC 实现) | 0.11.x，API 持续演进 |

此类实验栈进入桌面端生产环境，行为无法锁定，属于不可接受的工程风险。

---

## 二、 实测致命缺陷：客户端 ALPN 协商错误（HTTP/3 失败的根因）

用自研探针（[http_protocol_probe.rs](../crates/backend/nomifun-cloud/examples/http_protocol_probe.rs)）
对 `server.flowyaipc.com/claw/health` 实测：

| 客户端 | 结果 |
| --- | --- |
| 我方探针（reqwest 0.12 + http3） | **0/10 失败**：`received fatal alert: NoApplicationProtocol` |
| 服务端 Go QUIC 客户端 | **HTTP/3 200 OK** ✅ |
| 服务端支持 HTTP/3 的 curl | **HTTP/3 200 OK** ✅ |

**决定性对照**：同一探针访问 `www.google.com`（全球公认支持 HTTP/3），得到
**一模一样的 `NoApplicationProtocol`**；代理开/关交叉复测，失败表现完全一致。
结合直连环境路由、DNS、出口节点均干净，可以确定：

- ❌ 不是服务端问题（多客户端验证 200）
- ❌ 不是网络/代理问题（Google 对照 + 代理交叉验证）
- ✅ **是客户端 HTTP/3 栈自身的 ALPN 缺陷**：
  `http3_prior_knowledge()` 会把 rustls ALPN 强制为仅 `["h3"]`，但该请求实际
  落进 hyper (TCP) 链接路径 → 服务器（含 Google）对"只有 h3 的 TCP/TLS 握手"
  无法协商，即返回 `NoApplicationProtocol`。

上游官方正在修复：[reqwest PR #2929 "Specify h3 alpn for http3 connector"](https://github.com/seanmonstar/reqwest/pull/2929)
—— 该修复**尚未合入任何稳定版本**（0.12.x / 0.13.x 均不含）。

---

## 三、 没有平滑升级/回退机制（与浏览器不同）

浏览器使用 HTTP/3 的方式：首发请求仍走 HTTP/2，从响应头 `Alt-Svc: h3` 发现服务端
能力后**自动升级**；h3 连不上时自动秒级**回退** h2（Happy Eyeballs 双栈竞速）。

Rust 侧 `reqwest` 的行为则完全不同：

- 没有 `Alt-Svc` 自动升级发现；
- `http3_prior_knowledge()` 是一个**全有或全无**开关——开了则强制按 h3 处理，且
  触发上述 ALPN 缺陷；不开则完全不用 h3；
- **无按连接失败快速回退**能力。生产开启即等于把全部请求赌在 QUIC 上，环境不对就整体失败。

---

## 四、 若强行上线的风险（叠加此缺陷）

1. **失败率 100%**：当前 reqwest 栈实测对核心云 API 0/10 全失败（见上）；
2. **UDP 443 拦截环境直接不可用**：企业内网、校园网、部分运营商链路、代理软件
   （Clash/V2Ray 等多数只透传 TCP）会丢弃或限速 UDP 443，
   客户端没有 h2 回退兜底，等待超时后接口全挂；
3. **代理工具兼容性崩坏**：标准 SOCKS5/HTTP 代理不透传 QUIC/UDP，开启代理的用户
   全部连不上。

---

## 五、 当前状态与升级条件

**客户端链路现状**：HTTP/2（已启用并实测完备——多路复用、20/50 并发、降级正常），
随 PR #188 合入。

**HTTP/3 升级需要同时满足**：

1. reqwest [PR #2929](https://github.com/seanmonstar/reqwest/pull/2929) 合入，
   `http3` feature 转为稳定可用；
2. `h3` / `h3-quinn` / `quinn` 进入正式版本，行为稳定；
3. （可选但推荐）实现 Alt-Svc 探测 + 失败自动回退策略（或选择浏览器原生栈路径）。

**复测已就绪**：上游修复后，一条命令即可重新验证：
```powershell
$env:RUSTFLAGS="--cfg reqwest_unstable"
cargo run -p nomifun-cloud --example http_protocol_probe --features http3-experimental -- `
  --target https://server.flowyaipc.com/claw/health --protocol both --rounds 10 --ip 47.251.95.78
```

---

## 附录：实测数据汇总（2026-09-04）

| 环境 | HTTP/2 | HTTP/3 | 结论 |
| --- | --- | --- | --- |
| 代理开（首轮） | 10/10 · 309 ms | 0/10 `NoApplicationProtocol` | 曾疑 fake-IP/TUN |
| 代理关（直连） | 10/10 · **167 ms** | 0/10 `NoApplicationProtocol` | 排除网络因素 |
| 代理开（交叉复测） | 5/5 · 307 ms | 0/5 `NoApplicationProtocol` | 与代理无关 → 客户端栈缺陷 |
| Google 对照 | — | 0/3 `NoApplicationProtocol` | 决定性：公认 h3 服务器同样拒绝 |
