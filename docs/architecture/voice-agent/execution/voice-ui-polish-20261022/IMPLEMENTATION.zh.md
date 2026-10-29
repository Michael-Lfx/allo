# 语音入口与无效话语交互修复 · 2026-10-22

基线：`feat/voice-next`，`2e7fe85c7cecfd35b3278625f674c1369cbb98dc`。本轮未提交、推送或发布；没有增加依赖、数据库迁移、工具权限或执行器。

## 1. 当前代码行为

| 范围 | 实现与边界 |
|---|---|
| 无效话语 | 空识别、仅唤醒词和模型 ignore 共用 discard 路径，回到 Listening，保持同一 session，无休眠音，无需再次 KWS。使用独立 UtteranceIgnored 事件，保留真实失败分支的 IntentIgnore 语义。180 秒无输入仍结束，显式停止/generation 防线不变。 |
| 上下文 | 清除当前临时原文、回复、理解和预览行，保留此前有效对话。私有 journal 留存 ignore 幂等证据，但不发布公开用户/助手消息；handoff 在取最近有效轮次前排除 ignore 和非法输出。 |
| 正式会话 | ASR 完成和首个有效回复句即可显示，不等待整次生成或播放。用户原文右侧、助手回复左侧；理解折叠，原始决策仅进诊断。临时预览须匹配冻结 Conversation/session。 |
| 去重与滚动 | 宿主 additive projected_turn_ids 按声学 turn 撤下已落盘预览；恢复 binding 时从 journal 重建标记。音量事件不复活旧预览。底部时跟随正文，上翻后保持位置，新增正文只提示“查看新内容”。 |
| 任务控制 | 任务状态、队列、逐项取消、实际工具审批/过期校验、刷新、详情、播报状态与显式重播保留在正式会话；结果正文沿用原 Conversation 时间线。没有扩大权限。 |
| 桌宠 | 形象上方固定 64 logical-pixel 状态区，不改变形象尺寸和底部锚定策略。一个主状态，待授权优先，其余显示数量；标题最多两行、详情在提示中可读。使用主题变量，活动语音保留明确停止按钮。 |
| 显式入口 | 查询真实活动 binding 后打开其 Conversation；否则只唤醒一次、有界等待绑定。连续点击合并，不发送消息/任务，不迁移其他宠物的归属。后台 KWS 不抢焦点。 |
| 独立弹窗 | 删除原生 voice-session 窗口创建/关闭路径、独立路由和闲置 hide API。voice_session_bridge 只保留可信候选上下文报告与快照转发。挂载/订阅不自动开麦。 |

## 2. 实跑验证

| 命令或检查 | 结果 |
|---|---|
| `cargo test -p nomifun-app --test voice_dialogue_integration` | 24 passed；含连续 ignore 无公开消息、保留城市/数字纠正、恢复 binding 投影去重。 |
| `cargo test -p nomifun-app --lib voice` | 81 passed，3 ignored；含真实 runtime 文件读取、排队、停止后任务继续、一次性工具审批。 |
| `cargo test -p voice-companion --features voice-skeleton` | 通过；受控模型/音频/消费 sink 的生产 worker 回归，设备与在线探针保持 ignored。 |
| `cargo test -p voice-companion --features voice-skeleton --test full_voice_flow` | 22 passed，2 ignored；新增 ignore 后不再次唤醒接收下一句，同 session、无休眠音、后续播放完成。 |
| `cargo test -p nomifun-voice-terminal` | 40 passed，1 ignored；含 179999ms/180000ms 超时边界及显式停止反例。 |
| UI 语音/桌宠/adapter | 195 passed；双语、投影身份、点击合并、旧 scope 与重连。 |
| 桌面编译 | `cargo check -p Flowy` 通过，包含新 native bridge。 |
| UI 构建 | `bun run build:ui` 通过；最后一次更新后的重跑结果见下方构建补记。 |
| 定向质量检查 | i18n（11702 keys）、theme、motion、icons、CSS scope、button layout contract、定向 cargo fmt check 与 git diff check 通过。仓库 rustfmt 当前禁用格式重写，不能据此宣称已重新排版全仓。 |
| 全仓前端门禁 | typecheck 与 dead-css 仍失败，输出指向未修改的积分/墙纸/视频画布/Markdown 等模块；本轮 typecheck 无语音/桌宠文件错误。未降低断言或修改基线表。 |

一次受限沙箱尝试被 sqlx canonicalize migrations 的 Windows access denied 阻止；同命令在可读仓库的环境重跑通过，没有修改迁移目录。

## 3. 界面证据

隔离 Vite 页面导入生产桌宠、状态卡片及 VoiceLiveTranscript，使用隔离展示输入，不连接麦克风/用户模型/任务，也没有把测试输入放入生产 Context/hook。

本地证据：`build.noindex/voice-review/ui-polish-20261022/`。

- `pet-themes.png`：中英文、浅深主题、100%/200% CSS 缩放。
- `pet-long-labels.png`：三种场景的长文案均 contentFitsSlot=true、clearOfFigure=true；两行裁切仍在固定状态区内。
- `live-transcript.png`：用户原文、助手正文、折叠理解及生成状态。
- 浏览器滚动反例：上翻后 scrollTop=0，音量事件后仍为 0 且无新内容提示；追加句子仍为 0 并显示提示；点击提示恢复底部。
- `typecheck.log`：全仓类型检查实际失败输出。

本轮 tab 和 5376 Vite server 已关闭；临时页面移到忽略的证据目录。CSS 缩放不等同于 Windows 原生 DPI 验收。

## 4. 边界与 Owner 复测

- 不删除/重写数据库中的历史 ignore JSON；新话语不再生成这些消息。历史条目的精确折叠需要可信来源/回执映射，不能按 JSON 外观隐藏合法代码。
- 流式失败已到达的文字仍由当前 daemon 快照保留原 turn 和 interrupted 状态，不伪装成功、不加入有效 handoff；这不等同于全部分片在宿主重启后的长期恢复。本轮未新增分片持久协议。
- 临时文字采用纯文本预览；正式完成消息沿用 Conversation Markdown。
- 真人麦克风/扬声器、原生点击穿透/拖动/多屏 DPI、安装资源与 macOS/Linux 桌面需分别验收。本轮不运行安装器或自动发布。

Owner 顺序：桌宠点击入口 → 无意义话语 → 不重新唤醒直接问候 → 连续追问 → 提交工具任务并同意/拒绝 → 结果/重播 → 上翻长回复并继续讲话 → 显式停止 → 再唤醒 → 静默超时。核对没有调试弹窗、没有新增 ignore 气泡，停止语音不取消后台任务。

## 5. 最终构建补记

- 最终 UI 源码更新后再次执行 `bun run build:ui`：成功，日志 `ui-build.log`，耗时 1m58s；既有第三方 eval/大 chunk 警告保留。
- `cargo build -p Flowy -p voice-companion --bins`：成功，9m44s；默认开发产物为 `target/debug/Flowy.exe` 与 `target/debug/voice-companion.exe`。daemon fingerprint features 为 `["default"]`，不含 `voice-skeleton`；没有将测试可执行文件替换为生产 daemon。
- 构建前未发现运行中的 Flowy、voice-companion 或 ai-mate 实例。本轮不启动应用、不修改用户运行数据、不生成安装包。开发宿主仍由 Vite 提供 UI，非已安装版本自动更新。
- 最终 UI 回归再次通过：195 passed / 0 failed / 724 assertions。产物哈希收据保存于本地证据目录 `artifacts.json`。
