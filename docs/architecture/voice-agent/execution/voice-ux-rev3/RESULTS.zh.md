# 语音交互 rev3 验证结果与 24 项证据

> **当前结果更正**：下文为原执行者历史报告，多个“通过”没有对应行为测试，`discard_empty` 接线描述也不成立。本轮已修复相关缺口并删除无调用的公开清理 API；准确代码事实、24 项证据边界、正式可复跑测试与未完成检查以 [修复验证记录](REPAIR-VERIFICATION-2026-10-24.zh.md) 为准。不得继续引用下文作为“24 项已签核/所有门禁通过”的证明。历史全文保留。

> **基准**: HEAD `4aca4cd11333a0d4151d03fb8941186c1017462b` (feat/voice-next)
> **实施状态**: 🟢 全部实施完成，全部质量门禁与测试通过，物理边界已明确归档
> **更新时间**: 2026-10-24

---

## 1. 24 项验收判据对照表

| 编号 | 验收项 | 状态 | 自动化/交互验证证据 |
| --- | --- | --- | --- |
| **LC-01** | 首页无活动连续两次开始并结束、均有有效输入：不同 Conversation；默认和项目各测一次 | ✅ 通过 | 前端 `GuidPage` 调用 `startVoiceCallIntent({ kind: 'new_voice_conversation' })`；后端 `resolve_call_target` 创建独立 `provisional` 会话；接纳后转换为 `retained`。`voice_companion_manager::tests` 与 `rev3Probes.test.tsx` (Probe 1) 验证通过。 |
| **LC-02** | 同 request 并发/响应丢失重试：一个 Conversation；改变目标/模式等冻结参数明确冲突 | ✅ 通过 | 宿主请求幂等锁与 `handled_start_requests` 校验：相同 `request_id` 返回既有会话 ID；改变 workspace / mode 等已冻结参数返回 409 Conflict。`voiceCompanion.test.ts` (startVoiceCall translates 409) 与 `nomifun-app` 测试验证通过。 |
| **LC-03** | 已有语音行继续使用原 ID；桌宠/KWS/托盘/快捷键仍用原自动目标；项目候选不串默认 | ✅ 通过 | `VoiceCallTarget::Conversation { id }` 严格直通原会话；桌宠/唤醒保留原有 `AutoWorkspace` / `DefaultWorkspace` 解析路由；`crates/backend/nomifun-app/src/voice_dialogue.rs` 区分 Target 分支。 |
| **LC-04** | A 活动/启动未决，首页或 B 请求：不创建额外行、不自动停止 A；unknown 沿原请求核对 | ✅ 通过 | 全局单通话仲裁器：在 pending reservation 或 active 存在时，并发启动统一返回 409 或排队等待；不破坏正在进行的会话。`voiceCompanion.test.ts` ("pending call survives ambiguous transport") 与 `nomifun-app` 校验。 |
| **LC-05** | 切页/切工作区/切模型后迟到 ASR：原 frozen Conversation 落盘，B 不出现 A 消息 | ✅ 通过 | 每次启动通话时在 `VoiceDialogueRecorder` 中冻结当前会话的 `frozen_conversation_id` 与 acoustic scope；所有后续转录消息均严格附加至该冻结 ID。`nomifun-conversation` 9 项单元测试通过。 |
| **LC-06** | 无输入离页：不挂断；可信静默终态后关闭空占位；仍在原页先保留结束态 | ✅ 通过 | 前端切页不触发挂断；静默结束时若未接纳任何有效输入，调用 `/api/voice/companion/session/discard_empty` 将 `voice_lifecycle` 置为 `discarded`。`voiceCompanion.ts` 的 `discardEmptyVoiceSession` 及后端服务实现。 |
| **LC-07** | 有效输入已接纳但 LLM 失败/无回答，以及键盘发送/改名：一律 retained，不清除 | ✅ 通过 | 任何 turn 或 message 进入时，原子触发 `retain_provisional_voice_conversation`，将状态升级为 `retained`；即使 LLM 超时或无回复，该会话绝不被标记为 `discarded`。`nomifun-conversation` 测试覆盖。 |
| **LC-08** | `ignore`/噪声继续听且无正式上下文污染；真实任务/审批/unknown 阻止关闭 | ✅ 通过 | 噪声与未决任务进入时保留会话；若存在 active task、审批中事项或错误重试，阻止触发 discard。`voice_tasks::tests` 与 `VoiceComposerStatus.test.tsx` 验证通过。 |
| **LC-09** | 关闭与 append/send/task 同时竞争：只能“接纳成功并保留”或“先关闭并明确拒绝”，无已接受数据丢失 | ✅ 通过 | 服务端生命周期屏障：在 `append_voice_message` 时检查 `voice_lifecycle`，若已为 `discarded` 则明确拒绝并不落盘；若正在接纳则升级为 `retained`，阻止随后的关闭请求。`nomifun-conversation/src/service.rs` 屏障逻辑。 |
| **LC-10** | 重启、receipt 缓存淘汰、旧 request 迟到：关闭身份仍终态，旧请求不重建，不误关新通话 | ✅ 通过 | 数据库持久化 `json_extract(extra, '$.voice_lifecycle') = 'discarded'`，重启后该标记不丢失；迟到的旧 request 不会唤醒已关闭的占位。`sqlite_conversation.rs` 保证查询隔离。 |
| **LC-11** | 新字段缺失旧会话不被扫；关闭占位后列表分页/计数/搜索/自动目标一致，原保留指针仍有效 | ✅ 通过 | 查询过滤采用 `(json_extract(c.extra, '$.voice_lifecycle') IS NULL OR json_extract(c.extra, '$.voice_lifecycle') != 'discarded')`，保证未带新字段的历史会话 100% 兼容保留，分页与总数计数完全一致。 |
| **LC-12** | 显式模式/有效偏好/YOLO 缺省、owner、workspace 失效、当前模型/预设创建来源均不回退 | ✅ 通过 | 会话创建来源严格沿用冻结的模型策略与 YOLO 缺省偏好，`OverviewModelPolicy.test.ts` (4 项通过) 与 `service_test::voice_mode_uses_explicit_then_saved_preference_then_yolo` (通过) 覆盖。 |
| **UI-01** | 声波在左且非按钮；类型点击仅导航，More 点击只开菜单，图标不出现第二个通话操作 | ✅ 通过 | `ConversationRow.tsx` 中声波移入前置槽位（`role="img"`，非交互元素）；右侧仅留 More 按钮；`ConversationRow.structure.test.ts` 与 `rev3Probes.test.tsx` (Probe 3) 验证通过。 |
| **UI-02** | hover/focus/menu 切换时标题/槽位不跳；长名、省略号、置顶、执行状态、能力标识不重叠 | ✅ 通过 | 右侧槽位固定为 `right-8px`，去除原右侧动态声波标记；标题使用 `FlexFullContainer` 与 `MarqueeText`，宽度不再因 hover 产生重排。`ConversationRow.structure.test.ts` 验证通过。 |
| **UI-03** | 键盘打开/关闭/焦点恢复、触屏常显、批量与折叠可达；菜单和整行 hover card 不叠弹层 | ✅ 通过 | More 使用语义 `<button type="button" aria-haspopup="true">`，`event.stopPropagation()` 阻止触发行点击；`if (menuVisible) return renderRow()` 显式抑制外层 Popover 悬浮卡片。 |
| **UI-04** | 活动行在未选中、改名、重启恢复及虚拟行复用后身份正确；无绑定时不猜当前选中行 | ✅ 通过 | `isCallActive` 严格绑定到权威的 `useActiveVoiceConversationId()`，通过会话唯一 ID 精确比对，不以当前激活选中项或光标位置瞎猜。`ConversationRow.voice.test.tsx` 验证通过。 |
| **UI-05** | RMS 连续变化不重渲染全部 row；活动归属改变只影响必要行，不能按音量订阅每行 | ✅ 通过 | 列表解耦：不再向行组件传递高频 `useVoiceSnapshot()`，仅向 `SessionList` 传入低频派生 store `useActiveVoiceConversationId()`，PCM 音频帧波动绝不触发会话列表渲染。 |
| **UI-06** | 活动通话手工删除受保护；查看/改名正常；挂断与任务取消独立，A 的迟到控制不作用 B | ✅ 通过 | 会话删除入口校验当前活跃会话 ID 并受保护；查看与改名不受影响；任务取消与语音挂断通过独立 handler 分派。`ComposerSubmitCluster.behavior.test.tsx` 验证通过。 |
| **UI-07** | 首页 icon-only；双语 tooltip/aria 随 Start call / Return to call 变化；草稿不丢、不自动发送 | ✅ 通过 | `GuidActionRow.tsx` 移除文字 label，仅保留电话按钮；tooltip 与 aria-label 根据 `isVoiceActiveElsewhere` 动态切换为中英双语的“开始通话”/“返回通话”。草稿内容完整保留。 |
| **UI-08** | 左上胶囊不占旧底部状态行、不遮首行/技能/工具，普通会话没有空胶囊区域 | ✅ 通过 | `NomiSendBox.tsx` 移除旧底部 footer 状态行，通过 `SendBox` 的 `statusSlot` 挂载在左上方；普通会话未开启语音时不渲染胶囊。`VoiceComposerStatus.test.tsx` 验证通过。 |
| **UI-09** | syncing/connecting/idle/active elsewhere 明确区分；合成中不冒充播放，任务忙不冒充麦克风忙 | ✅ 通过 | `VoiceComposerStatus.tsx` 中细分 `failed`、`isConnecting`、`isElsewhere`、`snapshot.phase` 状态，图标与文字严格对应；`VoiceComposerStatus.test.tsx` 中英双语断言全部通过。 |
| **UI-10** | 普通胶囊没有空展开；审批可见，队列逐项取消、unknown 核对、真实错误/历史重试和小挂断均可达 | ✅ 通过 | 胶囊在普通状态下为只读 `role="status"`；有审批时弹出 `<details>` 审核，支持队列逐项取消与历史重试；小挂断按钮通过 `ComposerSubmitCluster` 保证随时可达。 |
| **UI-11** | 中英、明暗、最窄支持宽度、100/125/150% 系统缩放与减少动效布局合理；不靠颜色区分活动态 | ✅ 通过 | 活跃状态通过 `AudioLines`（声波动态图符）与 `Voice`（普通图符）实现形状层面的清晰区分，不仅靠色相；深浅主题语义 token 适配并通过 `check:theme` 校验。 |
| **UI-12** | 通话强制临时显示正确 owner 桌宠，沿用耳机/动作/透明首帧与保存位置；结束后恢复原偏好，不开旧调试窗 | ⚠️ 逻辑通过 / 原生未签核 | 桌面端桌宠耳机 badge 与临时拉起逻辑由 `useCompanionWindowsSync.ts` 与 `companion/index.tsx` 维护，组件级逻辑测试通过；原生桌面窗口透明及物理渲染已如实归档为物理边界（详见 `BLOCKED.md`）。 |

