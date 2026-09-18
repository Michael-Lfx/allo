# Session Logs — 执行计划（U0–U5）

> **最后维护：** 2026-09-02（增量加入详情 timeline、请求渐进式展示、单回合 JSON 导出、三栏工作区与窄轨收起优化；核对基准为当前实现）
> **文档状态：实施稿；U0–U5 已合入 `main`；配额 GC 与请求扫描列表随 PR #119 落地。现行语义以本文 + 源码 + [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md) 为准**  
> 日期：2026-08-19  
> 修订：enqueue_order 合并写盘（禁止 control-first persist）、Delete tombstone vs Clear/Reset generation、16MiB 预算默认 128KiB×128、`recorder_health` 在 list 顶层、quota 不删 active segment、Call 410 `observation_retention`、`turn/end` 零等待、控制队满与 `try_enqueue` 对齐、§1 改为已落地/收口缺口；U3 请求 `messages`/`tools` 默认扫描列表。  
> 分支：现行语义以源码 + [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md) 为准；历史实施分支 `feat/session-observation` 仅作考古  
> **作废：** 只做 Drawer 卡片的稿；功能打通但不写 IO 的稿；把执行失败写成 `integrity=degraded` 的稿；**control 优先消费 / 永久 tombstone 用于 Clear/Reset / 64KiB×256 神圣 / health 塞进 Session Summary / `turn/end` 等 50ms /「control 满时保证不丢 turn/end」。**  
> **读者：** U0–U5、§9.1 与 PR #119 收口已完成。合并后的现行语义以本文 + 源码 + [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md) 为准。  
> **词汇**仍以 [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md) 与提案第 7 节为准。本文补产品、投影与 writer 语义，不另起第三套领域。

| | 是什么 |
| --- | --- |
| 图 1 | 当前对话列滑动 pane |
| 图 2 | DSH Workflow 信息架构（对齐呈现，不装插件） |
| S0–S8 | **保留** JSONL / capture / `event_seq` / canonical request / 工具生命周期 / debug API / developer mode |

不再改总体架构：磁盘 JSONL SSOT、bounded queue、writer 线程、`spawn_blocking`、streaming summary、有界 UI 缓存、Call 虚拟化。不上子进程、mmap、观测 SQLite。

---

## 0. 已钉死的 7 个实现语义（授权前不再开放）

对照源码后的执行答案，不是待议项。

### 0.1 `ExecutionStatus` 与 `ObservationIntegrity` 分开

**同意分开。** 完整记下的工具失败是观测成功、执行失败。

```text
ExecutionStatus:  running | completed | failed | cancelled | interrupted | truncated
ObservationIntegrity: complete | degraded
```

`status` = Agent **做了什么**。  
`integrity` = **日志缺不缺**。

`integrity=degraded` **仅**当：

- `observation/gap`（含 `writer_queue_overflow`）
- JSONL 解析损坏
- `llm/request` 无 `llm/response` **且** 该 turn 已 `turn/end`（或等价正式结束）
- `tool/execution_started` 无终态 **且** 该 turn 已正式结束
- writer/storage 失败（见 0.7；此时 UI 还看 RecorderHealth）

因此：`tool/execution_started` + `tool/execution_failed` 都在 → `status=failed`，`integrity=complete`。  
Workspace「降级」只表示观测不可信，失败用红色 `failed`，两套词不要混。

现有 `Integrity` 枚举保留；投影赋值改成上面这条，不要把 failed/cancelled 本身标 degraded。

### 0.2 Model Call `completed` 与 `turn/end` 解耦

**同意解耦。**

| 对象 | 谁决定终态 |
| --- | --- |
| ModelCall.status | 自己的 request / response / **该 call 的** tools |
| Turn.status | `turn/start` / `turn/end` |
| Turn.integrity | 观测完整性（0.1） |

Call #1 的工具已 completed 后，即使 #3 还在跑、尚无 `turn/end`，#1 必须显示 **completed**。  
`turn/end` 只回答「这个用户回合整体是否结束」。

### 0.3 异步 Writer 下的删除 / 清空 / 重置 / shutdown barrier

**已落地。** Writer 异步后，Delete/Clear/Reset 必须 ACK，否则 queue 里未写事件会把目录写活。

**永久 tombstone 只用于 Delete。** Clear / Reset 保留同一 `conversation_id`，必须 bump Recorder 内部 `observation_generation: u64`（**不进 JSONL schema**）：pending 旧 generation 丢弃；ACK 后新 send 用新 generation 照常入队。否则同一会话清空后观测永远停写。

对照 `ConversationService`：

| 操作 | 保留同一 `conversation_id` | Writer 语义 |
| --- | --- | --- |
| Delete | **否** | 永久 tombstone + ACK |
| Reset | **是** | `ClearConversation` + generation++ + ACK |
| 清上下文 `clear_context` | **是** | generation bump + ACK |
| 清空消息 `clear_messages` | **是** | generation bump + ACK |
| Factory reset | 全局 | `ResetAll` ACK（writer 仍活着时） |

Writer 只消费 `WriterCommand`：

```text
Event { … }                 // 普通或 control 观测事件（携带入队时 generation）
Flush
DeleteConversation(id)      // ACK；永久 tombstone
ClearConversation(id)       // ACK；generation++，不是永久拒绝
ResetAll                    // ACK；factory reset / 清整个 observation 根
Shutdown                    // drain → flush → 退出线程
```

`DeleteConversation`：

1. 先丢弃该 id 的 pending Event，再 flush/drop writer，再删目录，再清 seq/lost
2. 删除生效后，该 id 的后续 Event **拒绝入队**（进程内 tombstone）
3. 管理路径 **等待 ACK**（Delete / Clear / Reset / Shutdown 不是 Agent 热路径）

