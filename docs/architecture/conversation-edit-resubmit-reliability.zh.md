# 编辑重提交可靠性

本文记录会话消息编辑、报错重试和重提交的权威恢复协议。实现保留既有
epoch/barrier/snapshot/consumer ack 与 `loadOlder` keyset 架构；没有数据库迁移。

## 契约

一次逻辑操作只生成一个 UUIDv7。该值同时是 renderer coordinator operation ID、POST
`Idempotency-Key`、GET observation key 和同-key replay key。正文、附件路径和后端 input
在首次提交时冻结；恢复不得生成新 key 或重建不同 payload。

后端观察接口固定为：

```text
GET /api/conversations/{conversation_id}/messages/{message_id}/edit-resubmit/state
Idempotency-Key: <原操作 UUIDv7>
```

repository 在同一个 SQLite read transaction 中读取精确 edit receipt、target 是否存在、
receipt candidate replacement 是否存在，以及会话是否有 accepted edit fence。service 先校验
用户和会话，再校验 operation namespace、receipt kind、request payload target 与 candidate ID。
读取、解析或身份错误直接失败，不能降级成“消息不存在”。普通 `public-turn` receipt 即使使用
相同 client key，也不属于 edit receipt。

`requires_reset` 只表示无法自动恢复：accepted edit fence 没有 preparation gate 或 durable live
owner；或 completed receipt 仍显示 target 存在，无法证明 destructive cutpoint。它从来不是
revoke 证据，也不会触发自动 reset。

## 前端状态机

wire decoder 对 receipt enum、必填 boolean、delivery/candidate/replacement 组合做运行时校验。
observation delivery 必须是 durable replay；accepted receipt 不允许携带任何 terminal metadata，
completed receipt 的 success/error 字段也不能互相矛盾。字段缺失、未知枚举或身份不一致都作为
observation failure，fail closed。

| Observation | 结果 | 行为 |
| --- | --- | --- |
| `requires_reset=true` | `requires_reset` | 停止 replay，不 revoke，等待用户显式 reset |
| missing + target 存在 + server 明确拒绝 | `safe_failure` | 唯一允许 revoke 的路径 |
| missing + target 存在 + transport/观察不确定 | `unknown` | 保留 key/barrier；每确认周期最多一次同-key POST |
| missing + target 消失 | `post_mutation_failure` | reconciliation，保留草稿并显示 Composer 内 target-changed 提示，停止 replay |
| accepted + target 存在 | `claimed_pending` | 只 GET |
| accepted + target 消失 | `transcript_truncated` | 立即 reconciliation，继续 GET terminal receipt |
| completed + target 消失 + success + replacement 存在 | `success` | reconciliation 后清未修改正文和本次附件 |
| completed + target 消失但失败/结果缺失/replacement 不一致 | `post_mutation_failure` | 同挂载同步清旧 target/badge，正文和附件保留为普通草稿 |
| completed + target 存在或其他不可能组合 | `requires_reset` / `unknown` | 绝不 revoke |

自动确认使用 capped backoff。accepted receipt 不重复 POST；只有 missing + transport ambiguous
每周期允许一次同-key replay。周期耗尽后停在 confirming，不再自动联网；“继续确认”启动新周期，
仍使用原 key，不能解锁新 destructive submit。

## Renderer 生命周期

`EditResubmitOperationRecord` 按 conversation 保存：operation ID、target ID/createdAt、原正文、
精确 backend input、附件路径、draft revision、来源 `edit | retry`、phase、request outcome 和最后
observation。模块级同步 admission 同时覆盖 edit、retry、Enter、click 和双 SendBox 实例。

会话切换或 unmount 只释放 runner lease 和 timer。`submitting/confirming` record、armed barrier、
key 和 payload 留在同一 renderer；返回会话或 remount 后通过 runner-availability subscription
自动 adopt。新旧 renderer 短暂重叠时，新实例会等待旧 owner 释放后再恢复，不依赖一次性 effect
的执行顺序。应用/renderer 重启不持久化敏感草稿和 payload，accepted orphan 仍要求显式 reset。

Nomi draft 的 `contentRevision` 跨 remount 保留。成功只在 revision 未变化且来源为 edit 时清正文；
附件始终按提交路径集合做精确差集，飞行中新附件保留。post-mutation failure 不清正文或附件。

Nomi 在权威 terminal 分类后按 `reconciliation → terminal lifecycle event → shared editing store clear
→ controller release` 提交终态。SendBox 使用当前挂载生命周期内的 operation ID ledger 幂等消费
terminal，因此 A→B→A 的旧终态不会再次污染 B；`success` 退出编辑并仅在
revision 未变时清正文，`post_mutation_failure` 立即退出编辑但不清、不恢复正文。terminal subscriber
抛错不会阻止共享状态和 controller 清理；Promise resolution 只作为非 Nomi 调用方的幂等回退。

