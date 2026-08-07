# 会话消息链路（conversation-messages）

会话消息列表的合并、协调与编辑重发（edit-resubmit）链路的功能域文档。维护范围：
消息列表 store、`useMessageLstCache`（含 windowed 分页）、会话消息协调器（coordinator）、
SendBox 编辑/重试入口、NomiSendBox 编辑重发事务、两条 refresh 通道。

改本链路任何代码前，先读完「架构」与「坑点」；改完必跑「验证门禁」。

## 0. V5.2 当前交接（2026-08-07）

V5.2 已落地在分支 `fix/conversation-error-edit`，目标是把编辑重提交的结果判定从
「前端最新消息窗口 + HTTP 结果」提升为「同一个 idempotency key 的后端 receipt + 精确
消息身份观察」。本轮保留 V5/V5.1 的 epoch、barrier、snapshot、consumer ack 和
`loadOlder` fence，不重写分页协调器。

已完成：

- 后端新增只读 `GET /api/conversations/{conversation_id}/messages/{message_id}/edit-resubmit/state`，
  使用同一 `Idempotency-Key` 和 `public-edit-resubmit:v1` 命名空间，返回
  `missing | accepted | completed`、delivery、replacement candidate、精确
  `target_exists/replacement_exists`；观察失败在前端保持 `unknown`，不会当作目标消失。
- 后端新增 `created_at DESC, message_id DESC LIMIT 1` 的 latest-user text 查询，替换生产
  预检和测试辅助路径的 latest-50 扫描；claim/truncate 事务和数据库围栏未改写、无迁移。
- 前端主编辑提交和 retry 都由一次 `uuidv7` 生成 operation/key；`editingMessageStore`
  明确记录 `editing | submitting | confirming`，confirming 阶段只允许同 key 的观察/幂等
  replay，禁止新的 destructive edit，并提供只唤醒当前同 key 观察/重放的「继续确认」入口。
- `editResubmitRecovery.ts` 只在明确的 pre-admission 拒绝 + receipt missing + target 仍在时
  revoke；timeout、network、普通 HTTP、观察失败、accepted、目标消失或状态不一致均不 revoke。
  transcript mutation 确认后统一走 reconciliation；post-truncate 失败清除编辑目标/徽章，
  保留当前正文和附件草稿，且不二次 destructive edit。

当前未完成的不是自动化实现，而是人工验收：需要真实长会话、目标不在最新 200 条、truncate
后错误、同 key replay/in-flight replay、post-truncate 草稿/附件保留和 barrier 最终退休。
若下一 agent 继续，先运行第 5 节的命令并检查 `git status`，不要 reset、清理或覆盖已有脏改动。

## 1. 文件地图

| 文件 | 职责 |
|---|---|
| `ui/src/renderer/pages/conversation/Messages/conversationMessageCoordinator.ts` | 协调器（纯 TS 无 React）：epoch / barrier / snapshot 三件套，模块级单例按 conversationId 分片 |
| `ui/src/renderer/pages/conversation/Messages/hooks.ts` | 消息列表 store + `useMessageLstCache`（loadMessages/loadOlder/refresh 监听/consumer 注册） |
| `ui/src/renderer/pages/conversation/Messages/messageRowKeys.ts` | 行身份：`getFetchedMergeKey`（`${type}:${msg_id}`，tool_call 用 `${turnId}:${callId}`） |
| `ui/src/renderer/pages/conversation/platforms/nomi/NomiSendBox.tsx` | 编辑重发事务（capture → arm → invoke → exact observation → reconciliation） |
| `ui/src/renderer/pages/conversation/platforms/nomi/editResubmitRecovery.ts` | receipt/精确身份恢复纯函数：safe_failure / pending / truncated / success / post_mutation_failure / unknown |
| `ui/src/renderer/components/chat/SendBox/index.tsx` | 编辑模式/retry 入口、operation mutex、inputRevision、编辑态时序（C2） |
| `ui/src/renderer/components/chat/SendBox/editResubmitOutcome.ts` | 编辑提交结果的纯决策函数（stale/clearInput/restoreSubmittedInput/exitEditMode） |
| `ui/src/renderer/components/chat/SendBox/editResubmitTypes.ts` | SendBox 与 Nomi 之间的 typed success/post-mutation failure 边界 |
| `ui/src/renderer/pages/conversation/Messages/editingMessageStore.ts` | 「编辑中/重发中」徽章 store（useSyncExternalStore，ownerId 守卫，按 conversationId 分片） |
| `ui/src/renderer/utils/file/messageFiles.ts` | 附件集合差 `removeSubmittedAttachments`（稳定 id = 路径） |
| `ui/src/renderer/pages/conversation/SessionList/hooks/useConversationListSync.ts` | 侧栏列表同步（`chat.history.refresh` 唯一监听者） |
| `ui/src/common/adapter/ipcBridge.ts` | `conversation.editResubmit`（30s）与 `conversation.editResubmitState`（5s exact observation） |

