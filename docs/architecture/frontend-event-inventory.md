# 前端同窗口自定义事件通信清单

## 当前迁移状态（2026-11-12）

下方 §1–§7 保留原始盘点和迁移建议作为历史基线；其中 31 个契约、34 个发送位置和
35 个监听位置是盘点时的数据，不代表当前剩余数量。已核实的迁移如下：

| 原事件 | 当前状态与入口 | 发送、监听与清理 |
| --- | --- | --- |
| `nomifun-open-update-modal` | 已迁移：`app.update.open` | Titlebar 和 Layout 发送，UpdateModal 订阅并在 effect 清理时取消订阅。 |
| `nomifun-workspace-toggle` | 已迁移：`workspace.toggle` | `workspaceEvents.ts` 封装发送和订阅；订阅者负责清理。 |
| `custom-css-updated`、`nomifun:css-themes-changed` | 已迁移：`themeEvents.ts` | 同窗口发送和订阅；各订阅者清理。Tauri 跨窗口主题同步仍独立保留，详见 §4.3。 |
| `flowy:sider-history-search-select` | 已迁移：`sider.history-search.select` | Titlebar 选择会话时经 `historySearchSelect.ts` 同步发送；Sider 订阅并在 effect 清理时取消订阅。监听器异常会报告且不阻断其他监听器。 |
| `nomifun:tts-config-changed` | 已迁移：`speech.tts-config.changed` | `saveTextToSpeechConfig` 保存或恢复后在 `finally` 同步通知；`TextToSpeechContent` 订阅并在 effect 清理时取消订阅。监听器异常会报告且不影响保存结果或其他监听器。 |
| `nomifun:speech-to-text-config-changed` | 已迁移：`speech.stt-config.changed` | `saveSpeechToTextConfig` 保存或恢复后在 `finally` 同步通知；`SpeechToTextContent` 订阅并在 effect 清理时取消订阅。监听器异常会报告且不影响保存结果或其他监听器。 |

`skill-catalog-changed`、`nomifun-update-available` 等事件仍保留在
原通道。对其他清单条目，迁移前应重新核对当前发送方、监听方和清理方式，历史行号
不能视为实时位置。元素级画布事件、跨层和跨窗口事件继续按各自契约处理。

侧栏搜索事件验证：`bun test ui/src/renderer/components/layout/Sider/historySearchSelect.test.ts`、
`bun run typecheck`；该事件在仓内没有发现旧名称的其他消费者，外部脚本若监听旧 DOM
事件名则需要另行更新。

TTS 配置事件验证：`bun test ui/src/renderer/services/textToSpeechConfig.test.ts`、
`bun run typecheck`；仓内未发现旧事件名的其他消费者，外部脚本若监听旧 DOM 事件名
则需要另行更新。

STT 配置事件验证：`bun test ui/src/renderer/services/speechToTextConfig.test.ts`、
`bun run typecheck`；仓内未发现旧事件名的其他消费者，外部脚本若监听旧 DOM 事件名
则需要另行更新。

## 历史盘点基线与原始记录

> 状态：盘点基线（Batch 3 · 第三步）
> 范围：`ui/src/renderer/**` 与 `ui/src/common/**` 生产代码中的同窗口自定义业务事件
> 性质：只读盘点。本文不修改事件运行逻辑，不删除发送方，也不新增门禁。

## 1. 基线信息

| 项 | 值 |
| --- | --- |
| 扫描日期 | 2026-11-12（东八区） |
| Git commit | `97b76d99e10cf4391d157ff91a31b7a1b424f7b8` |
| 提交说明 | `fix(ui): prebundle home ambient shaders before first open` |
| 工作区状态 | 干净（执行前 `git status --short` 无输出） |
| 相关前置提交 | 更新弹窗迁移 `e3937b04f`；工作区标题栏迁移 `5ee84a366` |
| 迁移目标通道 | `ui/src/renderer/utils/emitter.ts`（`eventemitter3` 类型化总线） |

> 本次环境未安装 `rg`（PowerShell 报 `CommandNotFoundException`），因此使用等价的
> `Select-String` 与编辑器 grep 执行了任务中给出的同类模式，并对多行写法、常量、
> 变量、包装函数逐处人工核对。下列统计均按代码人工复核，不以命令命中行数作为事件数。

## 2. 统计口径与排除依据

### 2.1 计数单位

以「**事件名称或可解析契约 + 发送目标／作用域**」为一条记录：

| 指标 | 数值 | 说明 |
| --- | --- | --- |
| 事件契约 | **31** | 去重后的自定义事件名称（含 `CustomEvent` 与 `Event`） |
| 发送位置 | **34** | `dispatchEvent` 实际调用点；辅助函数被多次调用时只按 `dispatchEvent` 点计 |
| 监听位置 | **35** | `addEventListener` 注册点（按文件／辅助函数计；同一容器的多个订阅者不重复展开） |

补充说明：

- `custom-css-updated` 有 2 个直接 `dispatchEvent` 点（`useCssTheme` 辅助函数、`Layout`），
  但 `useCssTheme` 内部有 4 个调用辅助函数的位置，均汇总到同一发送点。
- `nomifun:auth-expired` 有 2 个发送点（`httpBridge`、`browser` 旧 WebSocket 通道），
  共用同一契约。
- `nomifun:session-list-project-workpaths-changed` 在新增和删除两个函数中各自
  `dispatchEvent`，计 2 个发送点。
- 辅助函数（如 `dispatchChatMessageJump`、`dispatchWorkspacePanelTabEvent`）的多个
  业务调用方不重复计入发送位置。

### 2.2 纳入范围

- 通过 `new CustomEvent(...)` 或 `new Event(...)` 构造，再经 `window`、`document`
  或具体 DOM 容器 `dispatchEvent` 的事件。
- 名称由常量、变量、辅助函数或动态参数决定的事件（如 `emitWindowEvent(name)`）。
- `common` 与 `renderer` 之间的跨层同窗口事件（认证失效、语音模型回退）。
- `videoCanvas` 容器级预览事件（单独分组）。
- 与事件配套的 `storage` 监听仅作为同步机制记录，不单独计为业务事件。

