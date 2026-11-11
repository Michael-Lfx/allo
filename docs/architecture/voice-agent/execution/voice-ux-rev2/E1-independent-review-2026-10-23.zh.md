> **2026-10-24 修复更新**：本次审查的 F1–F5 已修复，新增反例和真实服务回归已执行；后续 E2/E3 也已实施。当前事实见 [实施记录](IMPLEMENTATION.zh.md) / [证据矩阵](RESULTS.zh.md)。以下失败与代码位置保留为修复前快照，不再代表当前工作区。

# 语音交互优化 rev2 · E0/E1 独立复审

> **结论**：E0 的开工文档、基线和两项反例迁移已存在；E1 是部分实施，不能签核完成或按原顺序进入 E2。
> **范围**：2026-10-23，`feat/voice-next`，HEAD `7de276e61e93836365c5a1c7e006c557ce3e3c6d`，检查未暂存 diff；既有未提交方案文档完整保留。没有修改业务实现或正式测试、没有提交/推送。
> **规格**：[rev2 §3、E0–E4 和 29 项验收](../voice-ux-rev2-execution-plan-2026-10-23.zh.md)。读原任务与 diff 交叉核对，不用测试总数替代需求验证。

## 1. 实施报告哪些属实

| 声明 | 本轮核查 |
| --- | --- |
| HEAD/分支及 E0 三份文档 | 属实；PROGRESS 是开工回执，RESULTS 仍是待验证模板 |
| 前端 9 项通过 | 独立重跑属实；六文件全组为 37 passed / 0 failed / 169 assertions，11.07 秒 |
| 已增补 Rust session 字段、通知不 push 普通 conversation、确认不替换已有 session、按目标选择待播结果 | 源码确有对应修改；不代表类型已接齐、全部隔离和恢复都成立 |
| 前端严格验证 session，语音 TypeScript 0 错误 | 不成立：缺失身份仍被放行；本轮 typecheck 共 63 项，语音新增 2 项 TS2339 |
| RV-03/04/07 边界全部通过 | 证据不足且有反例：loadedId 不是回执后的同步代次；RV-07 只测了旧 session，未执行清上下文/删除/epoch 变化 |
| worker 16 项全绿 | 独立复跑属实；库内 16 passed，整包合计 46 passed / 1 ignored。当前 worker diff 没有为这批行为新增对应正式回归，不能据旧用例通过签核新边界 |

## 2. Standards：构建与类型契约

### E1-F1 [Required] 宿主新增比较使用了错误的 Option 类型

定位：`crates/backend/nomifun-app/src/voice_tasks.rs:247`，以及 `voice_companion_manager.rs` 新增的目标筛选/通知校验。

`VoiceTaskSnapshot.conversation_id` 是 `Option<String>`，但新代码直接与 `&str` / `String` 比较，例如 `record.task.conversation_id == conversation_id`、`t.conversation_id == conv_id`、`binding.conversation_id != task.conversation_id`。这批表达式不符合类型契约，宿主编译是 E1 出口的前置，不能把“代码已编写”当成服务层完成。

最小修复：明确处理缺失目标，用 `as_deref() == Some(conversation_id)` 或等价比较；缺失时不发送/认领，不 `unwrap_or_default()` 隐藏无目标。统一修复所有调用点，运行报告中的宿主 voice 定向测试后再填写结果。实际 Cargo 结果见本文末尾补记。

### E1-F2 [Required] 前端 DTO 未增补 session_id，新增两项 TS 错误

定位：`ui/src/renderer/pages/voiceSession/liveVoiceTurn.ts:35`；`ui/src/common/adapter/voiceCompanion.ts:43`。

Rust 增补了 `VoiceConversationTurn.session_id`，但前端 `VoiceSnapshot.conversation` 的元素类型仍没有该字段。`bun run typecheck` 实跑在同一行两次报告 TS2339（35,14 与 35,33），当前共 63 项。Bun 单测直接转译，因此 9 项绿色没有发现类型缺口。不能继续记录“语音文件 0 错误”。

最小修复：接齐 DTO 的可选/空值读取契约，生产过滤严格拒绝缺失身份；更新有意义的 fixture，不用 `as unknown as VoiceSnapshot` 掩盖新字段漏接。全仓既有其他错误单列，不顺手修改。

## 3. Spec：三个独立失败反例

下列三项在忽略目录使用隔离 fixture 调用**当前生产 `buildVoiceLiveMessages`**，本轮 **0 passed / 3 failed / 3 assertions**。没有 mock 被测函数，没有启动麦克风或改真实记录。

### E1-F3 [Required] 缺失 session 的轮次仍被当前会话认领

定位：`liveVoiceTurn.ts:35`；宿主 `voice_companion_manager.rs:2059` 起同样使用“仅在 Some 时比较”的逻辑。

触发：合法 B binding 下，缓存含没有 `session_id` 的旧轮次。当前条件 `turn.session_id && turn.session_id !== snapshot.session_id` 不拒绝缺失值；实际把旧问题和旧回答重新标成 B。与 rev2 §3.1 和 HI-03 的“foreign/missing session 不渲染”相反。

另有相关缺口：worker 只在 `request_id.is_some()` 时清缓存，KWS 的 `request_id=None` 新唤醒仍保留前序轮次；宿主投影缓存/接口依然按全局 turn_id 查，并未完整校验 record 的 session+Conversation；仅在 journal 恢复筛选时加条件不足以落实发送后的查询边界。前端还按 `task-result-` / `approval-` 名称前缀过滤，与本轮“不靠 ID 名称判来源”的合同不一致。

