> **接续状态更新**：本报告 F1–F3 已实施定向修复并完成代码验证，另补清理 epoch 与下一句同时到达的竞态。最终事实、证据与同 renderer 的实现边界见 [修复验证记录](REPAIR-VERIFICATION-2026-10-24.zh.md)。以下保留修复前的独立审查全文。

# 语音交互 rev2 独立复审与补充执行要求

> 日期：2026-10-24。范围：`feat/voice-next`，HEAD `7de276e61e93836365c5a1c7e006c557ce3e3c6d` 上的当前未提交实现及其方案、报告和测试。
> 本轮仅增加本审查记录及忽略目录中的反例/日志，没有修改业务代码、正式测试、既有方案、数据库或用户数据，没有提交、推送或切分支。
> 判定：**needs-attention**。原测试通过的数字有证据，但存在三类未闭环边界，不能仅凭通过总数签核全部验收项。

## 1. 事实与审查边界

- 实时快照与持久记录异步到达，挂载状态不能成为跨挂载数据失效的唯一依据 — physics。
- 首页入口使用 `current_selection`，会话输入框使用 `voice_conversation`；当前 adapter 按原始 target 严格比较未决请求 — convention。
- 继续使用同一 Conversation、消息列表、已有启动幂等合同的成本低于另建语音聊天系统 — economics。
- 规范侧：本次受审 diff 未新增依赖、migration 或 agent harness 改动；现有脏工作区完整保留。以下三项是 Spec/正确性问题，不是要求重建架构。
- 没有证据证明这三项会把数据库记录写进另一工作区；已复现的重置问题属于临时文字重新显示。不要据此迁移、删除或重写用户历史。

## 2. 攻击点与可复现证据

### F1 · P1 · evidence：清理后的临时轮次在重挂载后复活

代码：

- `ui/src/renderer/pages/voiceSession/liveVoiceTurn.ts:30–35`：只有相同 scope 内观察到 epoch 改变才加入 `blockedTurns`。
- 同文件 `:48–52`：重挂载等待核对的逻辑只约束 projected 轮次；未投影的旧问答会直接成为 observed/live。
- `ui/src/renderer/pages/conversation/platforms/nomi/NomiChat.tsx:78`：屏障只存在于当前组件的 `useRef(createLiveVoiceHistory())`。
- `ui/src/renderer/pages/conversation/Messages/conversationMessageCoordinator.ts:407–460`：最后一个 consumer 释放后，coordinator 和 epoch 可销毁；其原设计假设重进页面读取数据库足够，而语音还会合并 worker 缓存。

反例：当前 session 有尚未投影的用户原文/部分回答 → 清理并递增 epoch → 当前视图正确隐藏 → 释放真实 coordinator consumer 并重建视图 → 同一 worker 旧快照重新送达。预期无旧 live 行，实际出现两行旧问答。另一个最小反例中，暂时无快照使 scope 变化，同样丢失屏障。

现有正式 reset 用例只覆盖同一 history 对象持续存在；现有 remount 用例只覆盖已经 projected 的旧轮次。这两者分别通过不代表组合成立。对应 **RV-07、HI-06** 需要补强。

最小执行要求：

1. 让旧轮次失效的权威边界跨页面挂载仍然成立，严格绑定 Conversation/session/turn；优先复用宿主/worker 生命周期，使同一次清理能废止旧易失问答及迟到输出。
2. 不仅在 renderer 当前视图内隐藏；不引入全局无界退休 Set，不清除仍合法的持久历史，也不顺带取消已接受后台任务。
3. 迁移正式反例：同视图、离开再进入、暂时缺快照再返回、清理后的新轮次正向对照；生成已发生而写入被 epoch 拒绝时也不得复活旧 live。
4. 将 RESULTS 中 RV-07 的结论明确补为“同视图已有覆盖，跨挂载失效待修复”，修复实跑后再升级结论。

### F2 · P2 · evidence：首页未决启动不能由目标会话输入框恢复

代码：

- `ui/src/common/adapter/voiceCompanion.ts:341–343`：未决请求只允许相同原始 target 和 mode 重试。
- 同文件 `:370–373`：首页 `current_selection` 收到 `starting` 后即打开返回的 Conversation。
- `ui/src/renderer/pages/conversation/platforms/nomi/NomiSendBox.tsx:281`：仅将 target 为 `voice_conversation` 的未决请求视为当前会话启动待核对。
- 同文件 `:394`：会话内重试总是发送 `voice_conversation`。

反例：模拟首页 `current_selection` 返回 `status: starting` 和目标 Conversation ID；暂未收到有效 binding 快照；进入该会话后点击通话。实际 adapter 在本地返回 `voice_call_starting`，没有再次发送原请求；页面也未显示属于本次启动的恢复状态。重新从原首页 target 调用才可重用 request_id。

精确接口事实：当前 `companion_start_call_handler` 成功返回 HTTP **200** 的 JSON，`starting` 是正文状态。本反例使用相同形状；不依赖 HTTP 202。原测试名称虽含 202，其 mock 也使用默认 HTTP 200。

正常 SSE 稍后送达合法 binding 时可以自动解除；这里失败的是延迟、漏帧或断线期间要求保留的手动恢复路径。对应 **RV-06、UX-02**。

最小执行要求：

1. 将原始启动请求与服务返回的目标 Conversation 关联；目标会话输入框能识别该未决启动。
2. 恢复时重用原始 request_id、target、mode，不能将 target 改写后再次提交，也不能清空未决状态另建一个请求。
3. 保留真实其他 Conversation 的启动冲突保护。没有可信返回目标时不能任意认领某页。
4. 增加首页/桌宠入口 → `starting` → 路由 → 会话内恢复的组合用例，以及跨真实目标仍应拒绝的正向安全对照；不仅重复调用同一个 adapter target。

