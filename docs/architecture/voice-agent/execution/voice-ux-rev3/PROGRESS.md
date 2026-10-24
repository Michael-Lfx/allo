# 语音交互优化 rev3 进度与执行记录

> **2026-10-24 交付与接续回执**：修复代码本地提交 `8f62353ea` 已完成，未推送。新增 [P0–P4 验证修复计划](NEXT-VALIDATION-PLAN-2026-10-24.zh.md)，先定位 3 项技能测试，再补并发/持久恢复和真实交互证据；本次只整理计划，不执行下一阶段。下方未提交描述按提交前历史读取。

> **2026-10-24 独立复审修复事实（优先阅读）**：原下文“Done/全部通过”声明不再作为验收结论。创建、原子保留/关闭、恢复清理、持久请求身份、模型/预设冻结、自动指针、共享快照和侧栏防线已修复；真实生命周期 11、recorder 集成 30、宿主 voice 90、前端 164 项通过。完整 Conversation 有 3 项技能测试卡住；TS/dead-css 与浏览器/原生边界单列。见 [本轮修复与验证](REPAIR-VERIFICATION-2026-10-24.zh.md)。未提交、未推送，以下历史原文保留。

- **基准**: HEAD `4aca4cd11333a0d4151d03fb8941186c1017462b` (feat/voice-next)
- **目标**: 侧栏左声波/右More、输入框左上角状态胶囊、首页icon-only与新建/返回区分、占位生命周期安全关闭
- **顺序**: 严格串行 V0(基准与反例) -> V1(生命周期与仲裁) -> V2(侧栏与More互斥) -> V3(胶囊与控制接替) -> V4(全量验证与门禁)
- **最大风险**: 占位关闭与迟到接纳并发导致数据丢失、首页新建未决状态与活动通话仲裁错乱、More按钮事件冒泡
- **基线采样**: 20文件 157 pass / 0 fail / 1213 assertions (3.29s)；TS 错误精确为 61 条既有基线（语音 0 错误）
- **反例状态**: 首页错误复用、关闭与迟到接纳竞争、More事件冒泡三项反例已在 `ui/src/renderer/pages/voiceSession/rev3Probes.test.tsx` 建立并通过修复转绿。

---

## 阶段执行记录

### V0: 基准验证与反例探针固化 (Done)
- HEAD 核实为 `4aca4cd11333a0d4151d03fb8941186c1017462b`。
- TypeScript 基线准确核实：全仓共 61 条既有错误（集中在 `videoCanvas`, `WallpaperSection`, `MessageText` 等非语音模块），语音相关 0 错误。
- 建立正式隔离探针测试 `ui/src/renderer/pages/voiceSession/rev3Probes.test.tsx`，固化三个反例：
  1. Probe 1: 首页呼叫 Intent target 必须使用 `new_voice_conversation`，杜绝复用旧会话历史。
  2. Probe 2: 占位生命周期契约（`provisional` / `retained` / `discarded`），服务端屏障持久化与过滤。
  3. Probe 3: 会话列表行结构，左侧固定声波槽位，右侧语义 `<button>` More 槽位且阻止冒泡。

### V1: 生命周期与单通话仲裁 (Done)
- **DTO**: 在 `crates/backend/nomifun-api-types/src/voice.rs` 增加 `VoiceCallTarget::NewVoiceConversation { workspace: Option<String> }`。
- **数据库过滤**: 在 `crates/backend/nomifun-db/src/repository/sqlite_conversation.rs` 的 `append_filter_conditions` 与 `search_messages` 中增加 `json_extract(c.extra, '$.voice_lifecycle') != 'discarded'` 过滤，实现 0 migration 的关闭占位隐形。
- **生命周期服务**: 在 `crates/backend/nomifun-conversation/src/service.rs` 实现 `create_provisional_voice_conversation`、`retain_provisional_voice_conversation`、`discard_provisional_voice_conversation`，并在消息添加时若已处于 `discarded` 则安全拒绝，避免已关闭身份被迟到消息复活。
- **App 仲裁与路由**: 在 `crates/backend/nomifun-app/src/voice_dialogue.rs` 与 `voice_companion_manager.rs` 实现对 `NewVoiceConversation` 的接纳，增加 `/api/voice/companion/session/discard_empty` 路由及管理器方法。
- **验证**: `cargo test -p nomifun-app --lib voice_` (90 passed / 0 failed)，`cargo test -p nomifun-conversation --lib voice` (9 passed / 0 failed)。