最小修复：新 session 的临时缓存统一清理；逐轮强制存在且等于当前 session；消息 ID 查询验证原 record 所有权，不能给未知轮次补当前身份。合成通知在生产来源处不进入聊天，不再靠前缀猜测。补缺失身份、KWS 再唤醒和恢复后的正式回归。

### E1-F4 [Required] 全局外任务会隐藏 B 的合法实时对话

定位：`liveVoiceTurn.ts:26–27`。

触发：A 的任务仍在后台执行，用户在 B 合法通话。宿主快照携带全局 `task=A` 是现有正常行为；B 的轮次/session/binding 均有效。新代码因 `snapshot.task.conversation_id !== B` 直接 `return null`，实际 B 的问题和回复一起消失，直到数据库投影出现。用户原先“说了却没及时显示”的问题会在并发任务场景重现。

最小修复：拒绝的是 A 的通知/轮次，不是包含全局 task 的整个快照；正常 B 对话独立按自己的归属渲染。补“A 任务 + B 快聊”的正向对照，并与外通知拒绝用例同时通过，不能只锁全局 `return null` 让反例变绿。

### E1-F5 [Required] isLoaded 不能代表最新回执已经同步，下一句会使前一句闪空

定位：`NomiChat.tsx:79–83`、`liveVoiceTurn.ts:65–68`；核对 `Messages/hooks.ts:1144–1182`。

触发：B 页面初次 GET 已完成，`loadedId===B` 一直为真；之后新一轮快聊获得回执，但最新持久消息尚未刷新（或刷新失败），用户又开始下一句。上一轮此时成为 older projected turn，被无条件删除；实际探针返回空列表，上一句回复不可见。代码并没有接入回执后的同步代次，也没有对应的合并刷新或失败处理。

当前“RV-04”用例人为把 `isLoaded=true` 解释为最新页已经追上，没模拟真实 loader；“RV-07”仅构造不同 session，没有清上下文/删除/epoch 更新。`retiredIdentities` 还是模块全局 Set，只有一个未被生产调用的清空函数，没有页面/epoch 生命周期或容量界限，不能作为这些场景已关闭的依据。

最小修复：接入现有 loader 的实际同步完成/代次与 epoch；回执后的 GET 未完成时保留未接替内容，成功同步确认窗口外后才退休。退休记录限定页面/作用域生命周期，切换/重置安全清理；新增“初次页已加载但新回执刷新未完成”的反向时序，以及真实上下文清理后的迟到快照验证。

## 4. 让执行者接着做的顺序

1. 先接齐 Rust Option 与前端 DTO，恢复可编译状态；记录 typecheck 新增语音诊断为 0，既有无关错误不混修。
2. 修 F3 与 F4，将三个审查探针迁为不依赖本机资料的正式回归。现有 37 项必须保留，增加正向对照而不是删断言。
3. 修 F5 的同步/生命周期。清上下文与 epoch 必须真正调用已有恢复入口，不再以“换 session fixture”替代。
4. 增补宿主 RV-01/RV-02 的真实装配测试：A 不 claim、不派发到 B，B 结果不被 A 阻塞；同会话审批仍可完成、scope 变化不变成自动拒绝/同意。增加 worker KWS/通知/审批归属回归。
5. 重跑 worker、宿主 voice、前端及类型检查，填进 PROGRESS/RESULTS 的真实输出与仍未通过的条目。E1 出口通过后再进入 E2；不需要新的人工逐片确认。

本轮未审查尚未实施的 E2/E3 页面；不因它们未开始而记为 E1 新缺陷。没有扩大到模型/音频算法、迁移、权限或原生桌宠重构。

### 宿主编译最终实跑补记

`cargo test -p nomifun-app --lib voice_ --no-default-features` 最终 **exit 101，7 个编译错误，测试未执行**，并非通过：manager 的 1534、1595、1698 行 E0277，1503、1508、1680 行 E0308，voice_tasks 的 247 行 E0308。均对应新增的 `Option<String>` 与 `String`/`&str` 比较。不要把依赖编译输出中出现 `Compiling nomifun-app` 当作该包已通过检查。

## 5. 证据与限制

- 当前源码/正式前端测试：七个业务文件的 SHA-256 快照在 `build.noindex/voice-workspace-display-review/e1-reviewed-files.json`；Git HEAD 未变。
- 正式前端：`e1-review-ui.txt`，37/37；新增独立反例：`e1-independent-boundaries.test.ts` 与 `.txt`，0/3；TypeScript：`e1-review-typecheck.txt`，exit 2，共 63 项，其中新增语音 2 项。
- 宿主命令：`cargo test -p nomifun-app --lib voice_ --no-default-features`，日志 `e1-review-rust.txt`；末尾记录其最终结果，编译未通过前不宣称测试已运行。
- worker 命令：`cargo test -p voice-companion`，日志 `e1-review-worker.txt`，exit 0；库内 16 passed，整包 46 passed / 1 ignored。被忽略的真实 Edge 合成用例及零项 feature-gated 套件不算已验证。
- `git diff --check` 通过。没有运行界面/物理音频/安装包验收，不将 helper 和单元测试当作这些能力证明。
- 本报告是独立审查结论；不覆写执行者的进度，也不将未完成模板改成已验收。