`ClearConversation`：丢掉该 id 的 pending 旧 generation Event → 删目录（或等价清空 JSONL）→ generation++ → 清该 id 的 seq/lost → ACK。之后同 id 新 Event **必须能写**。

`ResetAll`：停收普通 Event → drain 或丢弃全部 pending Event → flush → 删 `diagnostics/observation/` → 清全部 seq/lost/generation/tombstone → ACK。  
进程内 factory-reset 相关清理走这条；**下次启动**的目录 Retire 仍由现有 `dataset_roots` `diagnostics` 处理，但若 writer 还活着必须先 `ResetAll`/`Shutdown`。

`Shutdown`：drain → flush → join。进 U0 DoD。

### 0.4 队列内存预算（不变式是 16 MiB，不是 64 KiB 神圣）

**现状：** `MAX_PREVIEW_CHARS=2000` 限制每个字符串。`llm_request_to_value` 在入队前即 stub `tools[].input_schema`（`omitted_reason=input_schema_elided`，不 clone / 不 `to_vec` 测字节），并对 system / messages 预截断（inline media 不拷贝、不哈希；`byte_length` 为源字符串字节数）。`capture_and_size_cap` 仍是 128 KiB backstop。没有真实线上 P50/P95。

**冻结：**

```text
NORMAL_QUEUE_MEMORY_BUDGET = 16 MiB     // 真正的不变式
MAX_EVENT_BYTES            = 128 KiB    // 开工默认；若夹具 P95 远小于 64 KiB 再改回 64×256
MAX_QUEUE_EVENTS           = 128        // 普通队列
MAX_CONTROL_EVENTS         = 64
最坏积压 ≈ 128 × 128 KiB = 16 MiB
```

流程：`capture` → 序列化试算 → `> MAX_EVENT_BYTES` 则按从大到小省略 `request.tools` / `request.messages` / `request.system`（或对称的 response/tool 大字段），写入 `omitted_reason=event_size_limit` 与 `original_bytes` / `captured_bytes`，直到 ≤ 上限或变成 stub。然后再 `try_send`。

**超限 omit 是 capture policy，不是完整性失败。** `capture=truncated` + `omitted_reason=event_size_limit`。**不得**因此 `integrity=degraded`。UI 必须能看见 omitted，不能显示成「观测未记录 / 加载失败」。

不要做复杂的 byte-aware channel。用「硬 cap 单 event × 事件数」锁死 RAM。

### 0.5 双队列保序：enqueue_order 合并，禁止 control-first persist

**属实：无条件先写 control 会让 `turn/end` 插到已入队的 tool/response 前面，`event_seq` 会撒谎。**

冻结：

- 入队时 `enqueue_order = AtomicU64::fetch_add(1)`（进程内，**不进 JSONL schema**）。
- 两个 `VecDeque<(u64, WriterCommand)>`：`normal` 与 `control`。
- Writer **看两个队首，取更小 `enqueue_order` 的那条** 写盘，**然后再**分配 `event_seq`。
- Control 的特权只是：**独立容量 + 满时先丢恰好一条 normal 再入队**；**禁止**无条件先写 control。不循环清空整个 normal 队。

```text
normal:  llm/request, llm/response, tool/*
control Event: turn/start, turn/end, observation/gap
lifecycle（不受 MAX_CONTROL_EVENTS=64 约束，始终入队）:
  Flush / DeleteConversation / ClearConversation / ResetAll / Shutdown
```

与 `DualQueue::try_enqueue` 对齐：

- 普通满：`lost++`，立即返回，不阻塞 Agent。
- control Event 满：先丢 **恰好一条** normal（`lost++`）再入队；若 control 仍满 → `DroppedControl`（含 `turn/end`），`RecorderHealth.queue_dropped`。
- **`turn/end` 禁止等 50ms。** Agent 产生的观测事件永不回压。
- Delete / Clear / Reset / Shutdown 才 ACK 等待。
- 若 `turn/end` 最终没进去：`RecorderHealth.queue_dropped` 作顶栏警告；成功 persist 后可恢复 `healthy`。Workspace **不**因 `queue_dropped` 停 poll（见 7.1）。

`event_seq` = writer 持久化全序，不是业务 happen-before。

### 0.6 大 Turn：Rendering lazy ≠ Data lazy

**按 capture 数学，整 Turn 拉 500 份 canonical request 会进 HTTP/JS 堆（约数十 MB 量级）。** 不把「先测再决定」拖过 U3。

**U3 默认 API：**

```text
GET /turns/{root_turn_id}
  → turn metadata + model_calls[] **headers only**
    (id, call_kind, scope, status, integrity, times, usage 标量, tool 名/状态/耗时)
  → 无 system/messages/tools schema/response 正文

GET /turns/{root_turn_id}/calls/{model_call_id}
  → 该 call 的 request / response / tool payloads
```

点卡片才 fetch 后者。虚拟化解决 DOM；这条解决数据内存。

U1 用夹具量 50/100 call 的 **header 包** 与 **单 call 包** 字节数，写入测试断言（防止 header 里误带正文）。不把「500 call 全量 ProjectedTurn」当 U3 默认。

### 0.7 Writer 无法写 JSONL 时的真相出口

磁盘满 / 权限 / writer panic 时，**不能**再靠 JSONL 里的 gap。

进程内（重启可丢）：

```text
RecorderHealth { healthy | queue_dropped | storage_error | writer_disconnected, last_error }
```

**`recorder_health` 移出 Session Summary，放在 list 顶层。** 进程活状态 ≠ 该会话历史完整性。

