你是执行者。以本任务书及仓库规则为任务合同；未决事项写入下述 BLOCKED.md，不猜测授权，继续不受影响部分。恢复会话先读 PROGRESS.md，每完成一项立即更新。
这活是让语音入口、项目任务和桌宠呈现使用同一真实绑定，避免任务误投、消息闪失及虚假配置。
取舍：正确目标和数据安全 > 既有契约 > 完整交互 > 动画。以下“不许”为约束，“建议”为可替换手段，替换须记录理由。

## 我替领导拍的板
已确认：新语音会话默认 YOLO；当次显式选择、保存偏好优先；已有有效模式不覆盖。项目语音创建/复用项目专属 Conversation，不把普通聊天原地转换。
工作默认（建议，未单独确认）：未通话宠物打字保留私聊；通话中仅 owner 宠物的补充进入绑定语音 FIFO，草稿不串。音色先支持已有 Edge/Sherpa，其他 provider 明示实际降级并记录缺口，不新做 SDK。猜错的代价是需调整这两处交互，不能以此扩大权限或迁移历史。

## 界限
仓库 D:/workSpace/git_clone_test/allo；沿当前分支。先确认前执行 agent 已停止同文件写入，冲突时暂停共享文件，先做独立核验。不新开并行 agent，不切分支、reset、清理他人改动，不提交/推送/发布。
方案 docs/architecture/voice-agent/execution/voice-call-entry-pet-presence-plan-2026-10-23.zh.md：先读 §9，再读 §0、§8。旧固定电话键的 D2/CP-06/U2 已由 §8.4/§9 取代；其他业务验收保留。
白名单：apps/desktop/src 的窗口及 voice_session_bridge 接缝；apps/voice-companion/src 的 TTS 装配及事件；crates/backend/nomifun-app 的 voice*、nomifun-api-types 的语音 DTO、nomifun-conversation 的语音创建/消息投影、nomifun-voice-terminal 的已有 TTS/播放契约；上述定向测试；ui/src/common/adapter/voiceCompanion*、平台桥，以及 renderer 的 companion/voiceSession/settings/guid/conversation 页面、chat 组件、AgentModeSelector、语音 hooks/services、项目导航上报接缝、草稿 store、双语词典及生成类型；本方案与状态文档。
仅改完成目标必需部分。禁止改 agent harness、数据库迁移、依赖/锁文件、auth/security、CI/门禁脚本、发布及音频算法。越界必要项记 BLOCKED.md；用户已有授权的 YOLO 默认调整属于本任务，不另起权限流程。
不得删除/skip 测试、放宽断言、mock 被测业务或用假运行态过关。仅可修正方案明确替代的旧视觉/投影断言，须登记旧意图及新覆盖。

## 现状与任务 0
基线 2026-10-23：HEAD e9b125536，加大量未提交工作；这不是干净分支，也不是所有功能已通过。
我实跑过：从 ui/ 执行
bun test ./src/renderer/pages/conversation/platforms/nomi/nomiSendBoxDispatch.test.ts ./src/renderer/pages/voiceSession/liveVoiceTurn.test.ts
结果 9 pass、0 fail、39 expect。沙箱父目录权限曾造成 EPERM/EBADFD，精确提升权限后通过。它不证明原生窗口/真实 runtime 已验收。
先记录当前 HEAD、dirty 文件和接管差异，核对命令、依赖及 Cargo 实际桌面 feature 图并复跑以上基线。数量或行为变化先解释，不复位仓库。检查项目脚本再选择真实存在的类型/i18n/theme、Rust、构建和聚合命令，禁止编造命令或重复声称历史通过。
在同目录 voice-call-ux-handoff-2026-10-23/ 创建 PROGRESS.md、BLOCKED.md，前者先写不超过十行目标/顺序/最大风险，后者无阻断也写“无”。

## 任务
1. 先贯通目标和授权：首页选 P、P 普通会话、P 空项目页、桌宠、KWS 使用同一可信宿主解析；无明确项目才默认，失焦不清选择，明确选默认才清。无效目录拒绝，非 Nomi 只借可信项目作用域，不强转模型/预设。复用会话保留配置，新建模式保存/显示/runtime/任务冻结一致。
先查稳定 request 回执及活动通话，再做创建/wake；P 活动时请求 Q 必须零创建/发送/wake。响应丢失先核对原身份，取消/迟到 ready/旧 stop 不污染新连接。复用既有启动修复，不新建调度器。
2. 接通通话文字和时间线：owner 宠物纯文字与正式页文字走同一 FIFO；独立聊天草稿不自动迁移，失败重试保留原目标/幂等身份。按 message_id/turn_id 合并临时与持久项，正式消息真正显示后再移除占位。ignore 不污染历史或挂断。附件/Skills 保留草稿并说明能力边界，不绕过队列。
3. 收敛交互配置：单主键顺序为运行任务→停止任务；无运行任务有草稿→发送；空草稿未通话→开始；空草稿通话中→红色挂断。主键为发送/停止时，状态行仍可挂断。停止仅处理绑定 command，取消中等真实终态；其他队列项保留并明确展示。终态回时间线、诊断进更多菜单、审批始终可达。
删除误导 UI 前盘点 /target 消费者；公共接口/存储兼容未定不得盲删。宠物隐藏创建，真实角色/位置/尺寸/首帧就绪再显示，失败有恢复入口。双语、双主题、窄屏、DPI 和减少动态都覆盖。
4. 说明并接通实际 TTS 来源：复用已有音色字段与支持能力，连接级冻结；不支持 provider 明示 actual/fallback。中文英文分别验证，实际播完才更新状态；失败保留文字、旧回执不复活播报。现有音频修复不倒退。

每项执行“最小失败复现→完整纵向实现→生产入口集成→定向验证→更新证据”，自动门禁通过后继续下一项，不等逐轮人工批准。
运行方案 AUTH、ROUTE/CHAT/UI/CFG、CLOSE-01–04 及未替代 CP 用例。实际延迟消息列表、丢失 wake 回执、提交无效项目，先留正确变红输出再修复变绿；测试仅在临时库/目录中注入故障。随后跑受影响 Rust 测试/编译、前端行为与类型/i18n/theme、必要桌面 feature 编译/UI 构建及仓库聚合门禁。已有失败与新增失败分列，不降低验收。

## 规矩
同一验收连败三次先诊断并记阻断，换独立项，禁止盲重试；需回退只撤销自己新增改动。不能让浏览器截图代替原生透明窗口、焦点、DPI 或系统播放证据；尽量用代码和宿主自动化闭环，做不到如实标 pending，不要求用户先口述。日志不得输出密钥/完整私聊。
不自动发布或下载新模型，不引入 Zipformer、云 Realtime、声纹、截图权限或新 AEC。所有未实现项更新当前状态文档，不能用“已闭环”盖过缺口。

## 完成条件
1. AUTH/ROUTE/CHAT/UI/CFG/CLOSE 及有效 CP 逐项有生产路径证据；无错投、重复执行、消息空白、草稿串线和权限显示不一致。上述两个基线文件有效测试不少于九项；新增断言覆盖反向场景，不能只绿结构测试。
2. 无越界修改、他人工作丢失、新依赖/迁移或自动 Git 交付。交付 diff、实际命令/退出码/数量、红→绿证据、基线失败清单、PROGRESS.md 与 BLOCKED.md。原生证据或 provider 接通不足时仅标部分完成，写明剩余项，不签核“全部完成”。
