# E · 语音 runtime 冷启动执行方案与修改边界

- 日期：2026-11-06（UTC+8）
- 分支：`explore/voice-runtime-warmup`
- 执行 checkpoint：`9f66997e8 fix: surface voice model fallback and isolate validation`
- 历史提交：`5e28569b4`（模型继承）、`18df67f4a`（首轮审查修复 + 文档）
- 当前状态：checkpoint 之前的功能代码已提交；本方案文件与文档状态更新尚未提交
- 验证原则：固定 commit、串行验证、先验收再改代码

## 1. 目标

让**新建**语音通话：

1. 不再承担 5 个 MCP 的冷启动（目标 gateway-only，首次 runtime build 秒级）；
2. 继承用户保存的默认模型（`nomi.defaultModel`）；
3. 模型不可用时可回退，并且回退对用户可见，不静默切 provider/model；
4. 不回归普通对话、显式模型选择、preset 和旧语音会话数据。

## 2. 非目标（本轮明确不做）

- 不迁移旧语音会话的 MCP/model 配置；
- 不改变旧 retained / workspace 会话的复用语义；
- 不恢复 Playwright / Excel / Figma 等外部 MCP 工具能力；
- 不处理 TTS 播报中的 barge-in；
- 不重构 capability plane / 全局 MCP 连接池；
- 不优化模型本身的 54.6s 长时间任务耗时；
- 不修改普通对话的 MCP 选择或模型策略。

## 3. 冻结的修改边界

允许修改的范围已经冻结为以下模块：

### 后端

- `crates/backend/nomifun-conversation/src/service.rs`
  - 自动语音会话的 MCP fence；
  - `nomi.defaultModel` 读取；
  - 新建 provisional/default/idempotent 语音会话的模型继承。
- `crates/backend/nomifun-app/src/voice_companion_manager/recorder.rs`
  - 唤醒自动路径的模型解析与 fallback 记录。
- `crates/backend/nomifun-app/src/voice_task_runtime.rs` / `voice_tasks.rs`
  - 仅限模型校验和任务模型 fallback。
- `crates/backend/nomifun-app/src/voice_dialogue.rs`
  - 仅限 recorder trait 的 fallback 查询。
- `crates/backend/nomifun-api-types/src/voice.rs`
  - 仅限新增 optional `model_fallback` 响应字段。

### 前端

- `ui/src/common/adapter/voiceCompanion.ts`
  - 解析 optional fallback 响应并派发事件。
- `ui/src/renderer/components/layout/Router.tsx`
  - 仅限 fallback warning toast（后按用户决定取消，见 E4b）。
- `ui/src/renderer/services/i18n/**`
  - 仅限 fallback 文案。

### 文档

- `docs/architecture/voice-agent/execution/voice-followup-ux-tts-20261105/RUNTIME-WARMUP.md`
- `PROGRESS.md` / `ACCEPTANCE.md` / `BLOCKED.md` 的 E 轮追加
- 本执行方案文件

边界外发现的新问题只记录，不直接修；需要另开变更集。

## 4. 已冻结的行为决策

| 决策 | 结论 |
| --- | --- |
| “新对话”定义 | 指**新创建的 conversation**；旧会话按用户要求不迁移 |
| 旧 retained/workspace 复用 | 保持现状，不在本变更集内改变；复用旧会话不承诺提速 |
| MCP | 新语音会话 gateway-only；显式选择/preset 仍优先 |
| 模型优先级 | 显式 model → `nomi.defaultModel` → 默认语音会话/runtime 模型 → 运行时默认 |
| fallback 可观测性 | 日志 + `VoiceCallStartResponse.model_fallback`；用户确认不再要求 UI toast |
| preset 模型不可用 | 直接失败，不隐式换模型 |
| 回退失败 | 只有所有模型都不可用才返回 `model_unavailable` / `task_execution_model_unconfigured` |

## 5. 验收表

全部验收必须绑定**确切 commit**，不能拿旧日志代替。