```json
{ "recorder_health": { "status": "healthy", "last_error": null }, "summary": { ... }, "turns": [] }
```

Workspace 分两行：当前写入器 / 当前会话日志。不是第二套 SSOT，只是 recorder 活着与否。

---

## 1. 现状（已对源码，2026-08-19）

### 已落地（不要再当 U0 待做）

| 项 | 落点 |
| --- | --- |
| `enqueue_order` 合并写盘，再分配 `event_seq` | `recorder.rs` `DualQueue` / `persist_event` |
| Delete tombstone + ACK；reset / clear_context / clear_messages = generation++ | `service.rs` ~6034 / ~6349 / ~12741 / ~12844 |
| Factory `ResetAll` + `diagnostics` Retire | `nomifun-system` + `dataset_roots.rs` |
| 128KiB × 128（条数×上限锁 16 MiB，不做 byte-aware channel） | `capture.rs` / `recorder.rs` |
| `recorder_health` 在 list 顶层；Call 410 `observation_retention` | `hub.rs` / `routes_trace.rs` |
| `turn/start` 在 conversation bind 后；同一 `root_turn_id` 的 `turn/end` 在 send loop 退出时写一次 | `service.rs` bind + `close_observation_turn_from_relay` |
| preview：`current_send.content` 首次 bind 胜出，agent 二次 bind 不覆盖 | `ObservationSession::emit_turn_start_once` |
| Workspace 观测 pill、左列计数、虚拟化、懒 Call GET、LRU=2、omitted、原始 token 芯片 | `AgentTraceInspector/` |

### 本轮已收口

Refresh / poll 共用 `refreshWorkspace`（独立 seq + abort）；选中 turn 跳转清展开与 detail；Refresh/poll 必 GET call（LRU 只服务卡片点击）；`queue_dropped` 只警告不停 poll；**真正写盘**成功才从 `queue_dropped`/`storage_error` 恢复（tombstone / generation skip 不恢复；overflow gap 失败归还 `lost_count`）；idle-kill **仅非 defer** 直写 `turn/end`，defer 由 host close 收口并沿用 stash 的 engine elapsed/usage；`TurnTerminationGuard` drop 仍绕过 defer；`turn/start`/`turn/end` first-write-wins 在 interned recorder；Call 410 仅 turn 已结束后的空 payload，UI 删 LRU 键；切换会话清空 list/detail/health；选中 turn 变化时 refresh 不重复 GET（交给 `selectedId` effect）；`seq_by_boundary` 按 `{folder}\0{boundary}` 分桶，clear/delete 丢掉该 folder。

Conversation host 在 bind 后 `set_observation_turn_end_deferred(true)`，failover / 剔图 / cron continuation **不得**提前 `turn/end`；loop 退出（含 cancel `return` 之前）`close_observation_turn_from_relay`。无 hub 的直连 `send_message` 仍立即结算。Prep 失败在未 defer 时立即 `Failed`，defer 时 stash 到 host close；distill 取消 stash `Cancelled`，成功路径在 distill 之后 stash，由 host close 写出。

### 不在本轮

- 读路径 `flush_blocking`（不改文件结构）

---

## 2. 冻结约束

1. 方案 B；不装插件；不抄 Cordis 类型。  
2. Canonical ≠ wire。  
3. 禁止气泡 / SQLite 补 REQUEST。  
4. 显式 `ObservationSession`。  
5. **查询排序**只认 `event_seq`。`event_seq` = **writer 接受并持久化的全序**，不是纳秒级 happen-before。  
6. 禁止无限定 `Run`。  
7. 开发者模式只门控 HTTP 读取与支持包；采集始终写盘。  
8. 观测/队列/磁盘失败不 `?` 打断 Agent。  
9. 不扩大采集（one_shot / health / speech / ACP）。  
10. 不做 replay / OTel / 导出 Sink；用户导出仅提供单个 `root_turn_id` 的已保留 JSONL 事件文档。
11. 虚拟列表用已有 `@tanstack/react-virtual`（或 `react-virtuoso`），不手写。Call 检查器允许 `react-json-view-lite`；禁止自写树，禁止把树用在 List/Turn。  
12. 不上子进程 / mmap / 观测 SQLite / 无界 channel。

产品面：Developer Mode 下顶栏最左能力按钮是「观测」pill（`aria-pressed`）；点开后对话列滑动到会话日志。Esc 与再点观测回到对话。无列内 tab，无日志顶栏 X，无 Arco Switch。不展示 `msg=` / `turn=` / `mc-xxxx`；不发明 `input_uncached`。切回对话不清 LRU、不 abort poll。

---

## 3. 性能不变式

| # | 不变式 |
| --- | --- |
| P1 | 热路径无 blocking write/flush/GC。 |
| P2 | bounded 双队列；`enqueue_order` 合并写盘；普通 overflow 不阻塞；control 见 0.5。 |
| P3 | JSONL SSOT；UI cache 可丢。 |
| P4 | 读/投影 `spawn_blocking`；Hub **Semaphore(4)** 限制并发重扫描。 |
| P5 | Summary streaming fold，禁止全量 `Vec`+`project_turns` 只为四数。 |
| P6 | List / Turn GET 不带 canonical 大正文；正文只在 Call GET。 |
| P7 | Model Call 行虚拟化。 |
| P8 | 同时 ≤2–3 个 **call detail**；换会话 `clear()`。切回对话不清 LRU、不 abort poll。不接 `videoCanvas` QueryClient。 |
| P9 | 禁止 mmap / 全历史 preload。 |
| P10 | 单 event ≤ `MAX_EVENT_BYTES` 才入队。 |
| P11 | 磁盘：高低水位（高 **1 GiB**，低 **800 MiB**）。写盘队列空闲 ≥30s 且距上次扫描 ≥1h 才扫盘；估算 ≥**1.2 GiB** 时下一个空档立刻收。writer 第一次队列空档只 reconcile 估算（计入盘上存量），不删文件；**禁止在 Event persist 热路径扫盘。** 按 mtime 删最老非 active 直到 ≤低水位；**禁止删当前 writer 打开的 `events.jsonl`。** |