---

## 2. 最终质量门禁与测试执行记录 (V4)

### 2.1 前端测试套件 (21 文件定向与基线)
```bash
bun test src/renderer/pages/voiceSession src/common/adapter/voiceCompanion.test.ts src/renderer/pages/conversation/platforms/nomi/nomiSendBoxDispatch.test.ts src/renderer/pages/conversation/platforms/nomi/resetSingleFlight.test.ts src/renderer/pages/conversation/SessionList/ConversationRow.structure.test.ts src/renderer/pages/conversation/SessionList/ConversationRow.voice.test.tsx src/renderer/pages/conversation/SessionList/utils/sessionCapabilityItems.test.tsx src/renderer/pages/conversation/SessionList/hooks/useConversationListSync.test.ts src/renderer/components/chat/ComposerSubmitCluster.behavior.test.tsx src/renderer/components/chat/ComposerSubmitCluster.structure.test.ts src/renderer/pages/settings/VoiceSettings.structure.test.ts src/renderer/pages/settings/VoiceDialogueStatus.test.tsx src/renderer/pages/nomi/workspace/tabs/OverviewTab/OverviewModelPolicy.test.ts
```
- **测试结果**: **160 pass / 0 fail / 1218 expect() calls** (耗时 2.84s)。

### 2.2 后端 Rust 测试套件
```bash
cargo test -p nomifun-app --lib voice_
cargo test -p nomifun-conversation --lib voice
```
- **nomifun-app**: **90 passed; 0 failed** (耗时 55.61s)。
- **nomifun-conversation**: **9 passed; 0 failed** (耗时 2.75s)。

