# 语音交互优化实施交接（2026-10-23）

> **最新恢复修复**：[RECOVERY-REPAIR](RECOVERY-REPAIR.zh.md) 已实现再次复审 F1–F3 并记录正式验证；下文与此前修复报告均保留为历史阶段。当前待办仍包括 UX4/非 Nomi/原生验收。

> **后续实施入口**：独立复审发现并修复的 R1–R4、当前测试与剩余边界见 [REPAIR-RESULTS](REPAIR-RESULTS.zh.md)。下文保留原交接全文；其中历史待复跑/未构建描述不作为当前结果，不能据此重复判定或将 UX4 等未实现项视为完成。

**部分实现，尚未最终验收；按用户要求先汇报并留下交接。** 历史测试数量不能解释为最终 dirty 工作树全部通过。

## 1. Git 与执行合同

- 根目录：`D:/workSpace/git_clone_test/allo`；分支 `feat/voice-next`。
- 用户要求先提交已有工作，已完成 `93e0ecac8ca817c47b9cf37ad354456977f90c65`：`feat(voice): 固化语音生命周期修复与通话入口桌宠联动基线`，53 文件、+4149/-161。
- author/committer：`hoye <hoyework@qq.com>`，仓库 hooks 已启用并通过；只本地提交，未推送。
- 此后的实现和本文档均未提交。不要 reset、checkout 覆盖或清理未跟踪文件，尤其不能遗漏新桌宠交付及按钮行为测试。
- 合同：[主方案 §9、§0、§8](../voice-call-entry-pet-presence-plan-2026-10-23.zh.md)、[任务书](../voice-call-ux-agent-brief-2026-10-23.zh.md)。用户授权自己实施，并单独授权基线提交；没有自动推送/发布授权。
- 不使用并行 Agent，不新增依赖、迁移、agent harness 或音频算法。

## 2. 已实现的生产路径

| 片段 | 代码事实 | 尚未覆盖 |
| --- | --- | --- |
| UX1 授权 | Conversation `resolve_voice_session_mode`：显式选择→保存 preferredMode→YOLO；非法模式拒绝；已有模式保留；runtime 冻结/发送比较统一 | 最终 AUTH 表未逐项签核 |
| UX1 工作区 | native bridge 校验主窗口/路由/焦点，传 workspace 与 sequence；失焦不清，旧上报不能覆盖新值；首页上报选中目录 | 非 Nomi 来源仍拒绝 |
| UX1 启动 | 活动 P/Q 仲裁在创建/wake 前；current_selection 进入宿主解析；同 ID 改 target/mode 冲突；未知超过 10 秒不重唤醒 | pending 取消、缓存/过期 reservation 边界需复核 |
| UX1 重试 | `startVoiceCallIntent` 在首页、正式页和桌宠入口共享稳定身份/in-flight Promise；未知保留；绑定快照/成功停止解除对应 pending | 跨页面→未知→绑定→停止→新通话仍需完整回归 |
| UX2 时间线 | 宿主返回真实 user/assistant message_id；NomiChat/MessageList 普通消息管道显示 live 项；持久行真正进入列表才逐项接替 | 实际组件分页/排序/操作未集成验证 |
| UX2 桌宠 | owner 通话使用 VoicePetInput，经 `/messages` 持久 FIFO；私聊草稿独立；丢失响应先查回执再同 ID 重试 | 原生布局和 owner 切换需核验 |
| UX3 操作 | 运行→停止任务；有草稿→发送；空草稿→开始/危险色挂断；主键被发送/停止占用时状态行仍可挂断 | 中英文 SSR 通过，原生 CSS/窄屏未验收 |
| UX3 精简 | 首页去重复 voice pill，改共享主键和双语提示；去重复开始/终态大面板/常驻停止说明，诊断折叠、审批可达 | CLOSE-04 最终取消/排队联动未签核 |
| UX3 首帧 | boot 样式保持透明/root 隐藏；等待真实 profile、位置、尺寸、首帧和资源再展示；不显示 AppLoader；失败重试并提示主窗 | 未验证真实透明窗口/DPI/首帧；保留原生 show 时序避免 macOS 定位倒退 |
| 配置 | 删除独立目标选择 UI；保留 target 公共 API/存储，如实返回 workspace_routing_active；伙伴 TTS 标明作用范围 | 没有盲删公共 DTO/用户配置 |
| UX4 来源 | TtsProvider diagnostic、Fallback 选择记录、作用域 TtsSource 事件及设置只读来源；Edge 在线、Sherpa 本地 | 全路径/旧 scope 测试、伙伴音色连接级冻结未完成 |

项目模型/预设只在创建时复制到语音 Conversation，复用从该语音 Conversation 自身冻结；无工作区普通会话不能原地转换。审批测试现在显式保存 default，继续验证真实副作用先挂起，不能依赖旧缺省值。

## 3. 已实跑证据