---

## 4. 目标信息架构与数据流

```text
聊天页 [观测] pill（最左能力按钮，aria-pressed）
  → 对话列滑动 pane（translateX；侧栏/标题栏保持可见）
  左列顶：用户回合 / 模型 / 工具 / 有效时长 + 刷新 + 最新在上|最早在上
  写入器 health（进程活状态）与会话 integrity / coverage 次级
  Turn Navigator（窗口 ≤200；行上时钟；第 N 轮按时间升序编号）
  Selected Turn：虚拟化 Call headers → 点瓦片才 GET 单 call 正文 → 请求 messages/tools 默认扫描列表，原始才是 object tree
```

```text
Agent ──capture+size cap──► enqueue_order
        ├ normal (128 × 128 KiB)
        └ control (64)     独立容量；满时丢 normal
                 ▼
        Writer 取两队首更小 enqueue_order，再分配 event_seq
        WriterCommand: Event | Flush | Delete | Clear | ResetAll | Shutdown
                 ▼
        JSONL + rotate + quota GC（不删 active segment；无按天 TTL）
                 ▼
        spawn_blocking + semaphore
        streaming summary | turn headers | lazy call detail
                 ▼
        有界 UI 缓存 + TanStack Virtual
```

---

## 5. 事件与投影

### 5.1 `turn/start` / `turn/end`

不升 schema major。

- `turn/start`：bind 后；同一 `root_turn_id` 一条；preview = 本次 send 文本（truncated+redacted）。  
- `turn/end`：**用户回合结算**（同一 `root_turn_id` 一条，first-write-wins 在 interned recorder）。Conversation loop 退出时写；不是每个 failover/continuation 的 `send_message` 结算，也不是第一条 `llm/response`。含 `status, elapsed_ms, stop_reason, aggregate_usage?`。非 defer 的 idle-kill 直写 `Cancelled`；defer 时 idle-kill 不写 end，由 host close 用 stash 结算。`TurnTerminationGuard` drop 仍绕过 defer 直写 `Cancelled`。

### 5.2 时间

Call/tool 墙钟 = 信封 `ended_at_ms - started_at_ms`。  
`Instant` 移到 `provider.stream().await` 前。此前不把旧 `ttft_ms` 标成真 TTFT。

Turn 主耗时 = `turn/end.elapsed_ms`。  
Summary `active_duration_ms = Σ turn.elapsed_ms`。`wall_span_ms` 次要。禁止用隔夜 wall 当「总耗时」。

### 5.3 状态（分层）

**ModelCall.status**（与 turn/end 无关）：

| 状态 | 条件 |
| --- | --- |
| failed | `response.error` / `stop_reason==error` **或** 该 call 任一 tool failed |
| cancelled | 该 call 任一 tool cancelled |
| interrupted | 有 request 无 response **且** 所属 turn 已结束 |
| running | 有 request 无 response，或工具 started 无终态，且 turn **未** end |
| truncated | `stop_reason==max_tokens`（警告） |
| completed | 有 response、非 failed/cancelled、该 call 工具均终态 |

**Turn.status：** 有 `turn/end` 用其 status；仅有 start 无 end → `running`；legacy 无 start 也无 end → `unknown`（不 poll）。

**integrity：** 只按 0.1。运行中缺 response **不是** degraded。

### 5.4 preview

1. `turn/start.prompt_preview`：conversation bind 时写入 **`current_send.content`**（本轮用户原文，truncated+redacted）。同一 `root_turn_id` 只发一次 start；agent 在 knowledge/skill 注入后的二次 bind **不得覆盖**。投影发现旧数据把首个 `[Context]` 注入块写成 preview 时，隐藏该 marker，并回退到请求中的真实用户文本。
2. 否则该 turn `event_seq` 最大的 `llm/request`，**从 messages 尾向前**最近 `role=user` 且含 Text，排除纯 ToolResult，并跳过该 user 消息首个 `[Context]` 文本块
3. 否则「观测未记录」  

禁止首条 user，禁止气泡。

### 5.5 Summary

```text
turn_count, model_call_count, tool_count
active_duration_ms, wall_span_ms?
integrity, coverage, max_event_seq  // coverage=retained_observation_history
```

`recorder_health` **不在** summary 内，见 0.7。  
GC/quota 后「32 回合」≠ SQLite 全会话。UI 写明「基于当前保留的观测日志」。

---

## 6. Writer / 读 / API

### 6.1 Writer

热路径：capture → 字节封顶 → `try_send`（Agent 事件永不回压；`turn/end` 零等待）。  
writer：按 `enqueue_order` 出队 → 分配 `event_seq` → BufWriter → rotate 48 MiB → flush（batch / 50–100ms / Shutdown）→ 不每条 fsync → overflow 后写 gap → GC（离开热路径，且只在 writer 线程）。  
生产路径不使用 `emit` 的同步 `ObservationEvent` 返回值。

### 6.2 读

Hub：`spawn_blocking` + `Semaphore(4)`。  
前端：换 `selectedId` **abort** 上一个 detail；`A.seq < current` 丢弃乱序响应；Summary 不并发重复算（single-flight）。  
List / Turn GET：metadata。Call GET：正文。