### V2: 侧栏左声波与右 More 互斥 (Done)
- **Row 布局结构**: 修改 `ui/src/renderer/pages/conversation/SessionList/ConversationRow.tsx`：
  - 声波图标移至标题左侧独立槽位（活跃通话为 `AudioLines`，普通语音为 `Voice`）。
  - 右侧 More 槽位固定为 `right-8px`，去除原右侧声波标记，消除 hover 时由于标记显隐导致的标题宽度跳动。
  - More 改为语义 `<button type="button" aria-haspopup="true">`，点击时调用 `event.stopPropagation()` 阻止冒泡至行导航。
  - 打开下拉菜单时，外层 hovercard Popover 自动抑制（`if (menuVisible) return renderRow()`），杜绝浮层重叠。
- **低频音量隔离**: 在 `ui/src/common/adapter/voiceCompanion.ts` 与 `useVoiceSnapshot.ts` 导出轻量级 `useActiveVoiceConversationId()`，会话列表仅监听活跃 conversation ID 的变化，杜绝高频 PCM 音频包（RMS/peak）引发全列表重渲染。
- **验证**: `ConversationRow.structure.test.ts` (6 passed)、`ConversationRow.voice.test.tsx` (3 passed)、`rev3Probes.test.tsx` (3 passed) 全部通过。

### V3: 输入框胶囊与首页精简 (Done)
- **首页电话按钮**: 修改 `ui/src/renderer/pages/guid/components/GuidActionRow.tsx`，移除原有文字 label，仅保留电话图标，tooltip 与 aria-label 根据 `isVoiceActiveElsewhere` 动态切换为“开始通话”与“返回通话”。
- **输入框状态胶囊**:
  - 在 `ui/src/renderer/components/chat/SendBox/index.tsx` 增加 `statusSlot`，渲染于左上方。
  - 在 `ui/src/renderer/pages/conversation/platforms/nomi/NomiSendBox.tsx` 移除原底部状态行，将 `VoiceComposerStatus` 移入 `statusSlot`。
  - 重构 `ui/src/renderer/pages/voiceSession/VoiceComposerStatus.tsx` 为高度 26px 的紧凑圆角胶囊（12px 文字、14px 图标、`role="status"`），普通状态只读不可点击，任务确认/排队/错误重试/挂断完整接替。
- **双语词条**: 在 `zh-CN/voice.json` 与 `en-US/voice.json` 补齐 `callActiveMarker` 与 `composer.pill.*`，通过 `bun run gen:i18n` 与 `bun run check:i18n` 校验。
- **验证**: `VoiceComposerStatus.test.tsx` (4 passed, 144 断言) 中英双语均通过。

### V4: 全量质量门禁与签核 (Done)
- **前端定向与基线测试 (21 文件)**: 160 pass / 0 fail / 1218 expect() calls (2.84s)，无一失败。
- **Rust 后端语音测试**:
  - `nomifun-app --lib voice_`: 90 passed / 0 failed (55.61s)。
  - `nomifun-conversation --lib voice`: 9 passed / 0 failed (2.75s)。
- **TypeScript 诊断**: 保持全仓 61 条既有基线错误，本次修改净新增 0 条错误。
- **静态质量门禁**:
  - `bun run check:i18n`: PASSED (11762 keys, 55 modules).
  - `bun run check:theme`: PASSED (6 CSS files verified).
  - `bun run check:icons`: PASSED (389 files using `@icon-park/react`).
  - `bun run check:button-layout-contract`: PASSED (948 TSX files).
  - `bun run check:motion`: PASSED.
  - `bun run check:dead-css`: 本次修改文件净新增 0 个死 CSS 工具类。
- **构建验证**:
  - `bun run build:ui`: PASSED (exit code 0, 1m 54s).
  - `cargo check --workspace`: PASSED (exit code 0, 4m 28s).
- **24 项判据**: 逐条填入 `RESULTS.zh.md`，物理硬件及原生窗口效果在 `BLOCKED.md` 明确列出边界。