| 检查 | 结果与限制 |
| --- | --- |
| `cargo test -p nomifun-conversation voice_ --lib` | 9 pass，覆盖模式优先级及语音创建 |
| `cargo test -p nomifun-app --lib voice_ --no-default-features` | 最近一轮 84 pass、0 fail、3 ignored，测试 67.76 秒；覆盖真实 runtime 读随机文件、审批、队列和 worker。**之后追加过代码/生命周期断言，仍需最终复跑** |
| 前端定向 11 文件组 | 81 pass、0 fail、500 assertions |
| 主按钮 SSR + adapter + voicePetDelivery | 22 pass、0 fail、124 assertions；与上组有重叠，不能相加 |
| gen:i18n | 11729 keys / 55 modules |
| i18n/theme/motion/button-layout-contract/icons/css-scope | 各 exit 0 |
| git diff --check | exit 0 |
| TS 只读覆盖层对照 | 已提交基线 62 → 工作树 61，新增诊断 0；比较 file/code/message，不切分支或覆盖源文件。最终修改后仍需复跑 |
| check:dead-css | exit 1；报告 Wallpaper、video/eval、codeBlock 等本轮未改文件。未签通过、未修改门禁 |

从 `ui/` 跑 81 项的命令：

```powershell
bun test ./src/renderer/pages/voiceSession ./src/common/adapter/voiceCompanion.test.ts ./src/renderer/pages/companion/voicePetDelivery.test.ts ./src/renderer/pages/companion/companionTurnDelivery.test.ts ./src/renderer/components/chat/ComposerSubmitCluster.structure.test.ts ./src/renderer/pages/guid/components/ComposerEntryStrip.test.ts ./src/renderer/pages/guid/GuidDraftSkills.test.ts
```

追加 22 项：

```powershell
bun test ./src/renderer/components/chat/ComposerSubmitCluster.behavior.test.tsx ./src/common/adapter/voiceCompanion.test.ts ./src/renderer/pages/companion/voicePetDelivery.test.ts
```

过程红→绿：测试注入改用现有公开接口；旧设置页 import 失败改为验证该入口已移除；旧视觉断言按方案 §9 明确替代；审批测试受 YOLO 缺省影响，改为显式 default 后真实挂起/拒绝用例通过，未放宽副作用断言。GuidDraftSkills 用现有 `test as it` 修复类型，不改测试意图。

本地日志位于忽略目录 `build.noindex/voice-call-ux/`：rust-voice.txt、check-*.txt、typecheck.txt、ts-baseline.json、ts-current.json、compare-types.cjs。**日志不随 Git 交付**，接手需重建。早期 typecheck.txt 的 63 项是中途结果，最后对照以 62/61 JSON 为准。

## 4. 接手后顺序

1. `git status --short --branch`、`git diff --check`，先看全部 dirty 和新文件。已知测试会话已结束，不要启动 dev 热重载争用 Cargo。
2. 复跑最新 `test_session_start_call_lifecycle_and_conflict`，再跑宿主 voice 组。核验 stable intent 未知→绑定→停止→新通话、改变目标/模式、旧 stop；内存缓存驱逐、过期 reservation 与新请求、pending 取消需补证，不得宣称 CP-05 完成。
3. 核验创建复制模型/预设、复用保留自身配置、键盘/语音一致、队列冻结；补非法 preferredMode 字段类型。非 Nomi 路径见 BLOCKED，不得直接删除过滤。
4. 验证真实 durable message_id→UI history 的映射、延迟列表/分页/排序、busy assistant projection、旧日志恢复及重复文本不同 turn。临时消息不能进持久缓存或暴露不可用的编辑/重试动作；检查 MessageList 自动滚动。
5. 桌宠私聊草稿→通话→挂断后保留；owner 切换隔离，IME Enter；两行输入不挤压头像/状态区。boot 资源超时要可恢复，位置/尺寸失败不得移除 boot 样式。原生透明/双主题/DPI 留真实证据。
6. TTS 目前常规播报/流式首音报告来源，检查工具确认/任务回执等其他直接播放路径；补 fallback、过期 scope、半句失败及中文英文验证。伙伴音色冻结尚未实现；不支持 provider 不可声称生效，不加 SDK/模型，不把来源当播放完成回执。
7. 最终运行前端组加 SSR、`cargo test -p voice-companion`、`cargo test -p nomifun-voice-terminal`、`cargo check -p Flowy`（桌面真实 feature 图）、`bun run typecheck`、`bun run check`、独立 UI gates、`bun run build:ui`，并按合同补 workspace 检查。已有失败单列，不能修门禁绕过。格式化避免巨型 service 文件整体无关重排。
8. 补 AUTH-01–06、ROUTE/CHAT/UI/CFG、CLOSE-01–04 和未替代 CP 逐项证据；没有原生证据就 pending。同步文档，后续提交/推送依用户授权，不自动发布。

## 5. 审批记录与未完成声明

自动审批拒绝了“移除 Nomi 会话类型过滤”的补丁，理由是扩大 ACP 等执行范围、可能导致配置错配。已保留过滤并对不支持来源明确拒绝；方案的非 Nomi 工作区借用仍有缺口，不通过别的工具绕过该拒绝。

尚未运行最终桌面构建、UI 生产构建、全仓聚合门禁和原生系统验收。未下载模型、修改依赖/迁移/发布配置。此交接不是全部完成声明。