后端对应物：`crates/backend/nomifun-conversation/src/routes.rs` 暴露 edit-resubmit POST
和 state GET；`service.rs` 做用户/会话/receipt namespace/target payload 校验；
`nomifun-db` 提供 exact message lookup 和 latest-user query。原有 `edit_and_resubmit` 仍在
202 返回前完成 rewind + `delete_messages_from` 物理截断（目标用户消息 + 之后所有行，含
持久化报错 tips）；receipt 表按 idempotency key 去重。

## 2. 架构

### 2.1 为什么需要协调器

后端截断提交前有数百 ms~数秒窗口。期间任何「读在截断前、合并在移除后」的陈旧 fetch
都会把旧后缀以 streamingOnly 行**永久复活**（合并是无条件并集，streamingOnly 行永不因
合并消失）。在飞/将飞的 refetch 触发源：`conversation.turn.settled` 轮询、`turnCompleted`、
`reconnected`、edit-resubmit 自己的 reconcile refresh。变体 B：合并置换本地行 id
（`withFetchedCanonicalIdentity`）导致按本地 id 移除落空。

### 2.2 三件套（coordinator）

- **epoch**：会话级计数器，**仅** `beginEditResubmitReconciliation` 成功路径 +1。fetch 发起时
  捕获、返回时比较，不等即整体丢弃——挡「跨越 edit 成功的陈旧 fetch」。普通 fetch 不动
  epoch（consumer 之间互不误杀）。`maybeDestroy` 在无消费者且无 armed barrier 时销毁
  coordinator，epoch 归零。
- **barrier**（按 operationId）：`armed`（invoke 前建立）→ `reconciling`（202 后翻转：原子
  bump epoch + 快照 consumers 入 pendingConsumers）→ 全部 ack 后删除。armed 只过滤 fetched
  行、**不动当前列表**（旧后缀成功前仍合法）；reconciling 过滤 + purge 当前列表。一次最新
  权威 fetch 可同时 ack 多个 reconciling barrier（`appliedEpoch >= successEpoch`）。
- **snapshot**：`captureReconciliationSnapshot(conversationId)` 在 fetch **接受时**（epoch 校验
  通过后、updater 入队前）冻结规则为不可变对象；`applyFetchedMessages(list, fetched, snapshot)`
  是纯函数（filter → merge → 按 snapshot.purge 清理）。React updater 延迟执行期间 barrier
  可能已被其他 consumer 的同步 ack 删除——updater 体内**绝不查 live coordinator**。

行身份三集合：`mergeKeys`（跨实例收敛的承诺）、`serverIds`（durable message_id）、
`localIds`（无 msg_id 的 stream-only 行，仅捕获实例可匹配，不承诺跨实例）。

### 2.3 调用链