```text
GET /api/debug/session-observations?conversation_id=
  { recorder_health, summary, turns[] }   // headers only，含 max_event_seq

GET .../turns/{root_turn_id}?conversation_id=
  { turn metadata, model_calls[] headers }

GET .../turns/{root_turn_id}/calls/{model_call_id}?conversation_id=
  { request, response, tools }

GET .../turns/{root_turn_id}/export?conversation_id=
  { export_version, schema_version, status, integrity, coverage,
    has_turn_end, turn, events[] }       // 单回合完整保留事件，按 event_seq 升序
```

鉴权不变。Call GET 同样 developer mode + 会话归属。

Turn GET 额外返回轻量 `timeline[]`，以及每个模型调用的
`request_message_view` / `system_prompt_state`。List GET 仍清除 timeline 和正文，仅保留回合与调用 headers。

Export GET 同样要求登录、会话归属与 Developer Mode；回合仍在执行时返回已写盘事件并标记 `status=running`、`has_turn_end=false`。找到 gap、截断或损坏记录时仍返回 JSON，但 `integrity=degraded` 或事件中的 gap 必须保留。找不到 retention 范围内的回合返回 404。

Call GET 在 header 还在、segment 已 GC：返回 **410**，body `reason=observation_retention`。仅当 **turn 已 `has_turn_end` 且 call payload 全空**；turn 仍在跑时的空 body 回 **200**，不得当成 retention。在 `routes_trace.rs` 映射即可，**不要**为调试 API 去扩公共 `AppError`（现无 Gone 变体）。UI：「此调用详情已被观测保留策略清理」，不是「加载失败」。

### 6.3 磁盘 quota

**所有 observation 文件 mutation 只在 writer 线程**（rotate、quota GC、delete）。Query 只读。

```text
Retention = high/low watermark (1 GiB / 800 MiB)
Estimate: first writer queue-empty gap reconciles on-disk size (includes leftover files after restart). Persist path does not walk.
Trigger: writer persist-idle ≥30s and last scan ≥1h; or estimated total ≥1.2 GiB on the next writer idle gap
按 mtime 删最老非 active segment，直到 ≤800 MiB
禁止删当前 writer 打开的 events.jsonl
禁止在 Event persist 热路径扫盘
```

---

## 7. UI

- 顶栏「观测」pill 切换对话列 view（`aria-pressed`）；无列内 tab。Esc / 再点「观测」回到对话。无新侧栏路由。无 `document.body` 全屏 overlay。无日志顶栏 X。  
- 左列顶是标注四数（数字 + 短标签）与图标排序/刷新；释义合一到工具条左 Info Popover。写入器 health 仅异常时强调；顶栏不出完整/降级字，降级只在回合行 flags。  
- 回合行：第 N 轮（按 `started_at_ms` 升序编号，与显示倒正向无关）、预览、时钟、模型/工具次数、时长。默认最新在上。  
- Call 行 `useVirtualizer`，overscan 3–5。宽屏横轴可横滚。窄对话列（`@container` ~720px，按列宽不是 window）左列改横向回合条，检查器单列。  
- 点时间线事件或请求/响应/工具阶段按钮才 GET call detail，不自动展开第一张；关详情 unmount。时间线按 `turn/start → llm/request → llm/response → tool lifecycle → next llm/request → ... → turn/end` 展示，每项带 `+relative`，模型响应区分工具请求/最终回答，工具名、状态和耗时单独呈现；事件、模型调用卡片与阶段按钮双向高亮。工具瓦片 title 用 `argument_preview`，无则 name。
- Call 检查器：对象/数组用 `react-json-view-lite`（根与 `messages`/`tools` 数组展开，元素默认收起）；`{` / `}` 必须能开合（punctuation 转发到 expander，禁止自写树）。请求消息按服务器给出的 `request_message_view` 默认展示当前尾部，历史公共前缀收起在顶部；无法可靠区分时展示完整上下文。`messages` / `tools` 默认扫描投影（role / 块类型 / 开头摘要，或工具名 + 描述），工具条「原始」才切回对象树；复制仍是 canonical JSON。消息扫描按原始顺序展示当前请求；工具定义默认收起，实际调用工具优先。`[Context]` 是保留前缀，仅识别 user 消息的第一个文本块，不参与主预览；真实用户文本优先显示。Context 仅通过 hover tooltip 显示 `上下文 · Current date: ...`，不增加摘要行；Context-only 消息保留且不额外生成摘要。整段 `{ omitted_reason }` 显示已省略，`[]` 显示没有消息/没有工具，缺字段显示观测未记录；行上有预览时仍带 omitted 标记。短 string 与响应 reasoning/content 直接展示。系统提示首次默认展开，后续未变化默认收起并显示沿用，变化/不可比较显式标记。放大弹层与 320px 面板各自渲染，禁止共用同一个 React 节点。复制用图标，禁止每块「复制 JSON」文案。详情字段是 hairline Raised 面板，不是灰底 dump。
- 当前回合标题栏右侧提供「下载本轮 JSON」；进行中显示「下载当前记录」，下载中防重复，成功通知服务端文件名，失败留在当前回合区域提示。导出不受 UI 折叠影响；retention/gap 语义显式显示。
- 缓存：summary + ≤200 turn headers + `MAX_CALL_DETAIL_CACHE=2` LRU。换会话 `clear()`。切回对话保持 poll 与 LRU。  
- Token 芯片：U3 只显示原始 `input_tokens` / cache_read / cache_write / output。不画未命中，不发明 `input_uncached`。  
- omitted 字段必须可见，不得显示成「观测未记录 / 加载失败」。不展示 `msg=` / `turn=` / `mc-xxxx`。  