### 2.3 明确排除

| 排除对象 | 证据／位置 | 理由 |
| --- | --- | --- |
| 浏览器原生交互事件 | `canvas:live-viewport` 的 `Event('scroll')`（`ui/src/renderer/pages/videoCanvas/oc/lib/canvas/canvas-live-viewport.ts:62`）、`Event('input')`（`ui/src/renderer/pages/videoCanvas/oc/components/canvas/canvas-craft-token-chip.tsx:114`）、`PointerEvent('pointerup')`（`ui/src/renderer/pages/videoCanvas/oc/components/canvas/director/director-viewport.tsx:655`）、`PointerEvent('pointercancel')`（`ui/src/renderer/pages/conversation/SessionList/index.tsx:201`） | 原生交互语义，不是业务通信契约 |
| 测试探针 | `ui/src/renderer/pages/test/SupportSurfaceProbe.tsx:430`（`Event('change')`） | 测试探针，不是生产运行路径 |
| 测试文件内的派发 | `*.test.ts` / `*.test.tsx` 中的 `dispatchEvent` | 测试自造事件，不计入生产事件 |
| `emitter.ts` 的 EventEmitter 事件 | `ui/src/renderer/utils/emitter.ts:20-108` | 已在迁移目标通道上（如已迁移的 `workspace.toggle`），不属于本清单的待迁移 `CustomEvent` 面 |
| Tauri 跨窗口事件 | `ui/src/renderer/utils/theme/themeBroadcast.ts:19`（`nomifun://theme-sync`）、`ui/src/common/adapter/tauriEvents.ts` | Tauri 事件通道，跨窗口语义，保留独立通道 |
| 后端 IPC / WebSocket 帧 | `ui/src/common/adapter/ipcBridge.ts`、`ui/src/platform/bridge.ts` | 不是 DOM 同窗口自定义事件 |

> `window` 上的同名事件在静态扫描下无法证明「外部脚本、扩展或另一个窗口」不存在
> 消费者。本文对这类契约只给仓内证据，不据此断言全局无消费者。

## 3. 已迁移参照（不计入本清单）

`workspace.toggle` 已从 `CustomEvent` 迁往 `emitter.ts`，可作为后续批次的样板：

- 契约：`'workspace.toggle'`（`ui/src/renderer/utils/emitter.ts:101`）
- 发送：`dispatchWorkspaceToggleEvent` → `emitter.emit('workspace.toggle', { target })`
  （`ui/src/renderer/utils/workspace/workspaceEvents.ts:22-25`）
- 监听：`subscribeWorkspaceToggleEvent` → `addEventListener('workspace.toggle', ...)`
  （`ui/src/renderer/utils/workspace/workspaceEvents.ts:31-36`）
- 测试：`ui/src/renderer/utils/workspace/workspaceEvents.test.ts`
- 迁移提交：`5ee84a366`

该事件不再计入第 2 节的 31 条契约。**同文件的
`nomifun-workspace-has-files` 仍未迁移**，见 §4.2。

## 4. 逐事件清单

分类图例：

- **建议迁移**：同窗口、发送/消费关系明确，可评估改走 `emitter.ts`。
- **保留现状**：跨层、跨窗口、安全敏感或依赖 DOM／返回值的契约，暂不并入普通 emitter。
- **待确认**：仓内静态扫描找不到消费者、无调用方，或存在外部消费者可能。
- **独立批次**：需要单独规划与验收的事件（主题相关）。

风险图例：**低**（payload 简单、单发单收、清理完整）；**中**（多消费者、作用域过滤、
与 `storage` 双通道或高频）；**高**（安全、跨窗口、依赖返回值、外部契约或外部消费者）。

### 4.1 通用同窗口事件

