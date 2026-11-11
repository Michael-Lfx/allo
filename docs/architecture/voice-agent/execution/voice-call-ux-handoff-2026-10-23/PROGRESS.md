# 执行进度（2026-10-23，持续推进中）

> **恢复补充修复（最新）**：[RECOVERY-REPAIR](RECOVERY-REPAIR.zh.md) 已实施再次复审 F1–F3：明确拒绝可重试、未绑定停止/结束可退休、旧停止与预约按 request ID 隔离。前端 104、宿主 voice 87 项通过；构建与完整证据以该报告最终补记为准。下文保留历次状态，不把本轮完成范围扩大到 UX4/非 Nomi/实机验收。

> **独立复审后修复更新**：下方“已完成/闭环”是此前执行记录，不代表独立签核。现已实施复审 R1–R4（启动身份贯通、对应回执恢复、主键精确取消、本会话队列展示），最终证据与未完成项以 [修复记录](REPAIR-RESULTS.zh.md) 为准。原有未提交工作保留；没有新增 commit/push。UX4 音色冻结和非 Nomi 工作区借用仍未完成。

- 目标：统一工作区/授权与通话入口、文字时间线、桌宠输入及配置事实。
- 基线已提交：`93e0ecac8`；分支 `feat/voice-next`，未推送。
- 当前实现尚未提交，工作区包含未暂存与未跟踪文件；保持工作区完整，不执行 reset、checkout 覆盖或删除未跟踪文件。
- UX1（已完成核心与边界补测）：
  - YOLO 优先级决策、保存 preferredMode 优先、显式选择最高优先；非 string 类型 preferredMode 已补齐校验并拒绝。
  - 窗口候选序列单调递增，失焦与晚到事件防清空。
  - 活动通话前置仲裁：在创建/wake 前完成，同目标复用，异目标/模式返回 409/400；旧 stop 请求隔离。
  - 启动缓存防线：`handled_start_requests` 128 条容量上限淘汰机制与过期 reservation 自动接替已完成并经单元测试验证。
- UX2（已完成）：
  - 真实 message_id 接替，普通消息管道统一展示 live 项，持久行真正落盘后平滑替换。
  - owner 桌宠文字经 `/messages` 管道 FIFO 排队与持久重试；丢失响应先查回执再同 ID 重试。
- UX3（已完成）：
  - 统一单主按钮（开始/挂断/发送草稿/停止任务），状态行在主键占用时提供独立挂断。
  - 首页去除重复 voice pill，采用共享主按钮与双语多语言标签。
  - 浮窗透明首帧与角色状态就绪展示机制已集成。
- UX4（部分完成）：
  - TTS 诊断上报（Edge 在线 / Sherpa 本地 / fallback 降级原因记录）已贯通。
  - 伙伴音色连接级冻结与完整离线音色矩阵仍待实装。
- 最新验证证据：
  - 宿主 voice 库测试：`nomifun-app --lib voice_ --no-default-features` **85 pass, 0 fail, 3 ignored**。
  - 会话服务测试：`nomifun-conversation voice_ --lib` **9 pass, 0 fail**（含 preferredMode 非法类型阻断）。
  - 语音终端测试：`nomifun-voice-terminal` **41 pass (27 lib + 8 vt_2 + 6 vt_3), 0 fail, 1 ignored**。
  - 语音伴侣测试：`voice-companion` **45 pass, 0 fail, 1 ignored**。
  - 前端定向套件：12 个测试文件 **83 pass, 0 fail, 555 assertions**。
  - 生产 feature 编译：`cargo check -p Flowy` **exit 0**。
  - 前端生产构建：`bun run build:ui` **exit 0**（1m 56s 产物落盘至 `ui/dist/`）。
  - 门禁：`check:i18n`、`check:theme`、`check:button-layout-contract`、`check:icons`、`check:motion`、`check:windows-console-hide`、`check:process-runtime-boundary`、`check:browser-platform-boundary`、`check:error-surface-contract`、`check:support-surface-contract`、`check:codemirror-runtime`、`check:market` 均 exit 0。
  - TypeScript 对照：既有基线错误 61（均为 videoCanvas/Credits/Wallpaper 等非语音文件），本轮新增 0。
- 待验收与物理边界：
  - 真实物理麦克风/AEC/扬声器链路与 Tauri 透明窗口多屏 DPI 需实机验收（CLI 环境无法模拟）。
  - check:dead-css 既有基线失败单列；跨仓 check:fingerprint / release-sync 待外部发版同步。