### 7.1 新鲜度

```text
refreshWorkspace = health+summary+list + 当前 turn headers + 若已展开则当前 call
Refresh 按钮与 poll tick 走同一条路径
```

- list / turn / call **各自** seq；禁止共用一个计数器。  
- 每次 `refreshWorkspace` 带 `AbortController`；换会话 / effect cleanup 时 abort。切回对话不 abort。  
- 410 → `retentionRemoved`，不得进空 `catch` 当成加载失败。  
- poll effect 只依赖「是否还应 poll」布尔，**不要**依赖整个 `entries` 数组。

Poll **仅** `has turn/start && !turn/end` 的 new-format turn。  
**legacy（无 turn/start）不自动 poll。**

退避：1.5s → 3s → 5s → 最大 10s；`max_event_seq` 变化则回到 1.5s。  
`turn/end` 或 health 已 `storage_error`/`writer_disconnected` → 停 poll。`queue_dropped` 只作顶栏警告，不停 poll。

### 7.2 三栏 Trace 工作区（现行 UI 行为）

> **现行状态：** 本节描述当前实现的 Trace 工作区行为。三栏布局、时间线收起、当前事件详情和窄屏退化均已落地；本次只改变前端布局与展示投影，不改变 JSONL、投影 DTO、导出接口、Call 懒加载或 retention 语义。

#### 7.2.1 目标与空间契约

物理场景：用户在明暗不同的桌面环境中长时间审查 Agent 执行过程，需要同时保持因果方向、当前事件和正文证据的空间连续性。三栏布局服务于定位与阅读，不把三个区域做成等宽卡片网格。

~~~text
回合列表               时间线导航栏                  当前事件详情
Round navigator  →     Timeline rail          →      Event inspector
~~~

- 应用级侧栏、对话标题栏、观测入口和回合列表语义保持不变。
- 宽屏（可用容器宽度 ≥1200px）：回合列表约 260px，时间线展开约 288px，收起约 88px，详情占用剩余空间。
- 不使用三列等宽，不给时间线分配与详情相同的正文空间。时间线是定位 rail，详情是主要阅读 surface。
- 回合标题、状态和下载动作固定在详情区顶部；时间线拥有自己的小标题与收起按钮。
- 使用 Trace 工作区的 container query 判断列宽，不只依赖 viewport。应用侧栏、窄窗口或面板折叠后，仍需重新计算三栏是否可用。

回合列表、时间线和详情工作区均设置 min-width: 0、min-height: 0，避免长文本撑破父级布局。详情标题固定，模型调用列表拥有自己的滚动容器，选中的调用在其卡片下方承载详情。选择时间线事件只定位对应调用卡片，不滚动整个页面；轮询更新不得重置时间线或详情滚动位置。展开的时间线本身承担当前回合全览，不再额外渲染重复的回合全览面板。

#### 7.2.2 时间线密度与收起

时间线应保持“低噪声、可定位、可恢复”的密度：

- 事件行建议 44–60px，只保留标题、相对时间、状态和必要的调用/工具标识。
- 间隔 0s 不单独占行。只有超过可感知阈值的等待才显示为连接线上的轻量文字，不能伪装成业务事件。
- tool/execution_started 与终态事件默认合并为一个可读的“工具执行”行，显示工具名、状态和总耗时；展开或进入详情时仍能查看开始、完成、失败、取消等原始生命周期。不得修改导出中的事件或 event_seq。
- 模型请求和模型响应继续分开。响应必须明确标识“请求调用工具”或“最终回答”，不能用卡片排列方向推断因果。
- 回合开始、回合结束和 gap 使用更轻量的节点样式；辅助调用保留，但通过标签弱化，不默认抢占主视觉。
- 事件序号是诊断辅助信息，放在次级位置。相对时间、状态和耗时优先于内部 ID。

时间线支持两级状态：

1. **展开 rail：** 显示事件标题、相对时间、状态和工具名。
2. **桌面窄 rail：** 900–1199px 时收起为约 88px 的固定轨道，只显示事件图标和相对时间；完整语义通过展开状态、无障碍名称和左侧图标说明查看，不能出现没有语义的空白可点击行；图标固定为正圆。
3. **顶部折叠区：** 899px 以下时间线移动到详情顶部，收起态使用约 88px 高的横向事件条，事件按钮只显示图标和相对时间，不把圆形节点压缩成椭圆。

收起按钮必须提供 aria-expanded 与 aria-controls。收起或展开不得丢失选中的 InspectTarget、时间线滚动位置和键盘焦点。折叠状态 v1 只保存在当前工作区，不新增 localStorage 或其他持久化状态。移动端可以进一步把 rail 变成“时间线”折叠区或切换项，但不能让用户失去当前回合上下文。

#### 7.2.3 详情区的当前事件优先

点击模型请求、响应或工具事件后，上下文条与 inspector 紧贴在对应模型调用卡片下方：

~~~text
回合 3 / 模型调用 #2 / 模型响应 · 请求调用工具
+12.4s · 用时 3.2s · 首 token 420ms
~~~