```
SendBox 编辑分支 (components/chat/SendBox/index.tsx sendMessageHandler)
  → admission: isLoading / activeEditOperationRef / activeRetryOperationRef 三重守卫
  → NomiSendBox.handleEditResubmit (platforms/nomi/NomiSendBox.tsx)
      captureBarrier（目标定位失败 → fail closed 不发请求）
      → armBarrier → editResubmit.invoke（同一个 uuidv7 operationId/key）
      → 首次响应或任意错误都进入 editResubmitState.invoke（5s timeout）
          → exact receipt namespace + target/replacement message ID observation
          → resolveEditResubmitRecovery（纯函数）
          → unknown / claimed_pending：保留 barrier，按同 key replay，不生成新 key
          → transcript_truncated / success：确认 mutation 后统一 reconciliation
          → post_mutation_failure：reconcile 后清 edit target/badge，保留正文和附件草稿
          → safe_failure：仅 definitive pre-admission 400 + receipt missing + target 仍在时
             revokeBarrier + failed refresh；网络/timeout/普通 HTTP/观察失败绝不直接 revoke
      → reconcileConfirmedEditMutation
          → beginEditResubmitReconciliation（undefined = invariant 破坏 → typed failure）
          → purgeCurrentRows（live 版）→ 乐观气泡（仅 fresh delivery）→ refresh 两条通道
          → replacement 权威成功后才 removeSubmittedAttachments；飞行中新增附件保留
  → useMessageLstCache.loadMessages (Messages/hooks.ts)
      捕获 epoch → invoke → epoch 校验 → captureReconciliationSnapshot
      → update(updater)（updater 内 applyFetchedMessages 用冻结 snapshot）
      → 同步 ackConsumerReconciled（先于 updater 执行，安全因为规则已冻结）
  → loadOlder（windowed 分页）：capturedEpoch fencing，与 loadMessages 同构
```

### 2.4 refresh 通道契约（防漂移测试钉死）

- `chat.history.refresh` → **仅**侧栏会话列表；唯一监听者
  `SessionList/hooks/useConversationListSync.ts`。
- `conversation.messages.refresh` → **仅**消息转录；唯一监听者 `Messages/hooks.ts`
  （`useMessageLstCache`，每个挂载实例一个）。
- 新增监听者会让 fetch 翻倍、epoch/ack 推理失效。`refreshChannelDrift.structure.test.ts`
  全量扫 renderer 源码拦截漂移；确需新增时先更新该测试并重新论证 ack 语义。

### 2.5 SendBox 侧守卫模型

- 编辑态时序（C2）：`setEditingMsgId(null)`/`setInput('')` 只在 `.then`（后端接受后）；
  所有 UI 副作用按 operation token + inputRevision 双守卫（`editResubmitOutcome.ts` 纯函数
  决策）。coordinator 的 begin/revoke **不受** token 守卫影响（会话事务必须推进）。
- operation mutex（P0-2/P1-1）：`activeEditOperationRef` / `activeRetryOperationRef` 是同步
  ref（state 挡不住同 tick 双发）。admission 拒绝 + finally 内在 `isCurrentOperation()`
  时清 ref + 降 loading。两个 ref 互查挡跨入口同 tick 双发。
- 附件集合差（C2）：提交时捕获已提交路径快照，只有 replacement 权威成功后才由
  `removeSubmittedAttachments` 精确移除；transcript 已截断但 replacement 失败时保留正文和
  附件作为普通草稿。飞行中新增保留、飞行中删除幂等。**禁止**在编辑路径全清（`clearFiles`）。
- 徽章（C3）：`editingMessageStore` 按 conversationId 分片，`{ownerId, msgId, pending,
  phase: editing | submitting | confirming, operationId?}`；confirming 时 SendBox mutex
  仍锁定，不得开启新的 destructive edit；clear 仅 owner 匹配生效；卸载自动清理。

## 3. 坑点（改代码前必读）

1. **React 函数式 updater 的执行时机不可信**。`update(fn)` 入队后 fn 延迟执行，期间 live
   全局状态可能已变（barrier 被其他 consumer 的同步 ack 删除）。规则：updater 体内不许
   查询可变外部状态，接受时冻结 snapshot 传入。复现：双 consumer + FIFO pendingUpdaters，
   先 ack 完再 drain → live 查询拿到空 barrier → 复活（P0-1）。
2. **epoch 与 snapshot 是两道独立栅栏，缺一不可**。epoch 挡「跨越 success 的在飞 fetch」；
   snapshot 挡「已接受但 updater 晚于 barrier 退休」。只有 epoch → P0-1；只有 snapshot →
   截断前快照整体灌入。