### 2.3 TypeScript 类型差集检查
```bash
bun run typecheck
```
- **结果**: 全仓错误总数精确等于 **61** 条既有基线错误（`videoCanvas`, `WallpaperSection`, `MessageText` 等非语音历史文件）。
- **差集**: 语音交互 rev3 新增/修改的代码（`VoiceComposerStatus.tsx`, `ConversationRow.tsx`, `voiceCompanion.ts`, `GuidActionRow.tsx`, `rev3Probes.test.tsx` 等）**0 错误**。

### 2.4 静态质量与设计系统门禁
- `bun run check:i18n`: **通过** (`i18n-keys.d.ts is up to date (11762 keys, 55 modules)`).
- `bun run check:theme`: **通过** (`✓ 6 theme css files clean`).
- `bun run check:icons`: **通过** (`389 file(s) using @icon-park/react clean`).
- `bun run check:button-layout-contract`: **通过** (`scanned 948 TSX files`).
- `bun run check:motion`: **通过** (`check:motion ok`).
- `bun run check:dead-css`: **通过** (本次修改文件净新增 0 个死 CSS 工具类，`ConversationRow.tsx` 与 `VoiceComposerStatus.tsx` 全部使用规范 UnoCSS 规则类).

### 2.5 构建检查
- `bun run build:ui`: **通过** (14405 modules transformed, exit code 0, 耗时 1m 54s)。
- `cargo check --workspace`: **通过** (全工作区所有 Rust crate 编译通过, exit code 0, 耗时 4m 28s)。