- 模型调用列表不额外渲染重复的分组标题；模型调用信息卡片与详情处于同一滚动流；每个模型调用只保留紧凑 header、请求/响应/工具阶段入口，选中的模型调用才在自身卡片下方嵌入完整 inspector。响应详情只保留一个 inspector，不再追加重复的“最终回复”卡片。
- 选中详情使用固定高度范围的 slot，建议 `height: clamp(320px, 42vh, 520px)`，正文和 JSON 树在 slot 内部滚动。加载中、失败、retention 和正常内容共用同一边界，不因正文长度推动其他模型调用卡片。
- 点击 request、response、tool 卡片时，必须反向高亮时间线对应事件；工具生命周期合并显示时高亮对应事件范围。
- 选择新事件只让对应模型调用卡片进入可见范围；切换阶段时保持调用卡片位置。轮询只能更新内容，不能强制改变用户当前阅读位置。
- 未选择事件时保留模型调用摘要和“选择时间线事件查看详情”提示；回合全览由展开的时间线承担。
- 回合开始、结束和 gap 没有模型调用归属时，使用独立的固定高度事件详情槽，并在上下文条中显示事件序号和来源。
- 请求消息继续使用服务端 request_message_view；系统提示、历史上下文、实际工具和工具定义的渐进式展示规则保持不变。
- 下载按钮仍属于当前回合标题区域，不移动到全局导航，也不因时间线收起而消失。点击后由桌面原生保存对话框或浏览器文件保存选择器让用户选择目标位置，再写入 JSON；不使用浏览器默认下载目录。用户取消选择时保持空闲状态，不显示下载成功。

#### 7.2.4 响应式退化

按 Trace 容器宽度而非单一设备型号退化：

- ≥1200px：完整三栏，详情列为主阅读区；时间线展开约 288px，收起约 88px。
- 900–1199px：保留回合列表与详情主列，时间线使用约 88px 的固定窄 rail，默认允许收起。
- 768–899px：回合列表沿用现有窄列行为，时间线变成详情顶部约 88px 高的横向可收起区域。
- <768px：回合列表沿用横向窄列，时间线与详情纵向排列；时间线使用横向滚动事件条，详情占满宽度，不强行压缩三栏。
- 200% 缩放时，按钮文本不得互相覆盖，时间线事件可以换行，详情正文保持可读宽度。
- 360px 下不依赖 hover 提示来理解选中状态；工具名、状态和耗时至少有文本或图标辅助。

#### 7.2.5 交互、可访问性与性能验收

- 键盘 Tab 顺序为回合列表 → 时间线收起按钮 → 时间线事件 → 详情操作；Enter/Space 可选择事件。
- 当前事件同时使用边界、文本和 aria-pressed 表达，不能只依赖紫色高亮。
- 时间线新增事件提示使用 aria-live="polite"，用户不在底部时不得自动滚动。
- Reduced Motion 关闭 rail、详情和 chevron 的位移动画，只保留即时状态变化。
- 时间线继续只消费轻量 metadata，不把 request、response、工具参数复制到每个节点。
- 选择事件只触发必要的 Call GET；收起时间线不触发网络请求；导出仍始终读取完整保留事件。
- 空回合、无工具、单工具、多工具、失败、取消、运行中、gap、retention、长消息和大量工具定义都必须能在详情首屏理解当前状态。

#### 7.2.6 当前实现边界

1. Trace 工作区由回合列表、时间线导航栏和详情工作区组成；模型调用卡片与当前详情属于同一右侧滚动流，非 Call 事件使用固定高度事件详情槽。
2. 时间线使用 event_seq 投影，工具开始与终态只做视觉合并；详情和导出仍可追溯原始事件。
3. 模型调用卡片只保留紧凑阶段入口；选中卡片在自身下方渲染一个固定高度详情 slot，详情内容不再脱离来源卡片。
4. 容器宽度低于 900px 时默认收起时间线为约 88px 紧凑轨道；900–1199px 仍保持三栏横向布局。收起态只显示图标和相对时间；899px 以下改为约 88px 高的横向时间线顶部区域，收起状态不写入 localStorage。
5. 后续改动须继续通过键盘、200% 缩放、Reduced Motion、双主题和真实长日志验收。

---

## 8. Token（U5）

与前版同：provider 正规化后才有 `input_uncached`。U3 不抢跑。

---

## 9. 阶段

**U0–U5 已完成**（合入 `main`；后续配额 GC 与扫描列表见 PR #119）。下面保留验收清单，不再当实施顺序。

### U0 — Writer + 正确性

双队列 `enqueue_order` 合并、普通事件 128KiB×128 字节封顶、控制事件有界且溢出可观测、WriterCommand、Delete tombstone / Clear generation / ResetAll / Shutdown ACK、Health 顶层、投影 status/integrity 分离、Call 与 turn/end 解耦、时间字段、`Instant` 前移、consumer-drop、工厂重置登记（已有 diagnostics Retire）+ ResetAll 衔接。

**DoD：** `cargo test -p nomi-agent-trace`

- 工具失败 → call `failed` + integrity `complete`  
- 运行中无 response → call `running`，integrity `complete`  
- turn 已 end 仍无 response → `interrupted` + `degraded`  
- 先 enqueue 再 DeleteConversation：目录不复活  
- Clear 后同 id 新事件能写  
- Shutdown join  
- 超 `MAX_EVENT_BYTES` 带 `event_size_limit` 仍能入队，integrity 仍 complete  
- 先入 normal 再入 `turn/end`：持久化顺序不得 end 在前  
- control Event 满：先丢恰好一条 normal；仍满则 `DroppedControl`（可含 `turn/end`）+ `queue_dropped`；lifecycle 始终入队；`turn/end` 零等待  
- **阻塞 sink：** writer 卡住时 `emit`/try_send 不等待 latch（**不要**用 p95 磁盘 CI）  
- 热路径不 `flush`；quota GC 不删 active segment

### U1 — Turn 边界 + streaming summary + 读隔离

`turn/start`/`end`；preview 尾部 user；`{recorder_health,summary,turns}`；spawn_blocking+semaphore；quota GC（不删 active）；Call GET 410 `observation_retention`。

