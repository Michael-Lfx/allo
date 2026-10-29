# 悬浮语音条与积分隔离 · 首轮独立复审

2026-10-25。执行会话 `ses_e8e806323ffeFvfxM1muzC4e4J` 首轮 exit 0，原 exec 6131 已确认结束。本轮不接受“方案已闭环”：父审查在真实新增实现上补了三个反例，均失败，已安排同一 OpenCode 会话继续修复。没有提交、暂存、推送或分支操作。

## 已确认保留的成果

核对 `carried-baseline.json`：HEAD 未变、暂存为空；接手前 13 个文件中 11 个逐字未变，只有两个积分前端文件按方案修改。五个 Rust 文件及原始 ASR/投影/左右排版修复均完整保留。独立悬浮条已作为 SendBox 兄弟节点挂载；语音判定已从通用积分模块移出。这些事实不等于所有边界已验证。

## 必须补修

1. **[P1] 通用门禁拒绝可吞掉合法语音查询。** `runTurnCreditQuery` 在普通入口资格校验之前按 Conversation+turn 放入共享 inflight。普通本地主模型入口先进入、语音归属入口紧随时，后者直接复用前者的 null 结果，根本没有查询实际 usage；语音协调器又已将该轮记为 attempted，可能长期不展示积分。真实共享核心反例返回 `voiceUsage = undefined`，预期 120；不是只检查代码字符串。
2. **[P1] 积分协调缺少逐轮 session 校验。** `collectVoiceBilledTurns` 校验了 snapshot 与 binding，但没有核对 `turn.session_id`。在当前绑定内残留旧轮次时，会把旧语音轮次当成本次 Conversation 的积分来源。已有聊天投影 `liveVoiceTurn` 明确过滤这类轮次，新增积分入口必须遵循同一边界，不能对缺少 session 的旧临时轮次猜归属。
3. **[P2] PCM 停滞仍宣称能听/唤醒。** 生产 worker 在持续无音频帧时发布 `audio.capture_state = no_pcm`，不会必然同时改变 phase/acoustic_ready。新映射只识别 closed，故 no_pcm+listening 返回 wave。需要按实际采集健康显示准确、可恢复的状态，不能把没有 PCM 当成静音待机。

反例已写入正式测试：`fetchTurnCredits.runtime.test.ts`、`useVoiceTurnCredits.test.ts`、`voiceCallDockState.test.ts`，测试名均以 `independent review:` 开头。日志在本任务证据目录：`independent-isolation-red.txt` 为 **9 passed / 2 failed**，`independent-capture-red.txt` 为 **12 passed / 1 failed**。

## 还需补行为证据的边界

- `historyFailed` 且 `snapshot = null` 时，状态映射允许详情，但组件使用 `detailsOpen && snapshot` 阻止恢复面板挂载，历史重试入口不可达。应在真实组件 DOM 中制造该场景，修复后验证展开、重试、aria-controls 与焦点。
- 语音积分目前只从活跃 snapshot 收集；普通本地主模型门禁已经恢复。重开历史语音会话且没有活跃 snapshot、也没有缓存 usage 时，持久消息即使有正确 turn_id 也缺少语音专属回填路径。应从当前已加载持久消息的真实归属验证可恢复查询，不依赖活动页常驻或重新通话，不修改普通路径门禁。否则必须明确标为未闭环。
- 报告 FD-08 把四文件合计 70 项误写成 liveVoiceTurn 70 项；FD-09 200% 缩放和完整应用覆盖不全，不应给无条件通过。独立 Chrome 探针可以证明该探针夹具的 DOM，不能证明整个真实 Flowy 页面。

## 后续范围

沿原方案与原授权修复以上问题，保留三个 RED 日志和正式反例；补对应回归与准确文档。仍不扩展 AEC、硬件声学、鉴权、计价、Agent loop 或模型依赖。只有补修退出并完成独立复审后才签核/关闭定时检查；当前尚未签核。

## 补修后的独立复审与收口

补修 `exec 24632` 已 exit 0，父审查确认该进程结束后才运行独立测试和修改。以下为后续事实，不改写上面的首轮失败结论。

- 原三项反例已独立复跑通过：拒绝的普通门禁不再发布 null inflight；语音协调逐轮核对 session；no_pcm 不再冒充可以听或唤醒。普通入口恢复云模型门禁，语音协调复用同一无语音状态依赖的查询核心。
- 无快照历史失败面板已实际操作验证；historyMessages 来自当前 NomiSendBox 的已加载消息列表，回填最多 12 个明确持久轮次，并继续按 Conversation/turn 去重，不增加语音识别或 LLM 调用。
- 父审查额外补出“没有 binding/session 且 capture=closed，仍宣称等待唤醒”的反例，`parent-unbound-capture-red.txt` 为 **13 passed / 1 failed**。仅收紧两处 waitingWake 判断，要求 capture_state=receiving；原有健康采集和 KWS 条件保持。该补修由父审查完成，没有重启 OpenCode。
- 最终独立行为回归 `parent-final-green.txt` 为 **112 passed / 0 failed / 507 assertions / 8 文件**，覆盖四项父反例、普通/语音积分入口、持久历史、排版与 live/durable 接替。
- 独立重新编译当前源码并运行 Chrome 探针：normal **58/58**、Reduced Motion **58/58**，无 console/page 错误。结果位于 `browser-probe/parent-browser-result.json`，日志 `parent-probe-run.txt`。新建浏览器已关闭（`parent-browser-cleanup.json` connected=false），临时 node_modules 链接已按身份核对移除。首次重编译缺少探针自身依赖路径，留有失败日志；复用现有 UI 依赖链接后通过，没有安装依赖。
- 最终 TypeScript 与开工基线逐条比较：**61 对 61，新增 0、移除 0**，见 `parent-typecheck-comparison.json`。最终生产前端构建 **exit 0**，见 `parent-build-ui.txt` 与收尾回执 `parent-build-exits.json`。
- HEAD 仍为 `cd5a904af77493faceba1c41a9df85220666b2a5`，暂存为空。接手的 13 文件中，9 个逐字不变，4 个为积分修改或文档追加；五个 Rust 文件及原排版/持久投影修复均完整保留。没有新增未授权目录或业务域改动。

本轮代码审查与定向修复可以收口，剩余 FD-01/02/04/05/09 的完整 SPA、真实设备/daemon、200% 原生缩放，以及 FD-07 真实云端金额，继续明确为部分或未验收。独立 DOM 夹具不代表完整 Flowy 页面，既有 dead-css 和结构断言失败也未被改写成通过。没有提交或推送。

## 后续全面复审与积分恢复补修

同日进一步复审发现未被上述 112 项覆盖的多轮并发持久写回和失败后恢复边界；全面复审 RED 探针保留于 `build.noindex/voice-workspace-display-review/full-review-20261025/`。用户随后授权全部修复并提交，正式迁入反例先得到 7 passed / 3 failed，再修复为语音及积分相关回归 172 passed / 0 failed，并以真实 React/Chrome 验证 11 条 hook 生命周期行为。完整修复和最新门禁见 [积分写回与恢复修复记录](CREDIT-WRITEBACK-RECOVERY-REPAIR-2026-10-25.zh.md)。本段不回写、删除此前审查事实。
