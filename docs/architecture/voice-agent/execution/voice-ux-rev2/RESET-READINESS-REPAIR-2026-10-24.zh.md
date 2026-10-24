# 清理前语音归属准备检查 · 修复记录

> 2026-10-24；`feat/voice-next`，HEAD `7de276e61e93836365c5a1c7e006c557ce3e3c6d`。基于既有未提交改动修复 [追加复审](FOLLOWUP-REVIEW-2026-10-24.zh.md) 的 Required 项，未提交、未推送、未切分支。历史复审与测试日志保留原文。

## 行为与决策

清理和重置现在共用 `resetVoiceHistory`：先确认语音归属、冻结当前轮次身份，再执行原 Conversation API；只有 API 成功才提交 epoch 和失效屏障。

| 输入状态 | 当前行为 |
| --- | --- |
| 快照为空 | 暂不调用清理 API，不暂停队列、不重置本地执行状态；提示状态同步后重试 |
| session 已存在，但 binding 缺失或 session 不匹配 | 同上，等待可确认的归属 |
| 当前 Conversation 绑定有效 | 冻结三轮身份，调用原 API，成功后使旧 live 失效 |
| 明确没有 session，或有效通话绑定在其他 Conversation | 允许清理目标 Conversation，不使其他通话失效 |
| API 失败 | 不提交 epoch/语音失效；已有失败处理保留原文与恢复入口 |
| 原组件卸载后 API 才成功 | 屏障仍提交；页面 toast/本地状态继续由既有生命周期保护 |

中文与英文提示均说明“尚未清理，请稍后重试”。不自动重放清理意图，避免用户后来开始新一句时被旧操作误清。归属未确认期间清理暂不可执行，是显式准备条件；没有靠隐藏成功后收到的全部文字来掩盖问题。

普通会话仍调用原 API；未增加存储、依赖、配置、权限模式、后台任务或宿主/worker 重置协议。原模型上下文与任务生命周期仍由原服务管理。

## 实施入口

- `ui/src/renderer/pages/voiceSession/liveVoiceTurn.ts`：准备条件及共享的“冻结 → API → 成功提交”顺序。
- `ui/src/renderer/pages/conversation/platforms/nomi/NomiSendBox.tsx`：clearContext、required reset 两条路径均接入；准备失败在已有 catch/onError 内处理，不会变成未处理的拒绝。
- `ui/src/renderer/pages/voiceSession/voiceHistoryReset.test.ts`：8 项正式回归，包含原先遗漏的空快照/缺 binding/错 session、失败、卸载、新输入、明确空闲/外会话对照，以及两入口接线与双语提示。
- `ui/src/renderer/services/i18n/locales/{zh-CN,en-US}/voice.json`、`i18n-keys.d.ts`：新增一条可重试提示。

## 本轮已完成的验证

日志目录：`build.noindex/voice-workspace-display-review/`。

| 检查 | 结果 | 日志 |
| --- | --- | --- |
| 正式反例先运行 | 3 pass / 4 fail；未同步时错误调用 API、required reset 错误成功均被捕获 | `repair-reset-readiness-red.txt` |
| 定向组 | 27 pass / 0 fail / 93 assertions | `repair-reset-readiness-targeted.txt` |
| 完整语音前端 + 共享重置 20 文件 | 157 pass / 0 fail / 1213 assertions，4.76s | `repair-reset-readiness-formal-final.txt` |
| 新测试完整类型 fixture 后复跑 | 8 pass / 0 fail / 44 assertions | `repair-reset-readiness-fixture-final.txt` |
| 共享历史/清理/编辑重发 10 文件 | 100 pass / 0 fail / 369 assertions | `repair-reset-readiness-history.txt` |
| 双语/主题/动效/图标/静态按钮布局/CSS scope/错误面/支持面/进程边界/浏览器边界 | 10 项 exit 0 | `repair-reset-readiness-gates.json` |
| 完整 TypeScript 对照 | exit 2；61 项既有诊断，逐项对照新增 0 | `repair-reset-readiness-typecheck-final.txt`、`repair-reset-readiness-typecheck-compare.json` |
| 生产 UI 构建 | exit 0，2m 4s；原有大 chunk 警告保留 | `repair-reset-readiness-build.txt` |
| 差异检查 | exit 0；既有 CRLF 提示保留 | `git diff --check` |

反例先接入与生产调用方相同的共享函数，再运行确认 RED，随后加入准备检查。修复没有放宽“不显示旧轮次”断言；改变的是无归属时根本不允许后端清理成功，并以调用数 0、epoch 不变、原文保留证明。状态恢复后重试成功、单飞合并重复重置、下一句仍显示均有对照。原忽略目录的审查探针保留，不能继续将其“空快照下已成功清理”的前提当作现行为。

新测试最初使用了仓库现有 Bun 类型声明尚未声明的 `rejects` 写法；已改用项目已有的 Promise 错误结果断言，并补齐 fixture 的必填快照字段，运行检查与类型检查分开记录。

本轮没有重新运行 Rust 或物理设备验收；原生桌宠透明/DPI、麦克风/扬声器及生产 AEC/打断能力不因本轮逻辑验证而签核。完整 renderer 重建和多 renderer 仍遵循既有合同，本次准备检查没有扩展成新的持久失效协议。