3. **armed 与 reconciling 的 purge 语义必须分离**。snapshot 的 `mergeKeys/serverIds` 是全
   barrier 并集（fetched 过滤），purge 集合（`purgeMergeKeys/purgeServerIds/localIds`）
   **只收 reconciling**。单并集 + purge 标志会把「op1 reconciling + op2 armed」中 op2 仍
   合法的后缀从当前列表误删（op2 随后失败则闪烁）。mixed snapshot 测试已固化。
4. **revoke 单调性**：armed→reconciling 单向；reconciling 后任何失败都不得 revoke（截断已
   durable，revoke 只会放陈旧 fetch 进来）。`revokeBarrier` 返回 boolean，拒绝时 console.warn。
5. **begin fail-closed 不 bump epoch 的安全性**：barrier 缺失仅三成因——失败已 revoke（DB
   未截断，无需 fence）/ 成功已被 ack 退休（epoch 早已 bump）/ coordinator 不存在（无消费者
   无物可保护）。孤儿 bump 只会误杀无辜在飞 fetch。
6. **state 挡不住同 tick 双发，ref 才行；mutex 的 ref 必须在 finally 清**。不清 → 首次提交后
   ref 永非 null → 编辑模式永久死锁。清的位置在 `if (isCurrentOperation())` 内；`.then/.catch`
   先于 `.finally` 执行，读 token 时 ref 尚未清。
7. **transport/HTTP/观察失败都是 ambiguous**：连接被拒、连接重置、timeout、普通 HTTP
   错误和 state GET 失败都不能证明 destructive transcript 没发生。必须用同一个 key 观察
   receipt + 精确 target/replacement；观察失败保持 `unknown`，不转换成目标不存在，也不 revoke。
8. **唯一安全 revoke 证据**：`receipt_state=missing`、target 精确存在、且本次请求是明确的
   admission 前拒绝。completed failure、target 消失、replacement 缺失、状态不一致都不能
   revoke；reconciling barrier 也禁止 revoke。
9. **同 key 是一次逻辑操作的边界**：SendBox 主编辑和 retry 各生成一次 `uuidv7`，同时交给
   coordinator 和 POST/GET。确认阶段的 replay 必须复用该 key + 原正文/附件，不能生成新 key；
   用户在 safe failure 后重新主动编辑才是新的 logical operation。
10. **confirmed mutation 后附件不能过早清理**：accepted + target 消失但 replacement 尚未
   存在时只能进入 confirming/reconciliation，不能清掉附件；只有 replacement 权威存在或
   明确成功才清理本次提交附件，post-mutation failure 要保留为草稿。
11. **`useAddEventListener` 按 deps 重绑闭包**：listener 体内读到的 state 是绑定时的快照；
    用 `isLoading` 做守卫必须进 deps（参照 retry 监听器写法）。
12. **结构测试很脆**：断言源码字面量与相对位置。inline 重构（filter 提成 helper）会让断言
    失配——改实现时同步改断言；切片锚点选稳定注释（如 `// Steering injects into the turn`）
    而非易变代码行。
13. **`maybeDestroy` 会归零 epoch**：写 coordinator 测试若要在无 barrier 后断言 epoch，必须
    先 `retainConsumer`，否则断言到的是销毁后的 0。
14. **branded type**：`MessageId`/`ConversationId` 是 branded string，测试字面量要
    `as MessageId`（或既有用例的 `MID()` helper）。
15. **i18n 脚本在根 package.json**：`bun run gen:i18n` 在仓库根跑，ui/ 下没有该 script。

## 4. 有意保留的取舍（勿当 bug 修）

- **armed 窗口内新挂载实例短暂看不到旧行**：成功/失败后 refresh 自愈，瞬态不处理。
- **无 msg_id 的 stream-only 行不承诺跨实例收敛**（localIds 仅捕获实例可达）；durable 行
  （含持久化报错 tips，msg_id == message_id）全承诺。
- **编辑不继承原消息附件**：重发取提交时 composer 当前选择，成功后集合差精确移除。
  `nomi.selected.file.clear` 是 5 处 emit、0 监听的死事件：编辑路径已停止发射，事件类型与
  其他 emit 点保留（删 emit 属 contract 变更，未做）。
