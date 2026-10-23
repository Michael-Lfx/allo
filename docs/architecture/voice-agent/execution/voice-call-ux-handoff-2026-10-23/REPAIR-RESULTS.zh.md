# 通话交互独立复审问题修复（2026-10-23）

> **恢复补充修复**：本报告之后复审发现的 F1–F3 及后续修复见 [RECOVERY-REPAIR](RECOVERY-REPAIR.zh.md)。下方测试数量与 R1–R4 为此前证据；最新恢复合同和验证以补充记录为准。

本轮按用户“那你来执行修复”授权，修复 [独立复审](INDEPENDENT-REVIEW.zh.md) 的 R1–R4。基线仍为 `93e0ecac8`、分支 `feat/voice-next`；保留原有未提交工作，没有提交、推送、切分支或修改依赖/数据库迁移。

## 1. 修复内容与身份契约

| 发现 | 实际修复 | 回归证据 |
| --- | --- | --- |
| R1 迟到唤醒错绑工作区 | 宿主启动 request_id 经 daemon WakeHit、快照 call_request_id、对话请求和绑定日志贯通。超过 10 秒也只兑现同 request 的冻结目标；不同 request、已被替换的预约及无身份 KWS 均不能消费当前预约。保存绑定与消费预约在同一身份锁下完成 | 真实 Conversation 集成：原 P 预约老化 11 秒、界面选择 Q，仍绑定 P；旧 ID/无 ID 不能消费新预约 |
| R2 启动响应丢失阻挡重开 | adapter 按 call_request_id 与真实 session/binding 核对 pending，订阅快照和成功停止均可解除。网络失败保留原 ID，不因超时换 ID 重发。宿主仅在 daemon 确认停止后退休回执；自然结束也退休对应已绑定请求，先过滤旧序列 | 正式 adapter 测试：响应丢失→旧停止不能解除→对应停止→正式页新请求成功；宿主自然结束/旧结束隔离测试 |
| R3 主停止键缺少 command 身份 | 语音页冻结点击时 task，通过既有 voice cancel API 发送 instance/command/conversation/message；不走普通 Conversation stop。不确定/交付中/取消中只查原任务；取消等待真实终态。普通聊天保留原停止路径，其他 FIFO 项保留；导航仅释放本页等待 | HTTP 边界测试 A→B 交错时只取消 A，B 队列不变；unknown 等只 GET，错会话不发请求；主按钮接线与导航隔离断言 |
| R4 外会话运行隐藏本页队列 | 状态条隐藏条件同时检查本 Conversation 的有效排队项；Q 执行时 P 的排队、位置和取消入口保留，仍不展示 Q 的内容 | 实际状态组件 SSR：中英文、明暗主题，Q active/P queued 可见；空队列对照隐藏 |

`call_request_id` 是可选、serde 默认的新增传输字段；声学 session/turn、命令 command、runtime turn 的身份仍各自独立。KWS 发起的会话不继承上次 UI request。宿主与 daemon 应联合构建更新；不将混用旧 daemon 视为已验证的恢复路径。

本轮没有将任意超时当作失败或已停止，没有回退到当前选中的其他工作区，也没有以普通 conversation.stop 兜底语音取消。

## 2. 测试入口补全

此前 `nomifun-app --lib` 和桌面 check 没有编译 `tests/voice_dialogue_integration.rs`。本轮完整运行该 target，先发现旧签名调用与三处旧会话预期，再按当前合同更新：普通聊天不原地转换；创建/复用工作区专用语音 Conversation；忙碌追加使用真实语音目标 fixture。保留原有隔离、忙碌追加、真实持久化和无重复调用断言，没有删除或忽略失败用例。

新增反向用例进入正式源码测试，忽略目录探针不充当最终交付。状态条提取接收快照的实际展示组件，仅作为可验证观察边界，生产仍使用同一订阅和组件，不添加生产替身。

## 3. 本轮验证

| 命令/检查 | 最终结果 |
| --- | --- |
| `cargo test -p nomifun-app --no-default-features --test voice_dialogue_integration` | 26 passed / 0 failed |
| `cargo test -p nomifun-app --lib voice_ --no-default-features` | 86 passed / 0 failed / 3 ignored；69.45 秒 |
| `cargo test -p voice-companion --features voice-skeleton` | 各 target 合计 101 passed / 0 failed / 11 ignored |
| 前端定向 16 文件组 | 101 passed / 0 failed / 702 assertions |
| `bun run build:ui` | exit 0；2 分 12 秒，保留既有 eval/chunk warning |
| i18n/theme/motion/button-layout-contract/icons/css-scope | 各 exit 0 |

后续终端/会话测试、桌面 feature 编译、TypeScript/聚合门禁与差异检查汇总附于文末。本地原始日志在忽略目录 `build.noindex/voice-call-ux-repair/`，不随 Git 交付；接手应重跑相同正式命令。

前端回归从 `ui/` 执行：

```powershell
bun test ./src/renderer/pages/voiceSession ./src/common/adapter/voiceCompanion.test.ts ./src/renderer/pages/companion/voicePetDelivery.test.ts ./src/renderer/pages/companion/companionTurnDelivery.test.ts ./src/renderer/components/chat/ComposerSubmitCluster.structure.test.ts ./src/renderer/components/chat/ComposerSubmitCluster.behavior.test.tsx ./src/renderer/components/chat/SendBox/stopInteraction.structure.test.ts ./src/renderer/pages/conversation/platforms/nomi/nomiSendBoxDispatch.test.ts ./src/renderer/pages/guid/components/ComposerEntryStrip.test.ts ./src/renderer/pages/guid/GuidDraftSkills.test.ts
```

## 4. 保留边界

R1–R4 的代码修复与自动回归不等于整个语音方案全部验收。非 Nomi 来源的可信工作区借用、伙伴音色连接级冻结/完整离线音色矩阵仍未实现；此前对移除 Nomi 来源过滤的自动审批拒绝仍有效，本轮未改该过滤。物理麦克风/AEC/扬声器、原生透明首帧/DPI/焦点没有新增实机证据。全仓已有 TypeScript/dead-css 问题与跨仓发版检查单列，不放宽门禁。

## 5. 收口验证汇总

| 检查 | 结果与边界 |
| --- | --- |
| `cargo test -p nomifun-voice-terminal` | 27 lib + 8 vt_2 + 6 vt_3 = 41 passed / 0 failed / 1 ignored |
| `cargo test -p nomifun-conversation voice_ --lib` | 9 passed / 0 failed |
| `cargo check -p Flowy` | exit 0，1 分 20 秒；既有 unused 等 warning 保留 |
| `bun run check` | exit 2，typecheck 61 条诊断阻断，后续串联门禁不算通过 |
| TypeScript compiler 同配置对照 | 基线 62 → 当前 61；按 file/code/message 比较新增 0，不切分支、不覆盖源码。对照脚本须有与正式 Bun 检查一致的依赖读取权限；受限入口产生的 940 条缺失类型等噪声不作为代码结论 |
| `bun run check:dead-css` | exit 1；WallpaperSection、video/eval、codeBlock 等所报文件均不在本轮语音差异内；未修改基线/规则 |
| `git diff --check` | exit 0 |

未跑全仓 Cargo check、跨仓 fingerprint/release-sync 或安装包/原生设备签核；桌面完整 feature 编译与 UI 生产构建不替代这些检查。此次未改变声学算法、半双工默认或工具授权策略。
