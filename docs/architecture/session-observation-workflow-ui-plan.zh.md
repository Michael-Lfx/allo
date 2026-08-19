# Session Logs — 执行计划（U0–U5）

> **文档状态：实施稿；U0–U5 已在 `feat/session-observation` 落地，现行语义以本文 + 源码为准**  
> 日期：2026-08-19  
> 修订：enqueue_order 合并写盘（禁止 control-first persist）、Delete tombstone vs Clear/Reset generation、16MiB 预算默认 128KiB×128、`recorder_health` 在 list 顶层、quota 不删 active segment、Call 410 `observation_retention`、`turn/end` 零等待、控制队满与 `try_enqueue` 对齐、§1 改为已落地/收口缺口。  
> 分支：`feat/session-observation` 已合并；后续 follow-up 在 `feat/session-observation-followup`  
> **作废：** 只做 Drawer 卡片的稿；功能打通但不写 IO 的稿；把执行失败写成 `integrity=degraded` 的稿；**control 优先消费 / 永久 tombstone 用于 Clear/Reset / 64KiB×256 神圣 / health 塞进 Session Summary / `turn/end` 等 50ms /「control 满时保证不丢 turn/end」。**  
> **读者：** U0–U5 与 §9.1 已完成。合并后的现行语义以本文 + 源码 + [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md) 为准。  
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

**现状：** `MAX_PREVIEW_CHARS=2000` 限制每个字符串。`llm_request_to_value` 在入队前即 stub `tools[].input_schema`（`omitted_reason=event_size_limit`，不 clone / 不 `to_vec` 测字节），并对 system / messages 预截断（inline media 不拷贝、不哈希）。`capture_and_size_cap` 仍是 128 KiB backstop。没有真实线上 P50/P95。

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
10. 不做 replay / OTel / 导出 Sink。  
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
| P11 | 磁盘：14 天 **且** `max_total_observation_bytes`（默认 **1 GiB**）。先删 age>14d 的非 active segment，再按 mtime 删最老非 active；**禁止删当前 writer 打开的 `events.jsonl`。** |

---

## 4. 目标信息架构与数据流

```text
聊天页 [观测] pill（最左能力按钮，aria-pressed）
  → 对话列滑动 pane（translateX；侧栏/标题栏保持可见）
  左列顶：用户回合 / 模型 / 工具 / 有效时长 + 刷新 + 最新在上|最早在上
  写入器 health（进程活状态）与会话 integrity / coverage 次级
  Turn Navigator（窗口 ≤200；行上时钟；第 N 轮按时间升序编号）
  Selected Turn：虚拟化 Call headers → 点瓦片才 GET 单 call 正文 → 可收缩 object tree
```

```text
Agent ──capture+size cap──► enqueue_order
        ├ normal (128 × 128 KiB)
        └ control (64)     独立容量；满时丢 normal
                 ▼
        Writer 取两队首更小 enqueue_order，再分配 event_seq
        WriterCommand: Event | Flush | Delete | Clear | ResetAll | Shutdown
                 ▼
        JSONL + rotate + age/quota GC（不删 active segment）
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

1. `turn/start.prompt_preview`：conversation bind 时写入 **`current_send.content`**（本轮用户原文，truncated+redacted）。同一 `root_turn_id` 只发一次 start；agent 在 knowledge/skill 注入后的二次 bind **不得覆盖**。  
2. 否则该 turn `event_seq` 最大的 `llm/request`，**从 messages 尾向前**最近 `role=user` 且含 Text，排除纯 ToolResult  
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
```

鉴权不变。Call GET 同样 developer mode + 会话归属。

Call GET 在 header 还在、segment 已 GC：返回 **410**，body `reason=observation_retention`。仅当 **turn 已 `has_turn_end` 且 call payload 全空**；turn 仍在跑时的空 body 回 **200**，不得当成 retention。在 `routes_trace.rs` 映射即可，**不要**为调试 API 去扩公共 `AppError`（现无 Gone 变体）。UI：「此调用详情已被观测保留策略清理」，不是「加载失败」。

### 6.3 磁盘 quota

**所有 observation 文件 mutation 只在 writer 线程**（rotate、age GC、quota GC、delete）。Query 只读。

```text
Retention = age 14d AND total ≤ 1 GiB
1. 先删 age>14d 的非 active segment
2. 再算总量；仍 >1 GiB 则按 mtime 删最老非 active segment，直到 ≤quota
禁止删当前 writer 打开的 events.jsonl
```

