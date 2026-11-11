# rev2 复审三项问题修复与验证

> 2026-10-24；分支 `feat/voice-next`，HEAD `7de276e61e93836365c5a1c7e006c557ce3e3c6d`。本轮在既有脏工作区继续修复，未提交、未推送、未切分支。此前审查报告保留原文；其 F1–F3 当前状态以本记录为准。

## 1. 修复结果与选择

| 复审项 | 当前实现 | 正式回归 |
| --- | --- | --- |
| F1 清理后重挂载复活旧 live | 在清理请求前冻结原 Conversation/instance/session 的三轮身份，只有后端清理成功才提交失效；失效信息独立于页面卸载；暂缺快照不会撤销屏障；较旧请求迟到不能覆盖较新的成功清理 | liveVoiceTurn 的真实 coordinator 释放/重建、快照间断、新轮次与新 session、失败未提交、清理响应交错及迟到回答 |
| F2 首页 starting 后恢复失败 | pending intent 记录宿主返回的目标 Conversation；会话输入框订阅这个目标；同目标恢复仍发送原 request_id/target/mode，其他目标或显式不同 mode 继续拒绝 | adapter 首页 current_selection → starting → resolved Conversation 重试与冲突对照；真实电话按钮点击 |
| F3 折叠类型标记缺失 | 语音行在折叠时保留静态 Voice 标记；紧凑行使用 2px 间距；原双语 aria/tooltip 及任务状态保留 | 原双语 mobile/batch/menu/generating 渲染矩阵增加 collapsed true/false；普通会话对照仍无声波 |

F1 选择有界的**失效身份元数据**：只保存最近一次成功清理的一个 acoustic scope 和最多三个 turn ID，不保存用户文字、模型输出、持久消息或任务状态。普通退休/接替状态仍属于当前视图。另补齐清理 epoch 与下一句同时到达的竞态：只废止前一视图已观察到的轮次，不把新轮次加入旧屏障；其反例先出现 16 pass / 1 fail，修复后完整组通过，证据为 `repair-20261024-reset-race-red.txt`。它解决同一 renderer 中切页/卸载重进后丢失屏障的问题，不是另建消息 store，也不替代后端 admission epoch 的防回写/防重执行合同。

清理成功后即使原组件已经离开，失效提交仍完成；只对原页面继续显示的 toast/本地恢复操作增加生命周期检查。既有持久消息、模型上下文和后台任务仍由原有服务/API 生命周期管理；本轮没有新建声学上下文重置协议，也没有声称修改 worker 的 prompt context。

代码入口：

- `ui/src/renderer/pages/voiceSession/liveVoiceTurn.ts`：`prepareVoiceHistoryReset` 与所有 live 展示/接替消费者共同使用失效身份。
- `ui/src/renderer/pages/conversation/platforms/nomi/NomiSendBox.tsx`：clearContext/reset 成功提交接线及原页面异步保护。
- `ui/src/common/adapter/voiceCompanion.ts`、`voiceSession/useVoiceSnapshot.ts`：宿主解析后的 pending 目标、订阅与原请求恢复。
- `ui/src/renderer/pages/conversation/SessionList/ConversationRow.tsx`：紧凑行持久标记。

## 2. 实跑证据

日志目录：`build.noindex/voice-workspace-display-review/`。前一轮宿主 90、worker 47、Conversation 集成 26、终端及 34 项特征检查已由独立复审实跑。本轮修改范围为 frontend、正式测试与文档，未修改 Rust，未把前轮结果称作本轮重跑。

| 检查 | 本轮结果 | 日志 |
| --- | --- | --- |
| 修复前正式反例组 | 36 pass / 4 fail；重置重挂载、首页恢复、两种语言的折叠标记均 RED | `repair-20261024-red.txt` |
| 定向正式组 | 42 pass / 0 fail / 405 assertions | `repair-20261024-targeted.txt` |
| 完整语音前端 18 文件 | 147 pass / 0 fail / 1161 assertions | `repair-20261024-ui-final.txt` |
| 共享历史、清理、编辑重发 10 文件 | 100 pass / 0 fail / 369 assertions | `repair-20261024-history-final.txt` |
| 原审查反例及重置合同补充 | 24 pass / 0 fail；含原 5 个反例/对照 | `repair-20261024-boundaries.txt`；更新反例的清理触发点为现生产成功提交函数，原“不显示旧行”断言保留 |
| 完整 TS 检查 | exit 2；与复审基线相同的 61 项诊断，逐条 delta 0 | `repair-20261024-typecheck-delivery.txt` / `repair-20261024-typecheck-compare.json` |
| 生产 UI 构建 | exit 0，2m 2s；依赖 eval/大 chunk 原有警告保留 | `repair-20261024-build-final.txt` |
| 双语/主题/动效/图标/静态按钮布局/CSS scope/错误面/支持面/进程边界/浏览器边界 | 10 项均 exit 0；不得与动态像素布局混称 | `repair-20261024-gates.json` |
| 原生/物理 | 本轮未验证 | Tauri 首帧/DPI/物理麦克风、扬声器及 AEC 仍单列 |