| 事件（常量） | 作用域 | 发送位置 | 监听位置 | 用途 / payload | 行为契约 | 运行条件 / 清理 | 分类 / 风险 | 状态 / 证据 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `skill-catalog-changed`（`SKILL_CATALOG_CHANGED_EVENT`） | `window` | `ui/src/renderer/hooks/skills/skillCatalogEvents.ts:5`（`notifySkillCatalogChanged`）；调用方 `ui/src/renderer/pages/settings/SkillHubMarketPanel.tsx:99` | `ui/src/renderer/hooks/skills/useSkillCatalog.ts:109`（清理 `:110`）；`ui/src/renderer/pages/settings/SkillsHubSettings.tsx:130`（`:131`）；`ui/src/renderer/hooks/preset/usePresetEditor.ts:161`（`:162`） | 技能目录增删后刷新目录／可用技能；无 payload（`Event`） | 同步；不冒泡、不可取消；不依赖返回值 | 仅 `window` 存在时发送；三处监听均有卸载清理 | 建议迁移 / 低 | 未迁移；无专门测试 |
| `flowy:sider-history-search-select`（`SIDER_HISTORY_SEARCH_SELECT_EVENT`） | `window` | `ui/src/renderer/components/layout/Sider/historySearchSelect.ts:5`（`dispatchSiderHistorySearchSelect`）；调用方 `ui/src/renderer/components/layout/Titlebar/index.tsx:307` | `ui/src/renderer/components/layout/Sider/index.tsx:261`（清理 `:263`） | 标题栏搜索选中会话后让侧栏回到项目视图；无 payload（`Event`） | 同步；不冒泡、不可取消；不依赖返回值 | 仅同窗口；监听有清理 | 建议迁移 / 低 | 未迁移；无专门测试 |
| `nomifun-update-available`（`UPDATE_AVAILABLE_EVENT`） | `window` | `ui/src/renderer/components/layout/Layout.tsx:484-486` | `ui/src/renderer/components/layout/Layout.tsx:547`（清理 `:558`） | 启动/定时检查发现更新后触发后台下载；`{ version: string }` | 同步；不冒泡、不可取消；不依赖返回值 | 桌面更新检查路径；监听在 effect 清理中移除 | 建议迁移 / 低 | 未迁移；`TitlebarUpdateButton.test.ts` 只覆盖 `app.update.open` emitter，不覆盖本事件 |
| `flowy:first-win`（`FIRST_WIN_EVENT`） | `window` | `ui/src/renderer/utils/onboarding/firstWinMode.ts:43`（`markFirstWinCompleted`）；调用方 `ui/src/renderer/pages/commercialSlice/index.tsx:89`、`ui/src/renderer/pages/conversation/platforms/nomi/useNomiMessage.ts:298`、`ui/src/renderer/pages/conversation/Messages/components/MessageText.tsx:544`、`ui/src/renderer/utils/onboarding/firstWinMode.ts:79` | `ui/src/renderer/utils/onboarding/firstWinMode.ts:70`（清理 `:73`） | 首次价值确认完成；`{ completed: true }` | 同步；不冒泡、不可取消；不依赖返回值 | 需 `localStorage` 可用才发送；监听有清理 | 建议迁移 / 低 | 未迁移；`firstWinMode.test.ts:14` 覆盖 `markFirstWinCompleted` |
| `flowy:funnel` | `window` | `ui/src/renderer/utils/analytics/productFunnel.ts:194`（`recordFunnelEvent`） | `ui/src/renderer/utils/onboarding/firstWinMode.ts:71`（清理 `:74`） | 产品漏斗事件；payload 为 `FunnelEvent`（`name`、`props`、`cohort` 等） | 同步；不冒泡、不可取消；不依赖返回值 | 仅 `window` 可派发时发送；监听有清理 | 待确认 / 中 | 未迁移；仓内仅 `firstWinMode` 监听，但事件名与明细可能被外部埋点脚本消费，需外部确认 |
| `nomifun:tts-config-changed`（`TEXT_TO_SPEECH_CONFIG_CHANGED_EVENT`） | `window` | `ui/src/renderer/services/textToSpeechConfig.ts:42` | `ui/src/renderer/pages/modelHub/TextToSpeechContent.tsx:36`（清理 `:37`） | TTS 默认配置保存后刷新表单；无 payload | 同步；不冒泡、不可取消；不依赖返回值 | `finally` 中发送，成功/失败都会通知；监听有清理 | 建议迁移 / 低 | 未迁移；无专门测试 |
| `nomifun:speech-to-text-config-changed`（`SPEECH_TO_TEXT_CONFIG_CHANGED_EVENT`） | `window` | `ui/src/renderer/services/speechToTextConfig.ts:50` | `ui/src/renderer/pages/modelHub/SpeechToTextContent.tsx:40`（清理 `:41`） | STT 配置保存后刷新表单；无 payload | 同步；不冒泡、不可取消；不依赖返回值 | `finally` 中发送；监听有清理 | 建议迁移 / 低 | 未迁移；无专门测试 |
| `voice-dismissed-sessions-changed`（`VOICE_DISMISSED_SESSIONS_EVENT`） | `window` | `ui/src/renderer/services/voice/voiceCallPresence.ts:36`（`dismissVoiceSession`）；调用方 `ui/src/renderer/pages/companion/index.tsx:1843` | `ui/src/renderer/hooks/useCompanionWindowsSync.ts:87`（清理 `:99`）；`ui/src/renderer/pages/companion/index.tsx:181`（清理 `:184`） | 语音会话被忽略后同步伙伴窗口；无 payload（`Event`） | 同步；不冒泡、不可取消；不依赖返回值 | 需 `localStorage` 可用；与 `storage` 及 IPC 同步并用；监听有清理 | 建议迁移 / 中 | 未迁移；`useCompanionWindowsSync.test.ts:25,72` 覆盖 |
| `preview-panel-resize` | `window` | `ui/src/renderer/hooks/ui/useResizableSplit.tsx:68` | **未发现监听** | 分割面板拖动时广播比例；`{ ratio: number }` | 同步；不冒泡、不可取消；不依赖返回值 | 仅 `window.dispatchEvent` 可用时发送；无监听清理问题 | 待确认 / 低 | 未迁移；仓内仅发送方，多路由使用 `useResizableSplit` 但无消费者 |
| `nomifun-chat-message-jump`（`CHAT_MESSAGE_JUMP_EVENT`） | `window` | `ui/src/renderer/utils/chat/chatMinimapEvents.ts:14-18`（`dispatchChatMessageJump`）；调用方 `ConversationQuestionLocator.tsx:285`、`useMinimapPanel.ts:336` | `ui/src/renderer/pages/conversation/Messages/MessageList.tsx:1726`（清理 `:1728`） | 会话小地图/问题定位跳转到指定消息；`ChatMessageJumpDetail`（`conversation_id`、`messageId?`、`msgId?`、`align?`、`behavior?`） | 同步；不冒泡、不可取消；不依赖返回值 | 仅同窗口；监听有清理；按 `conversation_id` 过滤 | 建议迁移 / 低 | 未迁移；`ConversationQuestionLocator.structure.test.ts:29,54` 覆盖 |
| `model-picker-open` | `window` | `ui/src/renderer/pages/videoCanvas/oc/components/model-picker.tsx:85` | `ui/src/renderer/pages/videoCanvas/oc/components/model-picker.tsx:79`（清理 `:80`） | 同一画布内只允许一个模型选择器展开；payload 为 `pickerId: string` | 同步；不冒泡、不可取消；不依赖返回值 | 每个已挂载 picker 实例都会监听并通过 `detail !== pickerId` 关闭其他实例 | 建议迁移 / 中 | 未迁移；无专门测试；多实例语义需保留 |
| `nomifun:session-list-project-workpaths-changed` | `window` | `ui/src/renderer/pages/conversation/SessionList/utils/projectWorkpaths.ts:35`、`:48` | `ui/src/renderer/pages/conversation/SessionList/utils/projectWorkpaths.ts:61`（清理 `:64`） | 项目工作路径新增/删除后通知订阅者；无 payload（`Event`） | 同步；不冒泡、不可取消；不依赖返回值 | 与 `storage` 事件并行（跨窗口）；监听有清理 | 建议迁移 / 低 | 未迁移；无专门测试 |
| `nomifun:workpath-ui-changed` | `window` | `ui/src/renderer/pages/conversation/SessionList/hooks/useWorkpathUiState.ts:51`（`writeJson`） | `ui/src/renderer/pages/conversation/SessionList/hooks/useWorkpathUiState.ts:147`（清理 `:150`） | 工作路径 UI 偏好写入后同步多个已挂载实例；`{ storageKey: string }` | 同步；不冒泡、不可取消；不依赖返回值 | 与 `storage` 事件并行；监听有清理 | 建议迁移 / 低 | 未迁移；无专门测试 |
| `nomifun:session-sidebar-display-preferences-change` | `window` | `ui/src/renderer/pages/conversation/SessionList/hooks/useSidebarDisplayPreferences.ts:35`（`dispatchPreferencesChange`） | `ui/src/renderer/pages/conversation/SessionList/hooks/useSidebarDisplayPreferences.ts:78`（清理 `:81`） | 侧栏显示偏好变更后同步实例；payload 为 `SidebarDisplayPreferences` | 同步；不冒泡、不可取消；不依赖返回值 | 与 `storage` 事件并行；监听有清理 | 建议迁移 / 低 | 未迁移；无专门测试 |
| `workspace:navigate` | `window` | `ui/src/renderer/pages/videoCanvas/oc/lib/settings-navigation.ts:15-16` | **未发现监听** | 深层画布组件请求跳转模型中心；`{ to: string }` | 同步；**`cancelable: true`**；**依赖 `dispatchEvent` 返回值**：无人 `preventDefault` 时返回 `true`，随后 `window.location.assign(to)` 兜底 | 仅仓内静态扫描无监听；外部宿主可能订阅并 `preventDefault` | 待确认 / 高 | 未迁移；不能按普通 emitter 迁移，需先确认宿主契约 |