---

## 7. UI

- 顶栏「观测」pill 切换对话列 view（`aria-pressed`）；无列内 tab。Esc / 再点「观测」回到对话。无新侧栏路由。无 `document.body` 全屏 overlay。无日志顶栏 X。  
- 左列顶是标注四数（数字 + 短标签）与图标排序/刷新；释义合一到工具条左 Info Popover。写入器 health 仅异常时强调；顶栏不出完整/降级字，降级只在回合行 flags。  
- 回合行：第 N 轮（按 `started_at_ms` 升序编号，与显示倒正向无关）、预览、时钟、模型/工具次数、时长。默认最新在上。  
- Call 行 `useVirtualizer`，overscan 3–5。宽屏横轴可横滚。窄对话列（`@container` ~720px，按列宽不是 window）左列改横向回合条，检查器单列。  
- 点瓦片才 GET call detail，不自动展开第一张；关详情 unmount。瓦片 `aria-expanded`。工具瓦片 title 用 `argument_preview`，无则 name。`最终回复` 是不可点文案终点，不是瓦片。  
- Call 检查器：对象/数组用 `react-json-view-lite`（根与 `messages`/`tools` 数组展开，元素默认收起）；`{` / `}` 必须能开合（punctuation 转发到 expander，禁止自写树）。短 string 与响应 reasoning/content 直接展示；omitted 原样。复制用图标，禁止每块「复制 JSON」文案。详情字段是 hairline Raised 面板，不是灰底 dump。  
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

---

## 8. Token（U5）

与前版同：provider 正规化后才有 `input_uncached`。U3 不抢跑。

---

## 9. 阶段

**U0–U5 已完成**（`feat/session-observation` 本地 commit）。下面保留验收清单，不再当实施顺序。

### U0 — Writer + 正确性

双队列 `enqueue_order` 合并、128KiB×128 字节封顶、WriterCommand、Delete tombstone / Clear generation / ResetAll / Shutdown ACK、Health 顶层、投影 status/integrity 分离、Call 与 turn/end 解耦、时间字段、`Instant` 前移、consumer-drop、工厂重置登记（已有 diagnostics Retire）+ ResetAll 衔接。

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

虚拟化 headers + **Call GET 懒正文** + 可收缩 object tree（`react-json-view-lite`）+ 合法 token 字段。点瓦片才拉正文，不默认双 JSON。

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
| 1 GiB quota 砍历史 | UI `coverage` 说明 |
| Call 多一次 RTT | 点开才发生 |

---

## 13. 实施纪律

先读 `recorder.rs` `emit`/`remove_conversation`、`project.rs`、`observation.rs` `stream_llm`、`openai.rs` usage、`routes_trace.rs`、conversation `drop_conversation_observations`。  
只改当前 U 文件。Git 人类作者，无 AI trailer。只推 `feat/session-observation`。

---

## 14. 短 Review（只审钉死项）

1. `failed + complete` 表示执行失败、日志完整 —— 同意 / 不同意  
2. Call 终态不依赖 `turn/end` —— 同意 / 不同意  
3. `WriterCommand` + Delete tombstone / Clear generation / ResetAll / Shutdown ACK，防 JSONL 复活 —— 同意 / 不同意  
4. `128 KiB × 128` 普通队列（16 MiB 预算）+ 超限 `event_size_limit` 不降级 integrity —— 同意 / 不同意  
5. 双队列按 `enqueue_order` 合并写盘；control 只保容量、禁止 control-first persist；`turn/end` 零等待 —— 同意 / 不同意  
6. U3 默认 Call 级懒加载，不整 Turn 下发正文 —— 同意 / 不同意  
7. `RecorderHealth` 在 list **顶层**（不进 Session Summary）—— 同意 / 不同意  
8. 14 天 **且** 1 GiB quota；不删 active segment；Call 410 `observation_retention`；Summary `coverage` —— 同意 / 不同意  
9. legacy 不 poll；new-format 退避 poll —— 同意 / 不同意  
10. 读路径 Semaphore(4) + 前端 abort —— 同意 / 不同意  

---

## 15. 授权

U0–U5 与 §9.1 已落地。合并后 [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md) 已补 Workspace、status≠integrity、writer 命令、Call 懒加载、health/coverage。