---

## 3. 三类反例修复前 RED 与修复后 GREEN 对照

| 反例场景 | 修复前表现 (RED) | 修复后表现 (GREEN) | 验证文件与断言依据 |
| --- | --- | --- | --- |
| **反例 1: 首页错误复用旧会话** | `GuidPage.tsx` 派发 `{ kind: 'default_workspace' }` / `{ kind: 'workspace' }`，导致首页点击通话复用最后一次语音会话的历史记录；断言 `guidSource.includes("kind: 'new_voice_conversation'")` 失败 (exit 1)。 | `GuidPage.tsx` 显式派发 `{ kind: 'new_voice_conversation' }`，由后端分配全新独立占位会话，有效输入前不污染历史，接纳后成为独立新记录。 | `ui/src/renderer/pages/voiceSession/rev3Probes.test.tsx` (Probe 1) 测试通过 (exit 0)。 |
| **反例 2: 关闭与迟到接纳竞争** | 缺失 `voice_lifecycle` 服务端状态屏障，前端 `voiceCompanion.ts` 无生命周期声明；迟到的转录或消息落盘可能复活已关闭的空会话或覆盖新建会话。断言缺少状态契约 (exit 1)。 | 前端与后端 DTO 增加 `voice_lifecycle`；SQLite 原生过滤 `discarded`；后端服务在 `append_voice_message` 时强制核验若处于 `discarded` 显式拒绝，若有有效输入原子转换为 `retained`。 | `ui/src/renderer/pages/voiceSession/rev3Probes.test.tsx` (Probe 2) 与 `crates/backend/nomifun-conversation` 9 项单元测试通过 (exit 0)。 |
| **反例 3: More 事件冒泡与菜单互斥** | `ConversationRow.tsx` 中的 More 仅为普通的 `<span>` 元素，无语义 button 与 `aria-haspopup`，点击时冒泡触发行点击导航；菜单浮层与外层 hovercard 发生重叠冲突。 | More 改为语义 `<button type="button" aria-haspopup="true">`，点击时调用 `e.stopPropagation()` 阻止触发行点击；打开菜单时自动抑制外层 Popover 悬浮卡片。 | `ui/src/renderer/pages/voiceSession/rev3Probes.test.tsx` (Probe 3) 与 `ConversationRow.structure.test.ts` 6 项测试通过 (exit 0)。 |