- **NomiSendBox 成功路径用 live `purgeCurrentRows` 而非 snapshot**：begin 与 purge 同步同
  task，barrier 不可能在此期间被 ack 删除，无需冻结。
- **重发等待态**：首次 POST 返回不等于成功；`confirming` 期间保持 waiting 和 edit mutex，
  直到 exact observation 得到 success / safe_failure / post_mutation_failure。confirmed mutation
  的 barrier 仍由 `conversation.messages.refresh` 的 consumer ack 退休，不由 waiting state 代替。
- **编辑报错弹窗与源消息的关联靠 `findLast`**：够用，不做显式关联。

## 5. 验证门禁（改动后必跑）

```bash
cd ui && bun test                                   # 全量；历史基线约束见下文
bun test src/renderer/pages/conversation/Messages/ src/renderer/components/chat/SendBox/
cd .. && bun run typecheck && bun run check         # 无 CI，check 必须手动
cargo fmt --all -- --check
cargo check --workspace
cargo test -p nomifun-db latest_user_text_query_ignores_history_window_and_uses_message_id_tiebreaker --test conversation_repository
cargo test -p nomifun-db accepted_edit_resubmit_receipt_fences_rewind_and_truncate_crash_states_until_reset --test conversation_repository
cargo test -p nomifun-conversation edit_resubmit_delivery_state_uses_receipt_namespace_and_exact_message_ids --lib
bun run build:ui
```

2026-08-07 V5.2 实测：前端定向回归 133 pass / 0 fail（含 coordinator、loadOlder、merge、
send idempotency、exact observation、recovery resolver）；`bun run typecheck`、`bun run check`、
`bun run build:ui`、`cargo fmt --all -- --check`、`cargo check --workspace` 和上述三项 Rust
focused test 均通过。沙箱内直接执行 Bun 曾返回 `Operation not permitted`，需使用受控
PowerShell 工作区命令重试；这不是 TypeScript 编译错误。UI build 的动态导入/大 chunk 是既有
warning，不是本次失败。

预存基线失败 18 个（与本链路无关，勿修也别慌）：Guid preset picker ×2、PresetSettings
shell ×2、theme control、speech input CORS、capabilities checkbox、SkillsSettingsPage tabs、
SendBox add-menu、SendBox stop、Titlebar tooltips、transcript capability boundary、SessionList
dark-theme、MessageThinking soft-closed、PinnedPlan popover、ProcessTraceItem、
TurnDeliverablesCard、conversation.update merge_extra。

手动冒烟（dev）：

- 长会话中让目标消息落在最新 200 条之外，验证后端 latest-user 精确拒绝旧目标、接受真正最新目标；
- 报错 → 编辑 → 改文本重发（旧行/报错消失、新气泡单份）；同 tick 双击只发一次；重发期间点「编辑」被忽略；
- 在 POST 响应丢失、timeout、普通 HTTP error、truncate 后 replacement error 四种时序下，验证
  不生成新 key、不错误 revoke，post-mutation failure 保留当前正文和附件草稿；
- 同 key accepted/in-flight replay 只观察或幂等 replay，不发生第二次 destructive truncate；
- 刷新/重进会话一致，所有 consumer 最终 ack，barrier 不残留。

## 6. 测试地图（全部纯 TS，无渲染基建）

| 文件 | 形态 | 覆盖 |
|---|---|---|
| `Messages/editResubmitResurrection.test.ts` | 行为（真实 coordinator + deferred Promise / pendingUpdaters FIFO 模拟 React 调度） | 变体 A/B、乱序、连续编辑、失败收敛、多消费者、生命周期泄漏、P0-1 deferred race、P0-3 单调性、P2-2 fail-closed |
| `Messages/editResubmitState.test.ts` | 结构（readFileSync + indexOf 断言源码顺序） | pipeline 顺序、C2 时序、revision 守卫、P0-2/P2-3 mutex、双域拆分、P1-1 retry mutex、P1-2 核对先行 |
| `platforms/nomi/editResubmitRecovery.test.ts` | 纯函数 | receipt accepted/completed/missing、精确 target/replacement、truncate 后失败、状态不一致、唯一 safe revoke |
| `common/adapter/ipcBridge.conversation-send-wire.test.ts` | wire contract | edit-resubmit state GET、同 key header、candidate replacement identity |
| `Messages/hooks.loadOlderEpoch.structure.test.ts` | 结构 | P2-1 捕获/比较位置 |
| `Messages/refreshChannelDrift.structure.test.ts` | 结构（递归扫 renderer） | 通道单消费者契约 |
| `SendBox/editResubmitOutcome.test.ts` | 纯函数 | C2 结果决策 |
| `utils/file/messageFiles.test.ts` | 纯函数 | 附件集合差 |