共享历史扩展组最初发现 `editResubmitState.test.ts` 仍匹配已经移除的 `voiceConversation?.id!==conversation_id` 字符串。本轮把该过时写法断言替换为现有“缓存 ID 不匹配则 unknown；已知父级类型优先”的合同断言，保留 requires-reset disabled 断言。`nomiSendBoxDispatch.test.ts` 原有 unknown 不走普通队列的行为测试继续通过；没有放宽错误或状态门禁。

## 3. 真实组件点击验证

可维护的隔离测试入口：`ui/src/renderer/pages/voiceSession/tests/voiceControls.browser-probe.ts`。仅测试入口，无生产路由/调用方。使用现有 React、Arco、i18n 与 CloudAuth 测试 Provider；HTTP response 是本测试的 deferred fixture，不读取用户会话、不使用真实凭据、不申请麦克风权限。

在隔离 IAB 页面运行，实际挂载 React 组件并触发按钮，中文与英文各三条，共六条通过：

1. VoiceTaskQueue：点击 A 取消 → 复用组件切 B → 点击 B 取消 → A 失败迟到：B 仍 pending 且无 A 错误 → B 正常完成后才解锁。
2. VoiceTaskStatus：点击 A 一次审批 → 切 B 并审批 → A 失败迟到：不污染 B 的错误/pending；请求中的 Conversation/msg/call 身份仍各自属于原任务。
3. 首页 current_selection 返回 starting → 目标会话电话键显示核对启动 → 实际点击 → 与第一次完全相同的 request_id/target/mode 再次发送。

结果见 `repair-controls-iab-result.json`（根据实际页面 AX 结果落盘）。这证明真实组件事件/React 状态及 adapter 接线，不等同于启动整套 Router/后端，更不等于系统声学测试。

首个无头 Edge CLI 尝试没有返回 DOM，记录为未通过：`repair-controls-result.json` 中 report 为 null。没有重复扩散 Edge；其唯一隔离 profile 已删除，严格匹配该 profile 的进程为 0。之后使用 IAB 读取隔离页面，补齐 Provider 后六条链通过；首轮缺 Provider 的探针装配错误也未冒充产品缺陷。

复跑方式：从 `ui/` 用现有 Bun 编译上述测试入口，`target: browser`，开发版 React（定义 `process.env.NODE_ENV` 为 `development`）；将 bundle 作为隔离 HTML 的 module script，使用仅绑定 localhost 的静态服务打开。页面 `#voice-control-result[data-ready=true]` 返回 JSON；必须 `ok:true` 且六条 case 均存在。当前忽略目录保留 `repair-controls-build.mjs`、HTML 和结果以便本地复跑；该构建器指向上述仓库测试入口。

## 4. 验收矩阵增补与余留边界

- **HI-06 / RV-07**：新增“最后 consumer 卸载 → 重挂载”“暂缺快照”“失败清理不失效”“旧清理迟到不覆盖新 scope”“迟到旧回答仍隐藏”的代码证据；原后端 epoch 合同继续保留。
- **RV-06 / UX-02**：新增入口到目标 Conversation 的 pending 关联和真实电话键恢复证据；HTTP 200 + `status:starting` 与绑定成功仍分开，未绑定时不提供空身份挂断。
- **LI-02**：collapsed 已纳入双语正式 DOM 矩阵；原生实际 DPI 下的像素布局仍未签核。
- **RV-05 / UX-03**：新增双语真实 React 点击/迟到响应证据；不把原 SSR 文案检查当成按钮交互验证。
- 失效身份跨同一 renderer 的组件挂载保留；完整 renderer 重建或多 renderer 之间不靠这个内存元数据提供新的持久协议。宿主/worker 重启、模型上下文与持久历史继续按各自既有合同处理，不扩张本轮范围。
- TS 61 项、原 dead-css 基线、动态布局工具的既有边界保持明确；没有修改阈值、检查器、CI 或无关视频/账单文件。

本轮没有新增依赖、数据库迁移、权限模式、模型、harness 或声学能力。修复先闭合已复现边界，后续若要把“清理”扩展为跨 renderer 的声学上下文原子重置，需要另行明确宿主/worker 控制合同，不能用本次 UI 失效元数据冒充。
