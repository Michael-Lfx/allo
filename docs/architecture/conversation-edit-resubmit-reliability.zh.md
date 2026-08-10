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

2026-08-09 本次实测：新增/定向 UI 测试通过；UI 全量为 2097 pass / 18 个既有结构基线失败；
direct Vite production build、`cargo fmt --check`、`cargo check --workspace`、repository snapshot、
latest-user、服务级 >200 行旧目标拒绝、observation namespace/permission/reset 测试通过。
`cargo test -p nomifun-conversation edit_resubmit --lib` 为 4 pass / 1 Windows runtime-lock
`拒绝访问 (os error 5)`。直接 TypeScript 检查只报告未改动的 videoCanvas `Blob` 基线；直接
quality scripts 除未改动的 agent-vocabulary 注释基线外通过。`bun run typecheck/check/build:ui`
的 workspace 子进程在当前 Windows 沙箱返回 `Operation not permitted`，等价 Node 入口已执行。
截至 2026-08-09，自动验证已完成；当时尚未进行 owner 交互验收，不能由自动测试替代。

2026-08-09 合并审查修复追加：strict decoder 已覆盖非 replay、accepted terminal metadata 和矛盾
completed outcome；runner 释放后 remount subscription、stale refresh retry、生产 reset handler 的
deferred 双击均有行为测试。repository 增加并发 writer/readers 测试，在 80 次原子 transcript
generation 切换期间只允许完整的 mutation 前/后 snapshot。后续 V5.6 增加了 stale-target 的
Composer 内 Alert，因此该可见 UI 变化必须以截图或短录屏补充人工证据，不能再使用“没有可见控件变化”
作为豁免。

人工验收清单包括：长会话且 target 在最新 200 行之外、同时间戳 latest-user tie-break、double click、
会话切换后同-key 恢复、response loss、truncate 后 HTTP/terminal error、附件差集、refresh 首次失败后
重试并 ack，以及 accepted orphan 的显式 reset。后文逐次记录已完成项目，未明确记录的项目仍待验收。

2026-08-10 独立 Composer beforeinput 分支 Web 实测追加：逐键输入 `v55逐键输入` 后浏览器控制台为 0 error，不再出现
`undefined.includes`。最终 post-mutation failure 场景双击只产生 1 个 POST，POST 与 observation
共用 key `019fe76a-d12a-77e4-8600-6b7dc66afc68`；observation 为 completed failure、target 消失，
同一挂载立即移除 Editing badge/banner，并把 `编辑可靠性实测：V5.5 final acceptance。` 保留为
普通草稿。该轮后端 terminal error 是 `preparation_failed`；此前同会话已取得免费模型
`USER_LLM_PROVIDER_RATE_LIMITED` 的 completed-failure 证据。
本地验收会话按约定保留，未自动删除。

后续使用运行时实际提供的 `mimo-v2.5-free` 补齐成功路径：普通 turn、单击 edit-resubmit 和修复后的
双击 edit-resubmit 均 completed success。双击操作只产生 1 个 POST、没有 `/cancel`，operation key 为
`019fe948-83c1-732c-b711-42a16449e4b3`，receipt `result_ok=true`、replacement
`019fe948-83c9-72c2-943d-1c976a4906ba`、terminal text `双击成功`。终态后 Editing badge/banner 清零，
composer 清为空草稿。当前 managed catalog 没有名为 Kimi 2.5 的条目，实测模型名称必须记录为 MiMo。

V5.5 定向 UI 测试通过；UI 全量为 2110 pass / 18 个既有基线失败。direct Vite
production build 通过（13507 modules），直接 TypeScript 检查仍只报告未改动的 videoCanvas
`Blob` 7 项基线。`git diff --check` 通过，`.github/workflows` 下不存在 YAML workflow。根级 Bun
workspace wrapper 在 Web dev 子进程并存时仍可能返回 Windows `Operation not permitted`，因此构建和
typecheck 同时记录 direct Node 入口结果，未把该运行时锁归因于 V5.5。

2026-08-10 V5.6 收口：terminal ledger 覆盖 A→B→A，Stop gate 只消费多击事件；missing receipt 且
target 已消失时，统一 reconciliation 并显示 Composer 内提示，正文和附件保持普通草稿。原服务级
“>200 行旧目标拒绝”测试此前误用 ACP fixture，只验证到任意 BadRequest；现已改用 Nomi fixture并
断言精确 old-target 错误及无 receipt。冷运行成功 fixture 增加 220 条 assistant/error suffix，以锁定
长 transcript 的正向 admission。Windows knowledge-workspace runtime lock 仍可能阻塞该冷运行测试，
单独按环境基线记录。

V5.6 自动验证：定向 UI 91 pass / 0 fail；同步 `origin/main` 后 UI 全量 2119 pass / 18 个既有基线失败；两个精确
service admission 测试通过。direct TypeScript 只报告未改动的 videoCanvas `Blob` 7 项基线，direct
Vite production build 通过（13509 modules），`cargo check --workspace`、i18n、theme、icons、
CodeMirror/runtime/browser boundary、help、Rust format 和 `git diff --check` 通过。agent-vocabulary 仍只
报告主干已有的 `nomi-agent-eval/src/runner.rs` retired reference。冷运行正向测试在当前 Windows 环境
被 knowledge-workspace lock `拒绝访问 (os error 5)` 阻塞，未记为功能失败。

2026-08-10 PR #70 减负收口：Send→Stop 现在直接使用 React click detail，只有同一多击序列的第二击
会被 handoff gate 消费；主动单击和键盘 Stop 不受影响。retry 的 post-mutation failure 在当前
draft revision 未变化时恢复 retry 正文，用户中途输入则不覆盖。消息刷新在快照入队后等待实际 React
commit，再执行 consumer ack；reconciliation purge 使用冻结 snapshot，不再从延迟 updater 查询 live
coordinator。移除了 trivial target-notice helper 与 production live filter/purge 双轨路径；Composer
beforeinput 解码、测试和 changelog 已移到独立 `fix/composer-beforeinput` 分支，不属于 PR #70。

当前 PR #70 的手工截图/短录屏证据仍需在最终 PR 描述中附加，至少覆盖 stale-target Alert；自动化测试
不能替代这项可见 UI 验收。

## 交付边界

当前实现分支为 `fix/conversation-error-edit`，Composer 独立分支为 `fix/composer-beforeinput`，对照
基线和 HEAD 不写死在本文中；交付时以 `git rev-parse HEAD`、`git merge-base HEAD origin/main` 和
远端 ref 的实时结果为准。本文不记录易过期的“最终提交数量”或历史最终 SHA。