### 4.2 工作区面板与工作区文件状态

| 事件（常量） | 作用域 | 发送位置 | 监听位置 | 用途 / payload | 行为契约 | 运行条件 / 清理 | 分类 / 风险 | 状态 / 证据 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `nomifun-workspace-panel-tab`（`WORKSPACE_PANEL_TAB_EVENT`） | `window` | `ui/src/renderer/pages/conversation/components/ChatLayout/WorkspaceToolRail.tsx:32-34`（`dispatchWorkspacePanelTabEvent`）；调用方 `ChatLayout/index.tsx:242`、`TerminalSessionPage.tsx:78` | `ui/src/renderer/pages/conversation/hooks/useWorkspacePanelTabs.ts:57`（清理 `:58`）；`ui/src/renderer/pages/conversation/Workspace/WorkspaceRailBody.tsx:117`（清理 `:118`） | 切换工作区工具标签；`WorkspacePanelTabDetail`（`tab: WorkspaceTab`、`target: SessionTarget`） | 同步；不冒泡、不可取消；不依赖返回值 | 同窗口；监听按 `target` 的 `kind + id` 过滤；有清理 | 建议迁移 / 中 | 未迁移；无专门测试；`workspaceToolRail.test.ts` 仅覆盖 `has-files` 常量与 `workspace.toggle` |
| `nomifun-workspace-open-preview-tool`（`WORKSPACE_OPEN_PREVIEW_TOOL_EVENT`） | `window` | `ui/src/renderer/pages/conversation/components/ChatLayout/WorkspaceToolRail.tsx:39-43`（`dispatchWorkspaceOpenPreviewTool`）；调用方 `ui/src/renderer/pages/conversation/execution/ExecutionConversationLayout.tsx:55,62`、`ui/src/renderer/pages/conversation/Messages/components/conversationPlanContext.tsx:53` | `ui/src/renderer/pages/conversation/components/ChatLayout/index.tsx:257`（清理 `:258`） | 从头部/自动打开入口在预览区打开工具；`{ tab, target }` | 同步；不冒泡、不可取消；不依赖返回值 | 同窗口；监听按 `target` 过滤；有清理 | 建议迁移 / 中 | 未迁移；`ConversationPlanPanel.structure.test.ts:68` 覆盖调用 |
| `nomifun-workspace-panel-meta`（`WORKSPACE_PANEL_META_EVENT`） | `window` | `ui/src/renderer/pages/conversation/components/ChatLayout/WorkspaceToolRail.tsx:57`（`dispatchWorkspacePanelMetaEvent`）；调用方 `ui/src/renderer/pages/conversation/Workspace/WorkspaceRailBody.tsx:101` | `ui/src/renderer/pages/conversation/components/ChatLayout/index.tsx:355`（清理 `:356`）；`ui/src/renderer/pages/terminal/TerminalSessionPage.tsx:72`（清理 `:73`） | 同步工作区变更数量给工具栏徽标；`{ target: SessionTarget, changeCount: number }` | 同步；不冒泡、不可取消；不依赖返回值 | 同窗口；监听按 `target` 过滤；随 `changeCount` 变化发送 | 建议迁移 / 中 | 未迁移；无专门测试 |
| `nomifun-workspace-has-files`（`WORKSPACE_HAS_FILES_EVENT`） | `window` | `ui/src/renderer/utils/workspace/workspaceEvents.ts:44-48`（`dispatchWorkspaceHasFilesEvent`）；调用方 `ui/src/renderer/pages/conversation/Workspace/hooks/useWorkspaceTree.ts:153` | **仓内静态扫描未发现监听** | 工作区首次/后续加载出现文件时通知展开面板；`WorkspaceHasFilesDetail`（`target`、`hasFiles`、`isInitial`） | 同步；不冒泡、不可取消；不依赖返回值 | 仅 `window` 存在且 `hasFiles` 为真时发送；无监听清理问题 | **待确认 / 中** | 未迁移；证据见下方说明；**不得据此删除发送逻辑** |
| `nomifun-session-sider-toggle`（`SESSION_SIDER_TOGGLE_EVENT`） | `window` | `ui/src/renderer/utils/workspace/sessionSiderEvents.ts:21`（`dispatchSessionSiderToggleEvent`） | **未发现监听** | 会话二级侧栏切换（历史设计）；无 payload | 同步；不冒泡、不可取消；不依赖返回值 | 仅 `window` 存在时发送；**仓内无调用方** | 待确认 / 中 | 未迁移；疑似遗留通道，静态扫描无发送调用方与监听方 |
| `nomifun-session-sider-state`（`SESSION_SIDER_STATE_EVENT`） | `window` | `ui/src/renderer/utils/workspace/sessionSiderEvents.ts:26-28`（`dispatchSessionSiderStateEvent`） | **未发现监听** | 会话二级侧栏折叠状态同步（历史设计）；`{ collapsed: boolean }` | 同步；不冒泡、不可取消；不依赖返回值 | 仅 `window` 存在时发送；**仓内无调用方** | 待确认 / 中 | 未迁移；疑似遗留通道，静态扫描无发送调用方与监听方 |

