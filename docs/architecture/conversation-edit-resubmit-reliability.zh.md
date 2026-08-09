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
| missing + target 消失 | `post_mutation_failure` | reconciliation，保留草稿，停止 replay |
| accepted + target 存在 | `claimed_pending` | 只 GET |
| accepted + target 消失 | `transcript_truncated` | 立即 reconciliation，继续 GET terminal receipt |
| completed + target 消失 + success + replacement 存在 | `success` | reconciliation 后清未修改正文和本次附件 |
| completed + target 消失但失败/结果缺失/replacement 不一致 | `post_mutation_failure` | 清旧 target/badge，正文和附件保留为普通草稿 |
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

2026-08-09 本次实测：新增/定向 UI 测试通过；UI 全量为 2097 pass / 18 个既有结构基线失败；
direct Vite production build、`cargo fmt --check`、`cargo check --workspace`、repository snapshot、
latest-user、服务级 >200 行旧目标拒绝、observation namespace/permission/reset 测试通过。
`cargo test -p nomifun-conversation edit_resubmit --lib` 为 4 pass / 1 Windows runtime-lock
`拒绝访问 (os error 5)`。直接 TypeScript 检查只报告未改动的 videoCanvas `Blob` 基线；直接
quality scripts 除未改动的 agent-vocabulary 注释基线外通过。`bun run typecheck/check/build:ui`
的 workspace 子进程在当前 Windows 沙箱返回 `Operation not permitted`，等价 Node 入口已执行。
人工验收尚未由 owner 完成，不能由自动测试替代。

2026-08-09 合并审查修复追加：strict decoder 已覆盖非 replay、accepted terminal metadata 和矛盾
completed outcome；runner 释放后 remount subscription、stale refresh retry、生产 reset handler 的
deferred 双击均有行为测试。repository 增加并发 writer/readers 测试，在 80 次原子 transcript
generation 切换期间只允许完整的 mutation 前/后 snapshot。该修复没有布局、样式或可见控件变化，
因此无需新增截图；交互层人工验收仍按下列清单执行。

人工验收仍必须覆盖：长会话且 target 在最新 200 行之外、同时间戳 latest-user tie-break、
double click、会话切换后同-key 恢复、response loss、truncate 后 HTTP/terminal error、附件差集、
refresh 首次失败后重试并 ack，以及 accepted orphan 的显式 reset。

## 交付边界

当前实现分支为 `fix/conversation-error-edit`，对照基线为 `origin/main` `31638c21`。文档不记录
易过期的“最终提交数量”或历史最终 SHA；交付时以 `git rev-parse HEAD`、`git merge-base HEAD
origin/main` 和远端 ref 的实时结果为准。