### F3 · P2 · evidence：折叠侧栏丢失语音类型标记

代码：`ui/src/renderer/pages/conversation/SessionList/ConversationRow.tsx:270` 的 `isVoice && !collapsed` 直接移除了标记；`SessionList/index.tsx:765–783` 确有生产折叠分支使用相同行组件。现有 `ConversationRow.voice.test.tsx:27` 固定 `collapsed: false`。

反例：同一改名后的真实语音元数据行，在展开模式可渲染声波，传入 collapsed 后没有任何语音类型标记。这是 DOM 层遗漏，不需要原生 DPI 实测才能确认。对应 **LI-02** 和方案 §3.4“折叠保持类型可感知”。

最小执行要求：在既有紧凑行内保留静态语音类型信号及双语 tooltip/aria；继续给 spinner、未读和操作让位，不新增每行声学订阅。将 collapsed 加入实际渲染参数矩阵，覆盖重命名、运行中、未读与普通会话对照。

## 3. 测试数字核对与本轮独立实跑

所有本轮日志/反例位于忽略目录 `build.noindex/voice-workspace-display-review/`；六个关键受审源码 SHA256 见 `review-20261024-source-receipt.json`。

| 项目 | 本轮结果 | 证据/判读 |
| --- | --- | --- |
| 宿主 `cargo test -p nomifun-app --lib voice_ --no-default-features` | 90 pass / 0 fail / 3 原有 ignored，63.94 秒 | `review-20261024-host.txt`，与报告相符 |
| Conversation `cargo test -p nomifun-app --test voice_dialogue_integration --no-default-features` | 26 pass / 0 fail，1.92 秒 | `review-20261024-integration.txt` |
| worker + terminal：`cargo test -p voice-companion -p nomifun-voice-terminal` | worker 47 pass / 1 ignored；terminal 49 pass / 1 ignored；均 0 fail | `review-20261024-worker-terminal.txt`。组合命令发生 feature 合并，多运行 terminal 的 vt_0 八项；原独立命令报告 41 项并非伪造 |
| worker 特征套件：`cargo test -p voice-companion --features voice-skeleton --test voice_streaming --test voice_confirmation --test voice_session` | 14 + 6 + 14 = 34 pass / 0 fail / 0 ignored | `review-20261024-worker-feature.txt`。原默认命令这些文件均运行 0 项；本轮补跑，不将模拟 PCM/HTTP/sink 称为物理音频验收 |
| 前端原 18 文件组 | 142 pass / 0 fail / 1017 assertions，3.39 秒 | 本轮工具完整输出；对应既有 `fix-ui-closure.txt` 的相同文件组，数字一致 |
| 完整 TS 编译 | exit 2，61 项诊断；与上一轮最终诊断逐条比较差异 0 | `review-20261024-typecheck.txt` / `review-20261024-typecheck-compare.json`。不是 typecheck 全绿 |
| 新对抗探针（5 项） | 1 pass / 4 fail / 9 assertions | `review-20261024-boundaries.test.ts` / `review-20261024-boundaries-final.txt`。四个失败归于上述三类问题，包含正常同视图/新轮次/展开标记的对照 |
| `git diff --check` | exit 0 | 只有既有 CRLF 转 LF 提示，无空白错误 |

新反例重跑：

```powershell
bun test --cwd ui ../build.noindex/voice-workspace-display-review/review-20261024-boundaries.test.ts
```

本轮核对了原 54 项桌宠/首页、86 项共享历史、桌面 feature 编译和 UI build 的原始日志，数量/成功结果与报告相符；没有把读取日志称为本轮重新构建。原生窗口/高 DPI、物理麦克风/扬声器/AEC 仍未签核。动态布局 EBUSY 与 dead-css 原有失败也仍是未通过，未放宽门禁。

## 4. 幸存结论与补充执行顺序

继续保留：单一持久 Conversation、逐轮 session 身份、按会话选择通知、独立用户/助手消息接替、输入框阶段与必要控制、静态类型声波、诊断移入高级设置。没有证据要求增加权限模式、流式 ASR、重建消息表或合并普通/语音的执行队列。

建议在现有 E1–E4 内收口，不再新增一套长期路线：

1. **E1 补强 F1**：先固化 RED，再完成跨挂载失效及新轮次恢复；保留现有防回写和无二次执行合同。
2. **E2 补强 F2/F3**：贯通入口到会话的原请求恢复，补齐 compact 标记；单主键和跨目标冲突保护回归。
3. **E4 更新证据**：将忽略目录反例迁入对应正式 suite，保留旧通过用例；将上述带 `voice-skeleton` 的 34 项特征检查加入本次语音事件改动的验证命令。
4. **真实组件异步覆盖**：审批/取消的 SSR 只证明初始按钮和文案，不能证明迟到 callback 已安全落地。增加 deferred Promise 下的点击 A → 切 B → 返回 A 响应，断言 B 的 pending/error 和任务身份不变；再执行至少一条首页 starting → 路由 → 恢复链。可代码自动化，不要求用户反复用麦克风测试。
5. **最后同步报告**：按 HI-06、RV-06、RV-07、LI-02 分项更新；测试数量、feature、忽略项、原生未验收边界分别记录。不把总数变多当作遗漏已修复。

本记录是独立复审补充，保留已有 IMPLEMENTATION、RESULTS、方案与历史附录原文。审查没有授权任何自动后续任务或 Git 交付。
