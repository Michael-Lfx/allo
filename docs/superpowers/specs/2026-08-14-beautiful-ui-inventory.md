# Beautiful UI 试迁清单

对照 [Beautiful UI](https://www.beautifului.dev/) 的 19 个 AI 界面原语，和 Flowy 对话页现有实现。Beautiful UI 是 MIT、copy-paste，没有 npm 包；本文件只跟踪「有哪些组件、各有哪些状态、迁到哪、迁到哪一步」。

**本轮目标：** 换消息流里的过程带、流式正文、以及各种响应壳。**不换对话框**（Chat 整页、Prompt Bar、侧栏）。一次只换一个外壳，不改消息类型、合并逻辑、发送链路。栈仍是 Arco + CSS modules + UnoCSS + i18n + 主题变量，不把 Tailwind 从 `videoCanvas/oc/` 灌进对话页。

**已落地：** 公开预览页 `#/test/beautiful-ui`。Thinking、Tool Chips、Task Rows、Approval Card、Streaming Text、Context Cards、Code Block、Diff Table、Recommendation Card、Selection Actions、Loading State 已接到真实对话。不进侧栏，**不要求登录**（挂在 `ProtectedLayout` 外，和 `/companion` 一样）。

`bun run dev:web` 或只开前端 `bun run dev:ui` 后打开 `http://localhost:5173/#/test/beautiful-ui`（端口以 Vite 为准）。

## 当前计数

| 集合 | 数量 | 说明 |
| --- | ---: | --- |
| Beautiful UI 组件 | 19 | 官网列出的全部原语 |
| 其中建议试迁 | 11 | 消息流里的过程 + 流式正文 + 响应壳 |
| 其中建议跳过 / 后置 | 8 | 对话框整页、无对应产品面、或搜索不在消息流 |
| Flowy 对话消息类型 | 13 | `TMessageType` |
| Flowy 过程态 | 5 | `TurnDisclosureProcessState` |
| 已进预览页 | 11 / 19 | Thinking、Tool Chips、Task Rows、Approval Card、Streaming Text、Context Cards、Code Block、Diff Table、Recommendation Card、Selection Actions、Loading State 已在 `#/test/beautiful-ui` |
| 已接到对话 | 11 / 19 | Thinking → `MessageThinking`；Tool Chips → 工具头；Task Rows → `TurnProcessDisclosure` / 过程条系统行；Approval Card → 权限 / ACP / tool_group Confirming / 计划门；Streaming Text → `MessageText`；Context Cards → `KnowledgeSearchChip` 命中列表；Code Block → Markdown 开围栏 / 流式代码尾；Diff Table → replace / WriteFile / ACP 改文件证据；Recommendation Card → `SkillSuggestCard`；Selection Actions → `SelectionReplyButton`；Loading State → `MessageListSkeleton` |

状态取值：`未开始` → `预览页` → `对话已接`，或不迁则 `跳过`。

---

## 1. Beautiful UI 组件与状态

变体来自官网公开预览，不是他们的私有源码。迁壳时这些视觉态都要能切出来看。

| # | 组件 | Beautiful UI 状态 / 变体 | Flowy 对应 | 建议 | 进度 |
| ---: | --- | --- | --- | --- | --- |
| 1 | Loading State | Drive / Dots / Orbit；计时中 | `MessageListSkeleton`、`MessageAgentStatus` | 本轮（收尾） | 对话已接 |
| 2 | **Thinking** | Steps / Reasoning / Search / Coding；展开/收起；进行中/等待/完成/失败/取消 | `MessageThinking` | **过程带（对话已接）** | 对话已接 |
| 3 | **Streaming Text** | 流式正文、来源条、follow-up | `MessageText` | **本轮（对话已接）** | 对话已接 |
| 4 | **Approval Card** | 待选多选项、确认前 | `MessagePermission`、`MessageAcpPermission`、`PlanApprovalBanner`、tool_group Confirming | **消息流响应（对话已接）** | 对话已接 |
| 5 | **Tool Chips** | 紧凑 chip；多次 tool call 汇总；pending / running / completed / error / canceled / skipped / invalid_arguments | `MessageToolCall`、`KnowledgeSearchChip`、`MessageToolGroup`、`MessageAcpToolCall`、过程条工具行 | **过程带（对话已接）** | 对话已接 |
| 6 | **Task Rows** | Capsules / List；running / waiting / completed / failed / canceled | `TurnProcessDisclosure` 折叠头、`ProcessTraceItem` 系统行 / `file_summary` | **过程带（对话已接）** | 对话已接 |
| 7 | Chat | 多 Tab、推理回复、composer | `ChatLayout`、`ChatConversation` | 跳过（整页） | 跳过 |
| 8 | Prompt Bar | Vanilla / Rounded / Pill；@ / 模型 / 语音 | `SendBox`、`ComposerSurface` | 跳过（对话框） | 跳过 |
| 9 | Recommendation Card | 置信度、Alternatives、Accept、Needs review、No signal | `SkillSuggestCard`、`MessageSkillSuggest` | **消息流响应（对话已接）** | 对话已接 |
| 10 | Context Cards | 分块 + 来源类型（PDF/CSV 等） | `KnowledgeSearchChip` 命中列表 | **消息流响应（对话已接）** | 对话已接 |
| 11 | Diff Table | 表格内 AI 改稿；此处映射为改文件证据 | `FileChangesPanel`、tool replace 预览、WriteFile 汇总 | **消息流响应（对话已接）** | 对话已接 |
| 12 | Records Table | CRM 网格 | 无 | 跳过 | 跳过 |
| 13 | Filter Table | All / To do / In Progress / Completed | 弱对应（任务/筛选） | 跳过 | 跳过 |
| 14 | Sidebar Nav | 工作区导航 + 搜索 | `Sider`、`ContentSider` | 跳过（整页） | 跳过 |
| 15 | Search | 有结果 / 空状态 | 斜杠命令、会话搜索 | 后置（不在消息流） | 未开始 |
| 16 | Insight Cards | 分页洞察 + 图 | Nomi Insights 页，非对话 | 跳过 | 跳过 |
| 17 | Code Block | 逐行流式代码 | `MarkdownView` 开围栏预览 | **消息流响应（对话已接）** | 对话已接 |
| 18 | Fine-tune Card | 设计属性检查器 | 无 | 跳过 | 跳过 |
| 19 | Selection Actions | Explain / Improve / Shorten / Tone / Grammar | `SelectionReplyButton` | **本轮（对话已接）** | 对话已接 |

本轮要做的 11 个：1–6、9–11、17、19（全部已接）。  
明确不换的 8 个：7 Chat、8 Prompt Bar、12 Records、13 Filter、14 Sidebar、15 Search（后置）、16 Insight、18 Fine-tune。

---

## 2. Flowy 侧：消息类型与必须保住的状态

换壳时这些运行时状态不能丢。权威定义在源码，不在本表。

### 2.1 消息类型（13）

`ui/src/common/chat/chatLib.ts` 的 `TMessageType`：

`text` · `tips` · `tool_call` · `tool_group` · `agent_status` · `permission` · `acp_permission` · `acp_tool_call` · `plan` · `thinking` · `moa_reference` · `skill_load` · `available_commands`

消息通用 `status`：`finish` | `pending` | `error` | `work`。  
`position`：`left` | `right` | `center` | `pop`。

### 2.2 过程条（5）

`TurnDisclosureProcessState`：`running` | `waiting` | `completed` | `failed` | `canceled`。

Beautiful UI Task Rows 只展示 running / failed / completed；接到 `ProcessTraceItem` 系统行时还要覆盖 **waiting**（权限确认）和 **canceled**。过程条里的**工具行已经走 Tool Chips**，不要再当成 Task Rows 重做一遍。

### 2.3 和 Beautiful UI 对得上的运行时状态

| 主题 | Flowy 状态 | 源 |
| --- | --- | --- |
| Thinking | 消息 `thinking` \| `done`；过程条 `running` \| `waiting` \| `completed` \| `failed` \| `canceled`；布局 `standalone` \| `process`；展开受控；展示变体 Steps / Reasoning / Search / Coding | `IMessageThinking`、`MessageThinking`、`thinkingTraceModel` |
| Tool | 归一化 `pending` \| `running` \| `completed` \| `error` \| `canceled`；另有 skipped / invalid_arguments | `NormalizedToolStatus`、`toolChipModel` |
| tool_group 原始 | Success / Error / Canceled / Pending / Executing / Confirming | `MessageToolGroup`、`normalizeToolGroupStatus` |
| ACP tool | `pending` \| `in_progress` \| `completed` \| `failed` \| `canceled`/`cancelled` | `normalizeAcpToolCall`、`resolveToolChipStatusFromAcp` |
| Agent 连接 | connecting / connected / authenticated / session_active / preparing / prepared / disconnected / error | `IMessageAgentStatus` |
| Tips | error / success / warning | `IMessageTips` |
| 知识回写 | started / extracting / writing / written / partial / failed / no_candidate / no_completer / disabled / interrupted | `KnowledgeWritebackStatus` |
| 确认类型 | edit / exec / info / mcp / plan | `MessageToolGroup` confirmation、`kindFromPermissionAction`、计划门 |
| Streaming | `finish` / `!isStreaming` → `done`；其余 `streaming` | `MessageText`、`StreamingText` |

#### Thinking（已接）

Beautiful UI 的 Steps / Reasoning / Search / Coding 是展示变体，不新增消息类型。接到对话时从 `subject` 和思考文本形状推断变体，并把文本拆成时间线条目（pending / running / done）。过程条的 `running | waiting | completed | failed | canceled` 映射到同一套 Thinking 壳（`waiting` 保持等待，`completed` → `done`）。

预览页可切：进行中 / 等待 / 已完成 / 失败 / 已取消；Steps / Reasoning / Search / Coding；展开 + 收起；`standalone` + `process`；空内容 + 长文本；浅色 + 深色；`prefers-reduced-motion`。

壳：`ui/src/renderer/components/beautifulUi/thinking/`。对话适配器：`MessageThinking`（过程条里 `variant='process'`）。

#### Tool Chips（已接）

只换工具**头**，不改工具生命周期。`invalid_arguments` 优先于 skipped，skipped 优先于归一化状态。

| 来源 | 映射到 chip |
| --- | --- |
| `NormalizedToolStatus` | 一对一；skipped / invalid_arguments 另盖 |
| tool_group Success / Executing / Confirming·Pending / Error / Canceled | completed / running / pending / error / canceled |
| 过程条 running / waiting / completed / failed / canceled | running / pending / completed / error / canceled |
| ACP `in_progress` / `failed` / `cancelled` | running / error / canceled；对话里先 `normalizeAcpToolCall` 再走归一化映射 |

接到的头：

- `MessageToolCall` 普通工具；`replace`/`Edit` 完成态头也用 chip，diff 已接 Diff Table（经 `FileChangesPanel` 适配器）
- `KnowledgeSearchChip` 头用 chip，命中列表已接 Context Cards
- `MessageToolGroup` 普通工具头用 chip
- `MessageAcpToolCall` 头用 chip，rawInput / diff / artifact 仍在头下面
- `ProcessTraceItem` 的 `ToolTraceRow` / `ToolFileGroupTraceRow` 用 chip，详情面板可展开

**这一刀故意没换（留给后面的原语）：**

- ImageGeneration / VideoGeneration 媒体展示

预览页可切：7 种 chip 状态；row / stack；典型 / 空 / 长路径 / 全部状态（一次排出 7 种）。

壳：`ui/src/renderer/components/beautifulUi/toolChips/`。

#### Task Rows（已接）

Beautiful UI 只有 running / failed / completed；Flowy 还要 **waiting** 和 **canceled**。系统行一对一映射。折叠头保持生命周期文案：`failed` 仍显示成 completed（失败细节在下面的行里）。

接到的面：

- `TurnProcessDisclosure` 折叠头用 `TaskGroup`（无过程项时静态、有过程项时可展开；「展开全部思考」按钮保留）
- `ProcessTraceItem` 系统行、`file_summary`、agent_status、tips、artifact 用 `TaskRows` list
- 正文段落行仍用原来的图标+段落，不是 Task Rows
- 工具行仍是 Tool Chips；等待中的权限卡已接 Approval Card（`MessagePermission` / `MessageAcpPermission`）

预览页可切：Capsules / List；进行中 / 等待 / 已完成 / 失败 / 已取消；典型（嵌套组）/ 空 / 长文本 / 全部状态。

壳：`ui/src/renderer/components/beautifulUi/taskRows/`。

#### Approval Card（已接）

Beautiful UI 的 Approval Card 是待选多选项 + 确认前。不新增消息类型。确认 IPC 不变：`ipcBridge.conversation.confirmation.confirm`、`ipcBridge.conversation.confirmMessage`、`ipcBridge.agentExecution.approve`。

| 来源 | 映射到 kind |
| --- | --- |
| tool_group `confirmationDetails.type` edit / exec / info / mcp | 一对一（`kindFromConfirmationType`） |
| `MessagePermission` `action` edit / exec / info / mcp | 一对一；未知 / 缺省 → `info`（`kindFromPermissionAction`） |
| ACP `tool_call.kind` `execute` | `exec`；其余走 `kindFromPermissionAction` |
| `PlanApprovalBanner` | `plan` |

接到的面：

- `MessagePermission` 换掉 Arco `Card` 壳；截图 / 命令走 `children`；外层仍保留 `data-testid='message-permission-card'`
- `MessageAcpPermission` 同一张卡；确认仍走 `conversation.confirmMessage.invoke`
- `MessageToolGroup` `ConfirmationDetails`：问题 + 选项 + 确认走 Approval Card；`EditConfirmationDiff` / exec markdown / info / mcp 文本仍是 `children`；`onConfirm` 仍调 `ipcBridge.conversation.confirmMessage`
- `PlanApprovalBanner`：`kind='plan'`，单选项 `approve`，仍调 `ipcBridge.agentExecution.approve.invoke`，版本冲突刷新保留

命令 / 截图证据不重做成 Approval Card 内部协议。改文件证据已接 Diff Table（`EditConfirmationDiff` 仍走 `FileChangesPanel` 适配器）。

预览页可切：edit / exec / info / mcp / plan；假数据一问三选项（冰淇淋文案）；浅色 + 深色；`prefers-reduced-motion` 关掉选项高亮动画。

壳：`ui/src/renderer/components/beautifulUi/approvalCard/`。

#### Streaming Text（已接）

Beautiful UI 的 Streaming Text 是流式正文 + 来源条 + follow-up。不新增消息类型。只换 markdown / 纯文本正文框，不改发送链路、合并逻辑、划词编辑。

| 来源 | 映射到壳 |
| --- | --- |
| `message.status === 'finish'` 或 `!isStreaming` | `done`（隐藏 caret） |
| 其余进行中的助手正文 | `streaming`（末行 caret；`prefers-reduced-motion` 时静态、不闪） |

接到的面：

- `MessageText` 把 markdown / 纯文本 / JSON 折叠体包进 `StreamingText`
- 复制 / 时间、`hideActions`、`actionsOnly`、轮次积分、知识回写、文件标记、think-tag 剥离、skill-suggest 剥离、`CollapsibleContent`、`MESSAGE_BODY_*` 都保留
- `splitStreamingMarkdown` 仍负责开围栏代码尾；开围栏尾已接 Code Block
- 对话里**没有** follow-up 列表，不在 `MessageText` 里发明 follow-up chips；预览页可单独展示假 follow-up
- 来源条是可选 `sourcesLabel`；`MessageText` 目前没有来源列表，不传

预览页可切：streaming / done；典型 / 空 / 长文本；假冰淇淋段落；浅色 + 深色；`prefers-reduced-motion` 关掉 caret 闪烁。

壳：`ui/src/renderer/components/beautifulUi/streamingText/`。对话适配器：`MessageText`。

#### Context Cards（已接）

Beautiful UI 的 Context Cards 是分块 + 来源类型（PDF / CSV 等）。不新增消息类型。只换知识检索的命中列表，不改 `parseHits` / `parseHitCount`，头仍是 Tool Chip。

| 来源 | 映射到 `sourceKind` |
| --- | --- |
| `.pdf` | `pdf` |
| `.csv` / `.xlsx` / `.xls` | `csv` |
| `.md` / `.markdown` | `md` |
| `.ts` / `.tsx` / `.js` / `.jsx` / `.rs` / `.py` / `.go` / `.json` / `.toml` | `code` |
| 其余路径 | `other` |

接到的面：

- `KnowledgeSearchChip` 展开后的命中列表换成 `ContextCards`；标题用 `heading || path`，摘要用 `snippet`，来源标签用 `path`
- `onOpen` 仍走原来的 `navigate`：有 `kbId` 时 `/knowledge/:id?highlight=`，否则 `/knowledge`
- 头仍是 `ToolChip`；无法解析成命中时仍展示原始 `output` 面板
- 对话里**没有** Beautiful UI 的「All chunks / 字符数」统计条，不在命中列表上发明计数

预览页可切：典型（PDF + CSV 冰淇淋文案）/ 空（No chunks）；浅色 + 深色；`prefers-reduced-motion` 关掉卡片边框过渡。

壳：`ui/src/renderer/components/beautifulUi/contextCards/`。对话适配器：`KnowledgeSearchChip`（只换命中列表）。

#### Code Block（已接）

Beautiful UI 的 Code Block 是逐行流式代码。不新增消息类型。只换开围栏代码壳，不改 inline `` `code` ``，也不改 `MESSAGE_BODY_FONT_SIZE` / `MESSAGE_BODY_LINE_HEIGHT`。

| 来源 | 映射到壳 |
| --- | --- |
| Markdown 多行围栏 | `BeautifulUiCodeBlock` 包住现有 `SyntaxHighlighter`；header 用 language；Copy 走壳上的按钮 |
| `MessageText` 开围栏尾 `tailKind === 'code'` | `streaming=true`；header 用 `codeLanguage`；正文 `white-space: pre` |
| 单行 inline `code` | 不换 |

接到的面：

- `ui/src/renderer/components/Markdown/CodeBlock.tsx` 的多行围栏换成 Beautiful UI 壳，高亮器仍是原来的 `SyntaxHighlighter`
- KaTeX / Mermaid / 单行 inline `code` 仍走原来的分支
- `MessageText` 开围栏流式尾换成同一块壳；`splitStreamingMarkdown` 仍负责切开稳定前缀和代码尾
- 折叠/展开仍在 Markdown 适配器里（toolbar + footer），不做成新消息类型

预览页可切：streaming / done；`churn.ts` TypeScript 冰淇淋夹具；浅色 + 深色；`prefers-reduced-motion` 关掉 caret 闪烁。

壳：`ui/src/renderer/components/beautifulUi/codeBlock/`。对话适配器：Markdown 围栏 + `MessageText` 开围栏尾。

#### Diff Table（已接）

Beautiful UI 的 Diff Table 是表格内 AI 改稿；此处映射为改文件证据：路径、+ins/−del、展开/预览。不做成通用 CRM 表。不新增消息类型。

| 来源 | 映射到壳 |
| --- | --- |
| `FileChangeItem.file_name` / `fullPath` | `DiffTableFile.title` / `id` |
| `insertions` / `deletions` | `+N` / `-M` 统计 |
| 点击文件名 | `onFileClick`（打开文件预览） |
| 点击 +/− 统计 | `onDiffClick`（打开 diff） |

接到的面：

- `FileChangesPanel` 换成 `DiffTable` 薄适配器；仍导出 `FileChangeItem`，签名不变
- `MessageToolCall` replace / Edit 完成态预览仍走 `FileChangesPanel`
- `MessageAcpToolCall` diff 仍走 `FileChangesPanel`
- `MessageFileChanges` WriteFile 汇总仍走 `FileChangesPanel`
- `MessageToolGroup` `EditConfirmationDiff` 仍走 `FileChangesPanel`

对话里**没有** Beautiful UI 官网那种通用数据网格，不把 Diff Table 做成 CRM / Filter Table。

预览页可切：两个假文件（`churn.ts` + `reorder.ts`，开心果文案路径）；浅色 + 深色；`prefers-reduced-motion` 关掉展开箭头动画。

壳：`ui/src/renderer/components/beautifulUi/diffTable/`。对话适配器：`FileChangesPanel`。

#### Recommendation Card（已接）

Beautiful UI 的 Recommendation Card 是置信度 + Alternatives + Accept。不新增消息类型。只换技能建议壳，不改 save / dismiss artifact IPC。解析器没有 confidence 字段，**不发明假仪表**。

| 来源 | 映射到 `tone` |
| --- | --- |
| 默认有效 `SkillSuggestion` | `high` |
| 空建议（无 name / description / content） | `none` |
| 预览页 Alternatives / Needs review | `alternatives` / `review`（假数据；对话里没有） |

接到的面：

- `SkillSuggestCard` 换成 `RecommendationCard`；Accept → `ipcBridge.cron.saveSkill`，Dismiss → `conversation.updateArtifact` `dismissed`，展开 markdown 仍在 `body`
- `useUpdateConversationArtifactStatus` 保留；已保存 / 已忽略仍不渲染
- `MessageSkillSuggest` 仍是薄包装，不改 payload 解析
- 对话里**没有** confidence 数值，不画假 meter；预览页可切 High confidence / Alternatives / Accept / Needs review / No signal

预览页可切：High confidence / Alternatives / Accept / Needs review / No signal；假冰淇淋补货文案；浅色 + 深色；`prefers-reduced-motion` 关掉备选边框过渡。

壳：`ui/src/renderer/components/beautifulUi/recommendationCard/`。对话适配器：`SkillSuggestCard`。

#### Selection Actions（已接）

Beautiful UI 的 Selection Actions 是划词后的 Explain / Improve / Shorten / Tone / Grammar。不新增消息类型，也不新增 IPC。对话里现有能力只有 `sendbox.reply` 引用，**没有** rewrite 发射器，所以不发明解释 / 改写 / 缩短 / 语气 / 语法的发送模板。

| 来源 | 映射到 `id` |
| --- | --- |
| 现有划词引用 / 回复 | `quote` → `emitter.emit('sendbox.reply', { messageId, content, position })` |
| Explain / Improve / Shorten / Tone / Grammar | 仅预览页假按钮；对话不渲染 |

接到的面：

- `SelectionReplyButton` 换成 `SelectionActions` 壳；`getEffectiveSelection` / Shadow DOM 选区、消息容器查找、移动端禁用、滚动清除都保留
- 对话只传 `id: 'quote'`，文案仍是 `common.reply`
- 浮动条 `position: absolute`，用 `top` / `left`；适配器用 `position: fixed` 原点宿主，让视口坐标落到绝对定位上
- 对话里**没有** rewrite emitter，不把五键接到 `sendbox.fill` 或新 IPC

预览页展示五键（Explain / Improve / Shorten / Tone / Grammar）+ 假开心果划词；浅色 + 深色；`prefers-reduced-motion` 关掉按钮背景过渡。

壳：`ui/src/renderer/components/beautifulUi/selectionActions/`。对话适配器：`SelectionReplyButton`。

#### Loading State（已接）

Beautiful UI 的 Loading State 是 Drive / Dots / Orbit + 计时。不新增消息类型。对话打开线程只选 **一种** 语言（`drive`），避免和过程条两套加载态。过程条 `agent_status` 行已经是 Task Rows，**不重做成 Loading State**。

| 来源 | 映射到壳 |
| --- | --- |
| `MessageListSkeleton`（会话历史加载） | `variant='drive'`；文案 `conversation.skeleton.opening` |
| 预览页 Drive / Dots / Orbit | 一对一；假冰淇淋文案 Churning / Thinking / Searching；计时 `4s` |
| `ProcessTraceItem` `agent_status` | 不换；仍走 Task Rows |
| `PendingConversationOverlay`（新会话过渡） | 不换；仍是 ChatLayout 静态副本 + Arco `Spin`（停线 / 对话框过渡，不是 Loading State 未完成工作） |

接到的面：

- `MessageListSkeleton` 换成 `LoadingState`；外层滚动容器仍保留 `data-testid='message-list-skeleton'`
- 对话不传 `elapsedSeconds`（打开线程通常很快）；预览页传整数秒，壳格式化成 `Ns`
- `prefers-reduced-motion` 关掉 Drive 波前、Dots 跳动、Orbit 旋转和标签 shimmer
- 新会话 `PendingConversationOverlay` 故意不换：它复制 Chat / Prompt Bar 布局，助手行仍用 Arco `Spin`，属于停线外的整页过渡，不是 Loading State 漏面

预览页可切：Drive / Dots / Orbit；浅色 + 深色；`prefers-reduced-motion` 关掉旋转 / 波前。

壳：`ui/src/renderer/components/beautifulUi/loadingState/`。对话适配器：`MessageListSkeleton`。

---

## 3. 对话页现有壳（换壳时改这些文件）

| 壳 | 路径 | 和 Beautiful UI |
| --- | --- | --- |
| 思考 | `.../Messages/components/MessageThinking.tsx` | Thinking（已接） |
| Thinking 原语 | `ui/src/renderer/components/beautifulUi/thinking/` | Thinking 壳 |
| 正文 | `.../Messages/components/MessageText.tsx` | Streaming Text（已接）；开围栏尾 Code Block（已接） |
| Streaming Text 原语 | `ui/src/renderer/components/beautifulUi/streamingText/` | Streaming Text 壳 |
| 工具调用 | `.../Messages/components/MessageToolCall.tsx` | Tool Chips（已接） |
| ACP 工具 | `.../Messages/acp/MessageAcpToolCall.tsx` | Tool Chips（已接） |
| 工具组 / 确认 | `.../Messages/components/MessageToolGroup.tsx` | 头已接 Tool Chips；Confirming 已接 Approval Card；edit diff 已接 Diff Table |
| Tool Chips 原语 | `ui/src/renderer/components/beautifulUi/toolChips/` | Tool Chips 壳 |
| 过程行 | `.../Messages/components/ProcessTraceItem.tsx` | 工具行 Tool Chips；系统行 Task Rows（已接） |
| 过程折叠 | `.../Messages/components/TurnProcessDisclosure.tsx` | Task Rows（已接） |
| Task Rows 原语 | `ui/src/renderer/components/beautifulUi/taskRows/` | Task Rows 壳 |
| 权限 | `.../Messages/components/MessagePermission.tsx` | Approval Card（已接） |
| ACP 权限 | `.../Messages/acp/MessageAcpPermission.tsx` | Approval Card（已接） |
| 计划批准 | `.../conversation/execution/PlanApprovalBanner.tsx` | Approval Card（已接） |
| Approval Card 原语 | `ui/src/renderer/components/beautifulUi/approvalCard/` | Approval Card 壳 |
| 技能建议 | `.../Messages/components/SkillSuggestCard.tsx` | Recommendation Card（已接） |
| Recommendation Card 原语 | `ui/src/renderer/components/beautifulUi/recommendationCard/` | Recommendation Card 壳 |
| 知识检索 | `.../Messages/components/KnowledgeSearchChip.tsx` | 头已接 Tool Chips；命中列表已接 Context Cards |
| Context Cards 原语 | `ui/src/renderer/components/beautifulUi/contextCards/` | Context Cards 壳 |
| 代码围栏 | `ui/src/renderer/components/Markdown/CodeBlock.tsx` | Code Block（已接） |
| Code Block 原语 | `ui/src/renderer/components/beautifulUi/codeBlock/` | Code Block 壳 |
| 改文件证据 | `ui/src/renderer/components/base/FileChangesPanel.tsx` | Diff Table（已接） |
| Diff Table 原语 | `ui/src/renderer/components/beautifulUi/diffTable/` | Diff Table 壳 |
| 划词 | `.../Messages/components/SelectionReplyButton.tsx` | Selection Actions（已接） |
| Selection Actions 原语 | `ui/src/renderer/components/beautifulUi/selectionActions/` | Selection Actions 壳 |
| 骨架 | `.../Messages/components/MessageListSkeleton.tsx` | Loading State（已接） |
| Loading State 原语 | `ui/src/renderer/components/beautifulUi/loadingState/` | Loading State 壳 |
| 新会话过渡 | `.../ConversationShell/PendingConversationOverlay.tsx` | 停线外；ChatLayout 副本 + Arco Spin，不是 Loading State 漏面 |
| 预览页 | `ui/src/renderer/pages/beautifulUiPreview/` | `#/test/beautiful-ui` |
| 输入框 | `ui/src/renderer/components/chat/SendBox/` | Prompt Bar（第一轮不改） |
| 布局 | `.../conversation/components/ChatLayout/` | Chat / Sidebar（第一轮不改） |

---

## 4. 试迁顺序

**停线：** 消息列里的过程、正文、响应都换成 Beautiful UI 壳。输入框、多 Tab 对话框、侧栏仍是 Flowy。完成后大约 **11 / 19 对话已接**，不是整站换皮。

实施计划：`docs/superpowers/plans/2026-08-14-beautiful-ui-message-stream.md`。

过程带（已完成）

1. Thinking → `MessageThinking`
2. Tool Chips → 工具头 / ACP / 过程条工具行
3. Task Rows → `TurnProcessDisclosure` / 过程条系统行

消息流响应

4. Approval Card → `MessagePermission` / `MessageAcpPermission` / `PlanApprovalBanner` / tool_group Confirming（已接）
5. Streaming Text → `MessageText`（流式正文；不改发送链路）（已接）
6. Context Cards → 知识检索命中列表（已接）
7. Code Block → Markdown 开围栏的流式代码（已接）
8. Diff Table → replace / WriteFile 的改文件证据（不要做成通用表格）（已接）
9. Recommendation Card → `SkillSuggestCard`（已接）
10. Selection Actions → `SelectionReplyButton`（划在新正文上才有意义）（已接）
11. Loading State → 骨架 / 等待态（已接；过程条 agent_status 仍走 Task Rows）

每一步：假数据预览 → 主题变量化 → 接到上表对应壳 → 更新本文件的「进度」列，并把映射写进第 2.3 节。

---

## 5. 怎么改这份文档

换进度时改第 1 节表格的「进度」列，并重算文首「当前计数」。接到对话后，把状态映射、预览组合、以及故意留给后续原语的壳写进第 2.3 节和第 3 节，避免下一刀重复或漏面。

- 预览页加了一个组件：该行改为 `预览页`，`已进预览页` +1。
- 接到真实对话：改为 `对话已接`，`已接到对话` +1。
- 决定不迁：改为 `跳过`，并在「建议」列写原因。
- Beautiful UI 官网若增删组件：改文首 19，并增删第 1 节行。
- Flowy 若增删 `TMessageType`：改 2.1，并核对本表映射。

不要把源码行号抄进表里；状态以 `chatLib.ts`、`normalizeToolCall.ts`、`turnDisclosureModel.ts`、`thinkingTraceModel.ts`、`toolChipModel.ts`、`taskRowModel.ts` 为准。