**`nomifun-workspace-has-files` 待确认证据：**

- 定义与发送：`ui/src/renderer/utils/workspace/workspaceEvents.ts:5`、`:42-49`
- 唯一发送调用：`ui/src/renderer/pages/conversation/Workspace/hooks/useWorkspaceTree.ts:153`
- 仓内监听搜索：`ui/src` 内无 `addEventListener(WORKSPACE_HAS_FILES_EVENT` 或
  `addEventListener('nomifun-workspace-has-files'` 命中。
- 测试引用：`ui/src/renderer/pages/conversation/components/ChatLayout/workspaceToolRail.test.ts:140`
  只断言常量字符串存在，不验证消费行为；`workspaceEvents.test.ts` 只覆盖已迁移的
  `workspace.toggle`，未覆盖本事件。
- 其他目录（`apps/`、`web/`、`docs/`、`crates/`）同样未发现消费者。
- 结论：**仓内静态扫描未发现消费者，待确认**。可能存在外部脚本、旧构建产物或已删除的
  消费方；本次不删除、不改写。

### 4.3 主题与样式事件（独立批次）

| 事件（常量） | 作用域 | 发送位置 | 监听位置 | 用途 / payload | 行为契约 | 运行条件 / 清理 | 分类 / 风险 | 状态 / 证据 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `custom-css-updated` | `window` | `ui/src/renderer/hooks/ui/useCssTheme.ts:45`（`dispatchCustomCssUpdated`）；`ui/src/renderer/components/layout/Layout.tsx:273` | `ui/src/renderer/hooks/ui/useCssTheme.ts:179`（清理 `:187`）；`ui/src/renderer/components/layout/Layout.tsx:299`（清理 `:303`）；`ui/src/renderer/components/Markdown/ShadowView.tsx:840`（清理 `:843`）；`ui/src/renderer/pages/conversation/components/ConversationTitleMinimap/useMinimapPanel.ts:97`（清理 `:104`） | 自定义 CSS 皮肤更新后注入样式、同步 Shadow DOM 与小地图视觉；`{ customCss: string }` | 同步；不冒泡、不可取消；不依赖返回值；`detail.customCss` 允许空串 | 同窗口；发送前通常已写 `customCss`/`css.activeThemeId`；监听均有清理 | **已迁移 / 中** | **已迁移（Batch 3）**：入口 `ui/src/renderer/utils/theme/themeEvents.ts` 的 `dispatchThemeCustomCssUpdated` / `subscribeThemeCustomCssUpdated`（类型化事件 `theme.custom-css.updated`）；验证：`themeEvents.test.ts`、`themeEvents.react.test.tsx`、`useCssTheme.apply.test.ts` 通过；历史链路见 §5 |
| `nomifun:css-themes-changed`（`CSS_THEMES_CHANGED_EVENT`） | `window` | `ui/src/renderer/hooks/ui/useCssTheme.ts:55`（`notifyCssThemesChanged`）；调用方 `saveUserTheme:228`、`deleteUserTheme:245` | `ui/src/renderer/hooks/ui/useCssTheme.ts:180`（清理 `:188`） | 用户主题列表增删改后让各 `useCssTheme` 实例重新加载；无 payload | 同步；不冒泡、不可取消；不依赖返回值 | 同窗口；监听有清理 | **已迁移 / 低** | **已迁移（Batch 3）**：入口 `ui/src/renderer/utils/theme/themeEvents.ts` 的 `dispatchThemeCssThemesChanged` / `subscribeThemeCssThemesChanged`（类型化事件 `theme.css-themes.changed`）；公开函数 `notifyCssThemesChanged()` 保留；验证：`themeEvents.test.ts`、`themeEvents.react.test.tsx` 通过 |

> **Batch 3 迁移记录（追加）：** 两个事件已改走 `themeEvents.ts` 的类型化事件总线，仓内生产代码不再发送或监听旧 DOM 事件名 `custom-css-updated` / `nomifun:css-themes-changed`（旧名称仅作为 §5 历史记录保留）。发送保持同步、按注册顺序投递，`customCss: ''` 仍表示清除；单个监听异常通过 `globalThis.reportError`（不可用时 `console.error`）报告，不阻塞其他监听者。跨窗口 `nomifun://theme-sync`、companion 接收与 `broadcastCustomCssSync` 未改动。验证命令：`bun test ui/src/renderer/utils/theme/themeEvents.test.ts ui/src/renderer/utils/theme/themeEvents.react.test.tsx ui/src/renderer/utils/theme/useCssTheme.apply.test.ts ui/src/renderer/utils/theme/themeBroadcast.test.ts ui/src/renderer/utils/theme/customCssProcessor.test.ts`、`bun run typecheck`、`bun run check:theme`、`bun run check`、`git diff --check`。

