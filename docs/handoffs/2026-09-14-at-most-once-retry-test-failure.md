# 交接：at-most-once 重试测试确定性失败（2026-09-14）

- 基线：`772890fed`（`feat/agent-store-v1`），工作树含未提交的 agent-store 图标与连接门改动
- 性质：既有缺陷的记录与证据固化，**本轮未修复**
- 影响：`cargo test -p nomifun-app --lib` = **314 passed / 1 failed**；F7（at-most-once 投递）承诺的"连接建立失败可重试"分支实际不生效

## 失败对象

```
crates/backend/nomifun-app/src/commands/stdio_common.rs:1100
commands::stdio_common::tests::at_most_once_retries_undelivered_connection_failures
```

来自提交 `c75a2b71f`（2026-07-29，`fix(app): at-most-once delivery for forwarded browser tool calls`）。

被断言的行为（`stdio_common.rs:516-572`）：浏览器工具调用（`click` / `type` /
`press_key`）是不可逆副作用且无幂等键，因此定 `ToolDeliveryPolicy::AtMostOnce`
——**只重试"请求证明没离开本进程"的失败**，一旦可能已送达就绝不重发：

```rust
let undelivered = error.is_connect();            // stdio_common.rs:560
if matches!(delivery, ToolDeliveryPolicy::AtMostOnce) && !undelivered {
    return Err(format!(
        "{error}; the request may already have been executed and was not retried"
    ));                                          // stdio_common.rs:562-566
}
```

测试构造：先 `server.abort()`（此后必然是 connection refused），再在 400ms 后
于同一端口重启服务；重试计划是 `0 / 250 / 750 / 1500 ms`，因此期望前两次被拒
（`is_connect() == true` → 允许重试）、第三次成功：

```rust
assert_eq!(result, ForwardToolOutcome::Success("ok".into()));   // stdio_common.rs:1147 ← 挂在这里
assert_eq!(state.tool_count.load(Ordering::SeqCst), 1);
```

## 实际结果

```
panicked at stdio_common.rs:1147:9:
  left: Error("Error: tool transport failed: error sending request for url
        (http://127.0.0.1:<port>/tool); the request may already have been
        executed and was not retried")
  right: Success("ok")
```

那条消息**全仓只有一个出处**——上面 `!undelivered` 分支。故结论明确：

> **`error.is_connect()` 对一次 connection refused 返回了 `false`。**

独立佐证：失败耗时 **0.01–0.03s**。重试延迟为 `0/250/750/1500ms`，只要发生过
**任何一次**重试，耗时必然 ≥250ms；0.02s 说明**第一次尝试就直接放弃**（loopback
上 ECONNREFUSED 立即返回，不是在等 5s `connect_timeout`）。即"连接失败自动重试"
一次都没重试。

## 证据

| 检查 | 命令 / 方式 | 结果 |
| --- | --- | --- |
| 是否 flaky | 单独跑 3 次 | **否**，3/3 失败，0.01–0.03s |
| 是否本轮引入 | 干净 worktree（pristine HEAD）+ 仅 2 行最小编译修复，不含任何图标功能 | **否**，同样失败 |
| 是否仍存在 | 当前 HEAD `772890fed` | **是**，仍失败 |
| 同族测试 | `cargo test -p nomifun-app --lib stdio_common` | 14 passed / 1 failed |
| "不重发"那半边 | `at_most_once_never_reposts_after_possible_delivery` | **通过**（保守方向是好的） |

风险方向是**保守**的：不重试意味着绝不重复执行，不会重复点击。坏处是瞬断即报错、
不能自愈——F7 文档里"connection-setup failures … are retried"的描述与实际不符。

## 机制（已确认的部分）

调用链：`reqwest::Error::is_connect()` → 遍历 `source()` 找
`hyper_util::client::legacy::Error` → 其 `is_connect()` 即
`matches!(self.kind, ErrorKind::Connect)`（`hyper-util 0.1.20`
`src/client/legacy/client.rs:1643`）。

`ErrorKind::Connect` 确实会被赋值，例如
`connector.connect(...).map_err(|src| e!(Connect, src))`（同文件 517 行），
**因此按这条链它本该工作**。

依赖现状与测试诞生时（`c75a2b71f`）的差异：

| crate | 测试写下时 | 现在 |
| --- | --- | --- |
| hyper | `1.10.1` | `1.11.0` |
| hyper-util | `0.1.20` | `0.1.20` |
| reqwest（nomifun-app 解析到） | `0.12.28` | `0.12.28` |

`Cargo.lock` 自该提交后被改动 **317 次**。

## 尚未确认（不要当成结论）

**根因未坐实。** 是 reqwest 的 `downcast_ref` 链断了、还是 hyper 1.11 改了
connect 错误的包装/分类方式，**没有证据**。上面"依赖漂移"只是时间线上的可疑点，
不是已证明的因果。

要坐实，最便宜的路子是加一个临时单测：向一个已关闭的 loopback 端口发一次 POST，
打印 `err.is_connect()` 与 error 的完整 `source()` 链，逐层看在哪一环断掉。
代价是重编 `nomifun-app` 测试目标（约 3 分钟）。

## 建议的修复方向（未实施）

若坐实 `is_connect()` 不可靠，可把判定从"依赖上游分类"改为"显式分类"：

- 拿得到 `ErrorKind::Connect` 时照旧；
- 拿不到时退化为检查 `source()` 链上是否出现 `std::io::Error` 且
  `kind() == ConnectionRefused` / `ConnectionAborted` / `NotFound`（DNS）等
  **连接建立阶段**的错误码；
- 保持保守默认：**无法确定"未送达"时一律不重试**（现状行为），只把明确的
  connection refused 从"不重试"移回"可重试"。

注意不要为了让它变绿而放宽判据——那会牺牲 F7 的防重复执行保证。

## 复现

```bash
cargo test -p nomifun-app --lib at_most_once_retries_undelivered_connection_failures
```

## 相关发现（同一轮调查，已修）

工作区 `cargo check --workspace --all-targets` 在 HEAD 上曾失败，共 **7 处**
`E0063: missing field avatar_url`，根因是 `cc2185ec3` 给
`AppServerSkillSummary` / `AppServerConnectorSummary` 加了字段但未同步初始化式：

| 位置 | 归属 | 状态 |
| --- | --- | --- |
| `nomifun-app-server/src/catalog.rs:862, 876` | 测试辅助（`sample_skill` / `sample_connector`） | 已修（`avatar_url: None`） |
| `nomifun-app-server/src/lib.rs:8833, 8847, 9467` | 测试辅助 + `TempSkillCatalog` 测试替身 | 已修 |
| `nomifun-app/src/app_server_catalog.rs:62, 186`（HEAD 行号） | 生产投影函数 | 由未提交的图标工作覆盖 |

修后：`cargo check --workspace --all-targets` 通过、
`cargo test -p nomifun-app-server --lib` = **123 passed**。

另注：这类"初始化式缺字段"只在编 test cfg 时暴露，`cargo check --lib` 是绿的——
排查时不要把它误判为缓存假错。