operation controller 的 recovery subscriber 使用微任务 adoption：同一 tick 新建操作时，发起它的
SendBox/Nomi 调用先获得 runner lease，subscriber 不得抢先以无 terminal callback 的恢复路径执行；
旧 renderer 真正释放 runner 后，subscriber 会重新检查 operation ID、phase 和 owner 再恢复同一 key。

编辑提交还使用事件归属明确的 Send→Stop handoff gate：首击同步接纳 operation 后，只有 500ms 内
`click.detail >= 2` 的同一多击序列会被消费，避免第二击落到刚替换出来的 Stop 并取消自己的
preparation lease。`detail=1` 的主动单击、`detail=0` 的键盘触发和后续 operation 始终允许 Stop。

latest-user admission 使用无窗口的 `created_at DESC, message_id DESC LIMIT 1` 精确 SQL。若 observation
权威确认 `receipt=missing + target_exists=false`，该状态表示目标已被其他操作改变：前端统一
reconciliation、退出旧编辑态并保留正文/附件，在 Composer 上方显示可关闭的本地化提示，不解析
英文错误字符串，也不使用全局 Arco error toast。其他 terminal/model failure 仍沿用原错误展示。

coordinator 对同 operation 的 `arm` 和 `begin` 幂等：remount 不覆盖 reconciling barrier、不重复
bump epoch。所有已确认 transcript mutation 都调用统一 reconciliation。`conversation.messages.refresh`
使用可取消、去重、capped-backoff 的 retry-until-success；只有页面通过 sequence/epoch fence、实际
应用并执行 consumer ack 才返回成功。被其他请求淘汰的 stale/no-op refresh 会继续退避，不会泄漏
barrier。

显式 reset 使用生产级 promise single-flight handler；同 tick 双击复用同一 Promise，只发一次 IPC，
start/success/settled lifecycle 也各执行一次。reset 成功才提交新 generation、清 operation/barrier
和触发权威 refresh，失败不清 `requires_reset`。

## 关键文件

- `crates/backend/nomifun-db/src/repository/conversation.rs`
- `crates/backend/nomifun-db/src/repository/sqlite_conversation.rs`
- `crates/backend/nomifun-conversation/src/service.rs`
- `ui/src/common/adapter/ipcBridge.ts`
- `ui/src/renderer/pages/conversation/Messages/editResubmitOperationController.ts`
- `ui/src/renderer/pages/conversation/Messages/conversationMessageCoordinator.ts`
- `ui/src/renderer/pages/conversation/Messages/hooks.ts`
- `ui/src/renderer/pages/conversation/platforms/nomi/editResubmitRecovery.ts`
- `ui/src/renderer/pages/conversation/platforms/nomi/NomiSendBox.tsx`
- `ui/src/renderer/components/chat/SendBox/index.tsx`
- `ui/src/renderer/components/chat/SendBox/editResubmitLifecycle.ts`
- `ui/src/renderer/components/chat/ComposerSubmitCluster.tsx`

## 验证

自动门禁：

```bash
bun test ui/src/renderer/pages/conversation/Messages \
  ui/src/renderer/pages/conversation/platforms/nomi
bun run typecheck
bun run check
cargo test -p nomifun-db --test conversation_repository
cargo test -p nomifun-conversation edit_resubmit --lib
cargo fmt --all -- --check
cargo check --workspace
bun run build:ui
git diff --check
```

主干已知基线必须单列：videoCanvas `Blob` typecheck、agent-vocabulary 文档注释和历史 UI 结构
测试不属于本功能。Windows incremental/runtime-lock 的 `拒绝访问 (os error 5)` 也属于环境阻塞，
不能归因于本变更。

## 验收与交付证据

可见 UI 变化必须提供真实运行截图或短录屏；自动化测试不能替代 owner 交互验收。长会话、同时间戳
latest-user、同-key recovery、response loss、truncate 后失败、附件差集、commit 后 ack 和 accepted
orphan reset 均需要通过对应的服务级、行为级或人工场景证明，不能由宽泛的成功计数替代。

架构文档只维护稳定协议。当前分支、测试结果、环境阻塞和人工验收记录位于
[PR #70 编辑重提交可靠性交接](../handoffs/2026-08-10-conversation-edit-resubmit-pr70.md)，交付时以
实时 Git refs 和该记录中的证据边界为准。