### 4.4 认证与 `common` → `renderer` 跨层事件

| 事件（常量） | 作用域 | 发送位置 | 监听位置 | 用途 / payload | 行为契约 | 运行条件 / 清理 | 分类 / 风险 | 状态 / 证据 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `nomifun:auth-expired`（`AUTH_EXPIRED_EVENT`） | `window` | `ui/src/common/adapter/httpBridge.ts:372-381`（`emitWindowEvent`，调用 `:384`）；旧 WebSocket 通道 `ui/src/common/adapter/browser.ts:42-46` | `ui/src/renderer/hooks/context/AuthContext.tsx:287`（清理 `:288`）；`ui/src/renderer/components/settings/SettingsModal/contents/BrowserUseSettingsContent.tsx:300`（清理 `:302`） | HTTP/WS 证明浏览器会话失效后登出或停止轮询；无 payload | 同步；不冒泡、不可取消；不依赖返回值；动态名称经 `emitWindowEvent` 构造 | WebUI 浏览器会话路径；`AuthContext` 仅在非桌面运行时监听；监听有清理 | **保留现状 / 高** | 未迁移；`httpBridge.test.ts:365`、`BrowserUseSettingsContent.test.ts:1038-1046` 覆盖；跨层安全契约，需先评估依赖方向 |
| `nomifun:cloud-auth-expired`（`CLOUD_AUTH_EXPIRED_EVENT`） | `window` | `ui/src/common/adapter/httpBridge.ts:372-381`（`emitWindowEvent`，调用 `:388`） | `ui/src/renderer/hooks/context/CloudAuthContext.tsx:383`（清理 `:384`） | Flowy 云端凭证失效后弹出重新登录并登出；无 payload | 同步；不冒泡、不可取消；不依赖返回值 | WebUI 云端认证路径；监听有清理 | **保留现状 / 高** | 未迁移；`httpBridge.test.ts:429,471,514`、`CloudAuthContext.contract.test.ts:43` 覆盖 |
| `voice:model-fallback` | `window` | `ui/src/common/adapter/voiceCompanion.ts:577` | `ui/src/renderer/components/layout/Router.tsx:346`（清理 `:347`） | 语音模型回退时提示用户实际模型；`VoiceModelFallback`（`requested`、`actual`、`reason`） | 同步；不冒泡、不可取消；不依赖返回值 | `common` 向 `renderer` 发送；仅在语音启动返回 `model_fallback` 时触发 | **保留现状 / 中** | 未迁移；需先评估 `common → renderer` 依赖方向，不能直接并入 renderer emitter |

### 4.5 `videoCanvas` 专属事件

这些事件绑定在具体画布容器元素上（非 `window`），以逐帧或交互级频率发送，消费方
通过 `subscribe*` 辅助函数在同一容器注册。**不建议并入全局 emitter**。

| 事件（常量） | 作用域 | 发送位置 | 监听位置 | 用途 / payload | 行为契约 | 运行条件 / 清理 | 分类 / 风险 | 状态 / 证据 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `canvas:task-created` | `window` | `ui/src/renderer/pages/videoCanvas/oc/services/api/task-center.ts:653`（`notifyCanvasTaskCreated`，调用 `:496`） | `ui/src/renderer/pages/videoCanvas/oc/pages/canvas/use-canvas-active-tasks.ts:20`（清理 `:21`） | 新生成任务创建后触发活动任务查询刷新；`{ task: GenerationTask }` | 同步；不冒泡、不可取消；不依赖返回值 | 仅 `window` 存在且 `task.projectId` 有效时发送；按 `projectId` 过滤 | 建议迁移 / 低 | 未迁移；无专门测试 |
| `canvas:graphics-viewport-preview`（`CANVAS_GRAPHICS_VIEWPORT_PREVIEW_EVENT`） | 画布容器元素 | `ui/src/renderer/pages/videoCanvas/oc/lib/canvas/canvas-live-viewport.ts:58` | 辅助函数 `canvas-live-viewport.ts:75-79`（注册 `:77`）；消费方 `canvas-leafer-graphics-layer.tsx:125` | 逐帧同步图形层视口变换；`ViewportTransform` | 同步；元素级、不冒泡、不可取消；不依赖返回值 | 每次 `applyCanvasLiveViewport` 都发送，高频；消费方卸载时取消订阅 | 保留现状 / 中 | 未迁移；性能敏感，绑定具体容器 |
| `canvas:viewport-preview`（`CANVAS_VIEWPORT_PREVIEW_EVENT`） | 画布容器元素 | `ui/src/renderer/pages/videoCanvas/oc/lib/canvas/canvas-live-viewport.ts:60` | 辅助函数 `canvas-live-viewport.ts:81-85`（注册 `:83`）；消费方 `canvas-emotion-workspace.tsx:446`、`canvas-mini-map.tsx:110`、`canvas-node-toolbar.tsx:186`、`canvas-zoom-controls.tsx:42`、`infinite-canvas.tsx:97`、`canvas-workspace-overlays.tsx:66,111,152` | 视口预览通知（仅 `notify=true`）；`ViewportTransform` | 同步；元素级、不冒泡、不可取消；不依赖返回值 | 高频；可按 `notify` 节流；消费方卸载时取消订阅 | 保留现状 / 中 | 未迁移；性能敏感 |
| `canvas:node-drag-preview`（`CANVAS_NODE_DRAG_PREVIEW_EVENT`） | 画布容器元素 | `ui/src/renderer/pages/videoCanvas/oc/lib/canvas/canvas-live-viewport.ts:125` | 辅助函数 `canvas-live-viewport.ts:128-132`（注册 `:130`）；消费方 `canvas-leafer-graphics-layer.tsx:147` | 拖动节点时不进入 React 状态的瞬时预览；`CanvasNodeDragPreview \| null` | 同步；元素级、不冒泡、不可取消；不依赖返回值 | 拖动期间高频；消费方卸载时取消订阅 | 保留现状 / 中 | 未迁移；性能敏感 |
| `canvas:selection-preview`（`CANVAS_SELECTION_PREVIEW_EVENT`） | 画布容器元素 | `ui/src/renderer/pages/videoCanvas/oc/lib/canvas/canvas-live-viewport.ts:135` | 辅助函数 `canvas-live-viewport.ts:138-142`（注册 `:140`）；消费方 `canvas-leafer-graphics-layer.tsx:144` | 框选区域预览；`SelectionBox` | 同步；元素级、不冒泡、不可取消；不依赖返回值 | 交互期间高频；消费方卸载时取消订阅 | 保留现状 / 中 | 未迁移；性能敏感 |