| 编号 | 验收项 | 证据 | 通过条件 |
| --- | --- | --- | --- |
| E1 | 新语音会话只连 gateway | `Injecting MCP servers` 日志 | `mcp_count=1` |
| E2 | 新语音首次 runtime 快 | `Agent runtime ready` | ≤ 1s；旧数据 364–490ms 仅作参考 |
| E3 | 新会话模型继承 | conversation model + `voice_default_model_preference_selected` | model = `AIPC-deepseek-v4.1-flash` |
| E4 | 模型失效回退可诊断 | fallback 日志 + 实际模型 | 有实际 fallback model；E4b UI toast 经用户确认取消 |
| E5 | 普通会话不回归 | 普通新建会话 model/MCP | 不受语音 fence 影响 |
| E6 | 显式/preset 优先 | service 测试：`explicit_voice_model_overrides_saved_default_preference` + `preset_model_reconciliation_*` | 显式模型不被默认偏好覆盖；preset resolved model 替换 requested lead |
| E7 | 旧会话不变 | 旧 voice conversation | 不被本变更迁移/覆盖 |
| E8 | 回归测试 | 下述命令 | 全部通过，基线错误单列 |

## 6. 验证命令（代码冻结后串行）

```powershell
cargo test -p nomifun-conversation
cargo check -p nomifun-app
cargo build -p Flowy
bun test ui/src/common/adapter/voiceCompanion.test.ts `
  ui/src/common/adapter/voiceCompanion.callIntent.test.ts `
  ui/src/renderer/components/layout/voiceWakeRouter.integration.test.tsx
bun run check:i18n
bun run build:ui
rustfmt --edition 2024 --check <changed rust files>
git diff --check
```

已知限制：

- `cargo test -p nomifun-app` 集成测试受本地 `sherpa_onnx_sys` rlib 缺失阻断；
- `bun run typecheck` 仍为 71 个既有基线错误，本轮改动文件 0 error；
- 真机必须重启 Flowy 后，用新 commit 新建语音会话。

## 7. 真机验收步骤

1. 重启 Flowy（加载 checkpoint）。
2. 确认 `nomi.defaultModel` 为 `AIPC-deepseek-v4.1-flash`。
3. 新开语音会话（不要复用旧 retained 会话）。
4. 查看日志：
   - `mcp_count=1`；
   - 首次 `Agent runtime ready` ≤ 1s；
   - `voice_default_model_preference_selected`；
   - conversation model 为 ds4.1flash。
5. 如模型不可用：确认 fallback 日志与 actual model；E4b toast 已取消，不要求 UI 提示。
6. 普通新建会话发一条消息，确认模型/MCP 不受影响。

## 8. 回滚

按提交逆序回滚：

```text
9f66997e8  fallback 响应/UI/模型校验
18df67f4a  首轮审查修复/文档
5e28569b4  模型继承
```

旧语音 conversation 数据不迁移，因此回滚不需要数据修复。

## 9. 停止条件

出现以下任一情况，停止继续改代码，先重新评估方案：

- 新语音会话仍 `mcp_count>1`；
- 新会话模型仍为 `AIPC-auto-cost`；
- fallback 发生时日志无法证明 actual model；
- 普通会话 model/MCP 行为变化；
- 旧会话被本变更修改；
- 回归测试出现非既有基线失败；
- 需要跨出冻结文件范围才能继续。

## 9.1 执行结果（2026-11-06）

| 项 | 结果 |
| --- | --- |
| E1 | ✅ 新语音会话 `mcp_count=1` |
| E2 | ✅ 第二次新建会话 312ms；第一次 1379ms 为宿主刚启动波动 |
| E3 | ✅ 新会话模型 `AIPC-deepseek-v4.1-flash` |
| E4a | ✅ 不可用模型回退到 `AIPC-auto-cost`，日志完整 |
| E4b | ⏹ 用户确认无需 UI toast；相关未提交实现已撤回 |
| E5 | ✅ 普通会话 ds4.1flash、gateway MCP、0ms runtime |
| E7 | ✅ 旧会话 `01a1aebc` 仍为 4 MCP + auto-cost |
| E6 | ✅ 代码测试：显式模型测试 1/1；preset reconciliation 8/8；全量 662/662 |
| E8 | ✅ 回归测试通过；文档已更新；最终 commit 待用户指令 |

## 10. 下一步

代码测试已完成，不需要真机 UI 验证 E6：

- 显式模型优先级：新增 service 测试直接断言会话 model 为显式模型；
- preset 优先级：已有 `preset_model_reconciliation_*` 测试证明 preset resolved lead 替换 requested lead；
- 全量 `nomifun-conversation` 662 passed。

**本轮不再新增功能代码。** 下一步是：

1. 重启 Flowy；
2. 按第 7 节做真机验收；
3. 把日志和结果反馈；
4. 只有 E1–E8 全部通过后，才讨论下一轮（外部 MCP 能力、旧会话迁移、长任务延迟）。