**DoD：** Summary accumulator 最终结构只有标量；巨大 payload fixture 的 list **不**返回该正文。测 50/100 call 的 header JSON 字节（断言无 `input_schema`）。`cargo check -p nomifun-conversation`。

### U2 — Workspace

对话列滑动 pane（`translateX`，约 240ms / `--ease-out-expo`；`prefers-reduced-motion` 改为 120ms 透明度）。入口是顶栏最左「观测」pill，无列内 tab。侧栏和标题栏保持可见。左列顶：用户回合 / 模型调用 / 工具调用 / 有效时长（`2.5s` / `1m5s`）+ 刷新 + 最新在上|最早在上；写入器 health 仅异常时强调；integrity / coverage 次级。Navigator：第 N 轮 + 用户预览 + 时钟 + 模型/工具次数。无新侧栏路由。切回对话不 abort poll。

### U3 — Call 工作流

虚拟化 headers + **Call GET 懒正文** + 请求 `messages`/`tools` 默认扫描列表（「原始」才是 `react-json-view-lite`）+ 合法 token 字段。点瓦片才拉正文，不默认双 JSON。

### U4 — Fresh度

Refresh 双/三拉；new-format 才 poll；legacy 不 poll；退避；abort 旧请求。

### U5 — Token 正规化

观测层 `NormalizedObservationUsage` 拷贝原始字段，不改公共 `TokenUsage`。`input_uncached` 仍等 provider 正规化。

### 9.1 收口（已落地）

`AgentTraceInspector/index.tsx`：`refreshWorkspace` 共用 Refresh/poll；独立 `listSeq` / `turnSeq` / `callSeq` + abort；`max_event_seq` 变化把 poll 退回 1.5s，未变化才 `finally` 加退避。

---

## 10. 测试（可执行，少 flaky）

| 要证明 | 怎么测（CI） |
| --- | --- |
| 热路径不等盘 | writer 阻塞 latch，producer 仍返回 |
| overflow | 普通事件 lost+gap；control 满先丢一条 normal，仍满才 DroppedControl |
| Delete barrier | pending event 不能重建已删目录 |
| Summary 不持正文 | accumulator 类型/夹具，不是 RSS 断言 |
| Header 无 schema | serde 快照 / 禁止字段 |
| 虚拟化 | 结构测试存在 virtualizer；500 行 fixture 不要求真测 DOM 数（可测 window API） |

RSS / 切 30 会话 / 1 GiB quota 扫盘：本地清单，非默认 CI 门禁。

---

## 11. 落点

| 区域 | 路径 | U |
| --- | --- | --- |
| 队列/命令/health/size/quota/generation | `nomi-agent-trace` `recorder.rs` | 0 |
| status/integrity 投影 | `project.rs` | 0 |
| Instant、非阻塞 emit | `nomi-agent` `observation.rs` | 0 |
| start/end 发射 | conversation bind + TurnCompleted | 1 |
| Delete ACK | Hub + `ConversationService::drop_conversation_observations` | 0 |
| Hub 读 + 新 Call 路由 | `hub.rs` `routes_trace.rs` | 1–3 |
| Workspace | `AgentTraceInspector/` | 2–4 |
| Token | `nomi-providers` | 5 |

建议提交：

```text
fix(agent): isolate observation writer and split status from integrity
feat(agent): emit turn boundaries and stream observation summaries
feat(ui): add session logs workspace with lazy call payloads
fix(ui): poll new-format turns until turn/end
fix(providers): make token usage buckets unambiguous
```

---

## 12. 风险

| 风险 | 处理 |
| --- | --- |
| 删会话等 ACK 略慢 | 可接受（管理路径） |
| 崩溃丢未 flush 尾 | 诊断数据，接受 |
| 旧日志无 start/end | `unknown`，不 poll |
| 1 GiB 高低水位回收较旧日志 | UI 注意事项说明收录范围 |
| Call 多一次 RTT | 点开才发生 |

---

## 13. 实施纪律

先读 `recorder.rs` `emit`/`remove_conversation`、`project.rs`、`observation.rs` `stream_llm`、`openai.rs` usage、`routes_trace.rs`、conversation `drop_conversation_observations`。  
只改当前任务文件。Git 人类作者，无 AI trailer。历史 U 步骤曾只推 `feat/session-observation`；现行文档与源码以 `main` 为准。

---

## 14. 短 Review（只审钉死项）

1. `failed + complete` 表示执行失败、日志完整 —— 同意 / 不同意  
2. Call 终态不依赖 `turn/end` —— 同意 / 不同意  
3. `WriterCommand` + Delete tombstone / Clear generation / ResetAll / Shutdown ACK，防 JSONL 复活 —— 同意 / 不同意  
4. `128 KiB × 128` 普通队列（16 MiB 预算）+ 超限 `event_size_limit` 不降级 integrity —— 同意 / 不同意  
5. 双队列按 `enqueue_order` 合并写盘；control 只保容量、禁止 control-first persist；`turn/end` 零等待 —— 同意 / 不同意  
6. U3 默认 Call 级懒加载，不整 Turn 下发正文 —— 同意 / 不同意  
7. `RecorderHealth` 在 list **顶层**（不进 Session Summary）—— 同意 / 不同意  
8. 1 GiB/800 MiB 高低水位（紧急 1.2 GiB）；不删 active segment；Call 410 `observation_retention`；注意事项说明收录范围 —— 同意 / 不同意  
9. legacy 不 poll；new-format 退避 poll —— 同意 / 不同意  
10. 读路径 Semaphore(4) + 前端 abort —— 同意 / 不同意  

---

## 15. 授权

U0–U5 与 §9.1 已落地。合并后 [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md) 已补 Workspace、status≠integrity、writer 命令、Call 懒加载、health/coverage。