## 5. 重点一：`custom-css-updated` 主题链路

### 5.1 同窗口发送与监听

- 发送方 A：`ui/src/renderer/hooks/ui/useCssTheme.ts:44-46` 的
  `dispatchCustomCssUpdated(css)`，被以下路径调用：
  - `applyCssThemeRaw` 成功持久化后（`:72`）
  - 持久化失败后从 `configService.get('customCss')` 恢复（`:77`）
  - `reload()` 自愈 split-brain 后（`:163`）
  - `configService.subscribe('customCss', ...)` 回调（`:184`）
- 发送方 B：`ui/src/renderer/components/layout/Layout.tsx:273`，在
  `loadAndHealCustomCss()` 发现 CSS 实际变化时发送。
- 监听方：
  - `useCssTheme.ts:179`：只重读 `css.activeThemeId`，让主题选择器保持响应。
  - `Layout.tsx:299`：更新 `customCss` state，随后触发样式注入。
  - `ShadowView.tsx:840`：给 Shadow DOM 内的自定义 CSS 加 `!important` 后重设。
  - `useMinimapPanel.ts:97`：刷新小地图弹出层的视觉样式。
- payload：`{ customCss: string }`；空串表示清除自定义 CSS，允许空值。
- 契约：同步派发；默认不冒泡、不可取消；发送方不使用 `dispatchEvent` 返回值。

### 5.2 配置订阅、CSS 注入与 `storage` 监听的关系

1. **持久化轴**：`configService.set('customCss', ...)` 与
   `configService.set('css.activeThemeId', ...)` 是唯一写入源（`useCssTheme.ts:71`）。
2. **同窗口订阅**：`useCssTheme.ts:181-185` 订阅 `css.themes`、`css.activeThemeId`、
   `customCss`；`customCss` 变化会再次 `dispatchCustomCssUpdated`。
3. **同窗口广播**：`custom-css-updated` 让 `Layout`、`ShadowView`、`useMinimapPanel`
   在不重新挂载的情况下更新。
4. **样式注入**：`Layout.tsx:309-405` 根据 `customCss` 维护
   `#user-defined-custom-css` 等 `<style>` 节点，并用 `MutationObserver` 校正顺序。
5. **`storage` 监听**：`Layout.tsx:293-300` 监听 `storage`，当
   `customCss` / `css.activeThemeId` 键跨窗口变化时重新执行
   `loadAndHealCustomCss()`。该监听不替代 `custom-css-updated`，而是跨窗口兜底。
6. **跨窗口广播**：`Layout.tsx:312` 调用
   `broadcastCustomCssSync(customCss)` → `ui/src/renderer/utils/theme/themeBroadcast.ts:46-52`
   → Tauri `nomifun://theme-sync`（`:19`）。
7. **companion 接收**：`ui/src/renderer/pages/companion/index.tsx:1281-1297`
   通过 `tauriEvents.listen(THEME_SYNC_EVENT, ...)` 注入 customCss / wallpaper，
   不挂载 `Layout`，因此不参与同窗口 `custom-css-updated`。

### 5.3 边界与迁移注意

- `custom-css-updated` 与 `nomifun:css-themes-changed` 属于**同窗口事件**，可评估迁往
  `emitter.ts`。
- `nomifun://theme-sync` 属于 **Tauri 跨窗口通道**，必须保留，不并入 emitter。
- 独立批次需覆盖：同步语义（发送即更新，不依赖异步 store）、4 个监听的清理、
  `storage` 兜底不被破坏、companion 跨窗口广播验收、CSS 注入顺序与
  `MutationObserver` 行为。
- 本次不改代码。迁移前应补充/确认：`custom-css-updated` 的类型定义、空串语义、
  失败恢复路径（`useCssTheme.ts:73-82`）与桌面/Web 双宿主行为。

## 6. 重点二：`nomifun-workspace-has-files`

- 契约：`WORKSPACE_HAS_FILES_EVENT = 'nomifun-workspace-has-files'`
  （`ui/src/renderer/utils/workspace/workspaceEvents.ts:5`）
- 发送辅助：`dispatchWorkspaceHasFilesEvent`（`:42-49`）
- 唯一发送调用：`ui/src/renderer/pages/conversation/Workspace/hooks/useWorkspaceTree.ts:153`
- payload：`WorkspaceHasFilesDetail`（`:7-20`），包含 `target: SessionTarget`、
  `hasFiles: boolean`、`isInitial: boolean`；`isInitial` 的注释说明“重新挂载也会再次
  报告 `isInitial: true`”，这是已知语义限制。
- 监听：**仓内静态扫描未发现消费者，待确认**（证据见 §4.2）。
- 测试引用：`workspaceToolRail.test.ts:140` 只做源码字符串断言；没有运行时消费测试。
- 处理结论：**不删除发送逻辑，不断言不存在外部消费者**；后续如需迁移，先补调查或
  先补一个真实消费者再决定。

## 7. 后续批次建议

> 以下仅为路线，不在本次实施。每个批次都应在独立分支上完成，并保留现有界面行为。

### 批次 A：关系明确的同窗口低风险事件