## 7. 后续方向（按优先级）

1. **手动冒烟补测**：V5.2 自动门禁已通过，第 5 节长会话、响应丢失、truncate 后错误、同 key
   replay、附件草稿保留和 barrier 退休仍需 owner 在真实 UI/运行时验收。
2. **Windows runtime 基线**：`cargo test -p nomifun-conversation edit_resubmit --lib` 中
   `edit_resubmit_delivery_state_uses_receipt_namespace_and_exact_message_ids` 通过，但既有
   `edit_resubmit_rebuilds_a_missing_terminal_runtime_before_rewind` 仍因 runtime-lock authority
   文件 `拒绝访问 (os error 5)` 失败；不要把它归因于 V5.2，先修复当前用户 runtime-lock 权限后重跑。
3. **CI 门禁**：仓库无 `.github/workflows/`（仓库约束禁止随意新增；若要建，最小集合 =
   `bun test --cwd ui` + typecheck，先与维护者确认）。预存 18 个失败会让全量 CI 红灯——
   要么先修基线，要么 CI 只跑本链路目录。
4. **确认等待 UX**：当前 confirming 使用同 key 的自动 backoff/观察/replay，并提供「继续确认」
   入口；按钮只唤醒当前观察/重放，不能解锁 SendBox、创建新 key 或再次 destructive edit。
5. **渲染测试基建**：当前零 React 渲染测试能力，deferred race 靠纯 TS 模型保真（FIFO 假设
   成立但不验证批处理重入）。引入 testing-library 后可升级为真实组件级回归。
6. **死事件清理评估**：`nomi.selected.file.clear`（5 emit / 0 listener）等历史通道可单开
   cleanup 轮次统一处理（contract 变更需全链路 grep）。

## 8. 变更历史

- **2026-08-07 V5.2 编辑重提交可靠性修复**：新增 edit-specific state GET 和
  `EditResubmitObservation`（receipt namespace、candidate replacement、精确 target/replacement
  查询），latest-user 查询改为 `created_at DESC, message_id DESC LIMIT 1`；前端统一 uuidv7
  operation/key，加入 `editing/submitting/confirming` 和纯函数 recovery resolver。timeout、
  network、普通 HTTP、观察失败和状态不一致 fail closed；只在明确 pre-admission 失败时 revoke，
  truncate 后失败保留正文/附件草稿；保留原 epoch/barrier/snapshot/loadOlder 机制。自动验证：
  UI focused 133 pass / 0 fail，typecheck/check/UI build、workspace check、格式和新增 Rust
  focused tests 通过；既有 cold-runtime edit test 的 Windows authority lock `os error 5` 仍是
  环境阻塞，真实 UI 手工验收待 owner 执行。
- **2026-08-07 竞态根治（V5 + V5.1）**：分支 `fix/conversation-error-edit`。V5 `6d804ade`
  建 coordinator 三件套（epoch/barrier/consumer ack）+ C2 编辑态时序与附件集合差 + C3 徽章
  + C4 可观测性；V5.1 `a2a78822` 收口 review 边界：P0-1 snapshot 冻结（updater 不再查 live
  coordinator）、P0-2/P1-1 operation mutex、P0-3 双 failure domain + revoke 单调性、P1-2
  ambiguous 失败权威核对、P2-1 loadOlder epoch fencing、P2-2 begin fail-closed、P2-3
  sendbox.edit 在飞守卫、J 30s timeout、K 通道契约防漂移。验证：2043 pass / 18 预存失败，
  typecheck + check 全绿。根因与设计推导见第 2/3 节。

（后续迭代在此追加条目，保持最新在上。）