- **涉及事件**：`skill-catalog-changed`、`flowy:sider-history-search-select`、
  `nomifun-update-available`、`flowy:first-win`、`nomifun:tts-config-changed`、
  `nomifun:speech-to-text-config-changed`、`nomifun-chat-message-jump`、
  `nomifun:session-list-project-workpaths-changed`、`nomifun:workpath-ui-changed`、
  `nomifun:session-sidebar-display-preferences-change`、`canvas:task-created`。
- **主要风险**：迁移时把 `window` 监听误改成 emitter 后，`storage`／跨实例兜底失效；
  `chat-message-jump` 的 `conversation_id` 过滤需保留。
- **前置调查**：确认每个 `addEventListener` 的清理路径；确认没有测试或脚本依赖
  `window` 上的事件名。
- **验证重点**：`bun run typecheck`；相关 hook/组件单测；手动验证技能刷新、搜索回项目、
  更新徽标、首次价值、TTS/STT 表单、消息跳转、工作路径与侧栏偏好同步。
- **建议顺序**：先做单发单收的 TTS/STT、更新、搜索、首次价值；再做双通道的
  project/workpath/sidebar 偏好；最后做 `chat-message-jump` 与 `canvas:task-created`。

### 批次 B：`custom-css-updated` 独立批次

- **涉及事件**：`custom-css-updated`、`nomifun:css-themes-changed`。
- **主要风险**：主题是跨窗口、配置订阅、CSS 注入、Shadow DOM、小地图视觉的组合链路；
  稍有不慎会出现样式闪烁、重复注入或 companion 不同步。`nomifun://theme-sync` 不能动。
- **前置调查**：确定事件类型与 payload；确认 `useCssTheme` 的序列队列与失败恢复；
  确认 `Layout` 的 `storage` 兜底与 `MutationObserver` 顺序；确认 companion 的
  `tauriEvents.listen` 验收方式。
- **验证重点**：`bun run check:theme`、`bun run typecheck`、主题切换与自定义 CSS 应用、
  双窗口（主窗口 + companion）、桌面与 Web、监听清理（反复挂载/卸载）、样式节点顺序。

### 批次 C：跨层与无消费者事件先调查

- **涉及事件**：`nomifun:auth-expired`、`nomifun:cloud-auth-expired`、
  `voice:model-fallback`、`nomifun-workspace-has-files`、`preview-panel-resize`、
  `nomifun-session-sider-toggle`、`nomifun-session-sider-state`、`flowy:funnel`、
  `workspace:navigate`、`model-picker-open`。
- **主要风险**：`common → renderer` 的依赖方向、认证安全、`cancelable` + 返回值、
  多实例 picker 语义、外部埋点/宿主消费者。无消费者事件若直接删除，可能破坏外部或
  历史路径。
- **前置调查**：确认 `voice:model-fallback` 是否应保留在 `common`；确认
  `workspace:navigate` 是否有宿主监听；确认 `flowy:funnel` 是否被外部埋点消费；
  为 `nomifun-workspace-has-files` / `preview-panel-resize` / session-sider 两事件
  寻找真实消费者或明确废弃依据。
- **验证重点**：认证失效/云端失效流程、语音回退提示、工作区面板展开、分割面板行为、
  画布模型选择器、埋点与漏斗；无消费者事件只做调查记录，不先删除。
- **结论**：这些事件在调查完成前**保留现状**，不迁入普通 emitter。

### 批次 D：`videoCanvas` 容器级事件保持现状

- **涉及事件**：`canvas:graphics-viewport-preview`、`canvas:viewport-preview`、
  `canvas:node-drag-preview`、`canvas:selection-preview`。
- **理由**：绑定具体容器、逐帧/交互级高频、消费方通过 `subscribe*` 在同一元素注册；
  改走全局 emitter 会丢失元素作用域并增加跨容器串扰风险。
- **验证重点**：画布缩放、平移、框选、节点拖动、图形层跟随、小地图与缩放控件；
  建议只做性能回归验证，不做通信通道替换。

### 门禁建议

在批次 A–C 完成并稳定前，**不新增一刀切的 `CustomEvent` 禁用门禁**。可先增加
“新增窗口事件必须登记到本清单”的评审约定；待迁移覆盖面足够后，再评估 lint/脚本门禁。

## 8. 限制与待确认事项

1. **无消费者事件**：`nomifun-workspace-has-files`、`preview-panel-resize`、
   `nomifun-session-sider-toggle`、`nomifun-session-sider-state`、
   `workspace:navigate` 在仓内静态扫描中找不到消费者。静态搜索无法证明外部脚本、
   旧构建产物或宿主不存在监听。
2. **动态名称**：`httpBridge.ts` 的 `emitWindowEvent(name)` 与 `browser.ts` 的
   `AUTH_EXPIRED_EVENT` 通过变量决定名称；本次已解析出两个实际值
   （`nomifun:auth-expired`、`nomifun:cloud-auth-expired`），未发现其他动态取值。
3. **跨窗口/跨层通道**：`nomifun://theme-sync`（Tauri）、`tauriEvents`、`ipcBridge`、
   `platform/bridge.ts` 不属于同窗口 DOM 事件，未计入 31 条契约。
4. **外部消费者**：`flowy:funnel`、`workspace:navigate` 等可能被埋点或宿主消费，
   本文不据此断言可删除或可改通道。
5. **测试覆盖不均**：多数窗口事件只有结构断言或没有专门测试；迁移前需按批次补测试。
6. **`rg` 不可用**：本次用等价搜索完成盘点；若 CI 需要可复现命令，建议在脚本中
   固定 `rg` 或改用仓库已依赖的搜索工具。

## 9. 复现搜索模式

以下模式用于发现候选（命中行数不等于事件数）：

```text
new\s+(CustomEvent|Event)\s*[<(]
dispatchEvent\s*\(
addEventListener\s*\(|removeEventListener\s*\(
CustomEvent
```

随后按发现的事件名／常量在 `ui/src`、`apps/`、`web/`、`docs/`、`crates/` 中追踪发送方、
监听方、测试与文档引用。对多行构造、变量名、辅助函数与动态名称逐处人工核对。
