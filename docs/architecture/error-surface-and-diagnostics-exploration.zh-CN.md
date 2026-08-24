# 错误弹窗与错误详情展示探索记录

> **最后维护：** 2026-08-24（元数据维护，未重写结论）· 核对基准：commit `d791691c6` ·
> 文档性质：探索/审查记录（时点快照）· 内容基线：2026-08-18

## 文档目的

本文记录错误弹窗、错误 Toast、会话错误卡片和渲染错误边界的现有实现，作为后续“报错详情直接展示”修复的依据。

本记录只描述已经从源码和依赖中确认的事实，不代表已经实施修复。

- 探索日期：2026-08-18
- 探索分支：`fix/error-modal-details`
- 基线：当前分支从最新 `main` 创建
- 探索初始状态：未修改源码，未提交；后续实施记录见本文第十三节
- 无关变更：现有 `Cargo.lock` 脏改动保留，未触碰

## 总结结论

项目没有单一的全局错误弹窗。当前至少存在以下四类错误展示入口：

1. 会话/模型运行错误：后端结构化事件经过 WebSocket 进入 `MessageTips`。
2. 普通 HTTP/API 错误：后端返回 `{ error, code, details }`，前端包装成 `BackendHttpError`。
3. 页面操作失败：大量调用 `Message.error` 或 `message.error`，属于短时 Toast。
4. React 渲染崩溃：由 `RouteErrorBoundary` 直接展示错误栈。

最符合“用户截图时看不到报错详情”的代码原因是：

- 结构化会话错误的 `detail` 已经产生并传到前端，但被放在默认收起的 Arco `Collapse` 中；
- 普通错误经常被调用方降级成 `error.message` 或 `String(error)`，结构化 `details` 没有被展示；
- `Message.error` 是自动消失的 Toast，ACP 发送错误当前显式设置为 6 秒或 8 秒；
- 真正的 `Modal.error` 只接收一段字符串，没有统一的错误详情渲染器、复制能力和长文本滚动策略。
- 对话输入框还存在一条独立的前端运行时异常链路：`ComposerSkillTokenInput` 在 `beforeinput` 事件中直接对可能缺失的 `inputType` 调用 `.includes`，该异常目前只会进入控制台，不会自动转换为错误弹窗。
- 错误卡片点击编辑后，编辑重发路径会刻意延迟清空 Composer；当结果属于 `post_mutation_failure` 时，当前状态机还会显式保留文本，因此“点击发送后输入框仍有原文”是现有恢复契约的结果，而不是普通发送路径的行为。

## 一、会话错误的完整来源链路

### 1. 后端错误模型

会话错误使用 `AgentStreamErrorData`，字段包括：

- `message`：面向用户的摘要；
- `incident_id`：一次错误的关联 ID；
- `code`：稳定的机器错误码；
- `ownership`：错误归属；
- `detail`：经过清理后的技术详情；
- `workspacePath`：工作区相关信息；
- `retryable`、`feedback_recommended`、`resolution`：恢复和反馈建议。

主要位置：

- `crates/backend/nomifun-api-types/src/agent_error.rs`
- `crates/backend/nomifun-ai-agent/src/protocol/send_error.rs`

`AgentSendError::new` 会对详情做以下处理：

- 去除简单标记内容；
- 脱敏 URL 查询参数和敏感 Header/Token；
- 最多保留 1000 个字符。

因此后续 UI 应展示这份安全详情，不应重新渲染未经处理的原始 ACP stderr 或完整 HTTP body。

### 2. 后端持久化和实时转发

`StreamRelay` 在终止错误时会同时：

1. 将错误写入 `tips` 消息，内容中保留完整的 `error` 对象；
2. 将错误作为 `message.stream` 的 `type: "error"` 事件发送给 WebSocket 客户端。

主要位置：

- `crates/backend/nomifun-conversation/src/stream_relay.rs:3582`
- `crates/backend/nomifun-conversation/src/stream_relay.rs:3836`
- `crates/backend/nomifun-conversation/src/stream_relay.rs:3957`

持久化内容类似：

```json
{
  "content": "The model provider rejected the request",
  "type": "error",
  "error": {
    "message": "The model provider rejected the request",
    "code": "USER_LLM_PROVIDER_AUTH_FAILED",
    "incident_id": "...",
    "detail": "Provider returned 401.",
    "retryable": false
  },
  "turn_id": "..."
}
```

### 3. 前端转换

`ui/src/common/chat/chatLib.ts` 中的 `transformMessage` 将实时 `error` 或 `tips` 事件转换成 `IMessageTips`，`normalizeAgentStreamError` 会保留 `detail`、`code`、`incident_id` 等结构化字段。

因此当前主要问题不是“后端没有返回详情”或“历史恢复丢失详情”，而是后续展示策略。

## 二、`MessageTips` 的当前展示问题

结构化错误分支会展示：

- 错误归属；
- 重试状态；
- 错误码；
- 用户可读标题和正文；
- 恢复建议；
- 重试、修复配置、编辑、反馈操作。

技术详情通过以下结构渲染：

```tsx
<Collapse bordered={false} className='message-error-note__details'>
  <Collapse.Item name='technical-details'>
    <div className='message-error-note__detail-body'>...</div>
  </Collapse.Item>
</Collapse>
```

位置：`ui/src/renderer/pages/conversation/Messages/components/MessageTips.tsx:253`

当前没有传入 `defaultActiveKey`。Arco Collapse 的默认 `activeKeys` 是空数组，因此技术详情默认收起。项目现有结构测试还明确检查没有默认展开配置：

- `ui/src/renderer/pages/conversation/Messages/components/MessageTips.structure.test.ts:30`

非结构化错误走另一条路径：

```tsx
<CollapsibleContent maxHeight={48} defaultCollapsed={true} useMask={true}>
```

位置：`ui/src/renderer/pages/conversation/Messages/components/MessageTips.tsx:373`

这会让长错误正文只显示前 48px，用户截图时可能只看到摘要或遮罩。

## 三、普通 HTTP/API 错误链路

### 1. 后端错误响应

标准 HTTP 错误格式定义在：

- `crates/backend/nomifun-api-types/src/response.rs:62`
- `crates/backend/nomifun-common/src/error.rs:137`

格式为：

```json
{
  "success": false,
  "error": "...",
  "code": "...",
  "details": {}
}
```

`details` 是可选字段，并不是所有 `AppError` 都有结构化详情。目前常见结构化详情包括工作区路径问题、Provider 占用等；MCP 路由也支持自定义详情。

### 2. 前端包装

`BackendHttpError` 会保存：

- `backendMessage`；
- `code`；
- `details`；
- 脱敏后的 `body`；
- HTTP 状态码。

位置：`ui/src/common/adapter/httpBridge.ts:190`

但 `Error.message` 本身是通用诊断字符串，很多调用方只使用：

```ts
error.message
String(error)
```

这样会让 UI 无法按 `code/details` 做结构化展示。工作区创建错误是少数已经主动读取 `error.details` 的例子，可参考：

- `ui/src/renderer/pages/conversation/utils/conversationCreateError.ts`

## 四、真正的 `Modal.error` 和样式风险

当前源码中精确搜索到的 `Modal.error` / `Modal.warning` 主要集中在两个模块：

### `KnowledgeTagManagementModal`

位置：`ui/src/renderer/pages/knowledge/KnowledgeTagManagementModal.tsx:293`

问题：

- `errorText` 对未知对象返回空字符串；
- `BackendHttpError` 会被当成普通 `Error`，展示的是通用 `message`，没有单独展示 `backendMessage`、`code`、`details`；
- 没有复制、滚动或错误详情分段。

### `ArtifactPreviewPanel`

位置：`ui/src/renderer/pages/videoGeneration/components/ArtifactPreviewPanel.tsx:99`

问题：

- 使用 `e.message` 或 `String(e)`；
- 没有统一解析结构化错误；
- 长错误字符串没有专用详情容器。

### Arco DOM 和项目 CSS

Arco 2.66 的 `Modal.error` 通过 simple modal 渲染，传入的 `content` 最终作为 `.arco-modal-content` 的 children。项目中部分旧选择器仍指向 `.arco-modal-confirm-content`，与当前实际 DOM 不一致，不能作为错误详情样式的可靠挂载点。

项目当前对普通 modal 设置了：

```css
.arco-modal {
  max-height: calc(100dvh - 32px);
  overflow: hidden;
}
```

位置：`ui/src/renderer/styles/arco-override.css:330`

simple modal 的 content 规则目前只处理颜色、字号和行高，没有 `overflow: auto`、`white-space: pre-wrap`、`overflow-wrap: anywhere` 等详情展示约束。因此超长错误内容存在裁切风险，仍需要真实浏览器验证后再定最终样式。

## 五、`Message.error` Toast

`Message.error` 和 `message.error` 是项目中数量最多的错误入口，它们不是对话框：

- 默认自动关闭时间约为 3000ms；
- ACP 初次发送和发送失败显式使用 6000ms/8000ms；
- 大量调用只传一段字符串，无法展示完整结构化详情；
- Toast 全局位置还经过 `layout.css` 和 `arco-override.css` 调整。

ACP 典型位置：

- `ui/src/renderer/pages/conversation/platforms/acp/useAcpInitialMessage.ts:154`
- `ui/src/renderer/pages/conversation/platforms/acp/AcpSendBox.tsx:336`

这条链路适合展示短摘要，不适合作为开发排查详情的唯一载体。

## 六、对话输入事件异常与错误弹窗的关系

本节中的直接 `.includes` 代码和“尚未覆盖”结论是实施前快照；最终修复状态见第十三、十四节。

用户反馈的异常：

```text
Uncaught TypeError: Cannot read properties of undefined (reading 'includes')
at handleBeforeInput (ComposerSkillTokenInput.tsx:614:86)
```

对应实施前源码中的：

```tsx
const nativeEvent = event.nativeEvent as InputEvent;
if (isComposingRef.current || nativeEvent.isComposing || nativeEvent.inputType.includes('Composition')) {
  return;
}
```

位置：`ui/src/renderer/components/chat/ComposerSkillTokenInput.tsx:608`

已确认事实：

- `handleBeforeInput` 通过 `onBeforeInput` 绑定在 contentEditable 输入框上，发生在打字、粘贴和输入法组合阶段，不属于提交请求本身；
- `as InputEvent` 只是编译期断言，不会保证运行时事件拥有 `inputType`；本次堆栈说明实际事件的 `inputType` 为 `undefined`；
- 该组件没有调用 `Modal.error`、`Message.error` 或其他错误展示 API；
- `runtimePatches` 只过滤 ResizeObserver 噪声，真实错误仍然向控制台传播；
- `RouteErrorBoundary` 主要处理渲染阶段异常，不能把事件处理器异常自动转成页面错误面板；
- SendBox 对 `onSend()` Promise 的 `catch` 只覆盖提交异步失败，不能捕获这个同步的 `beforeinput` 异常。

因此，这条异常不是当前错误弹窗的直接来源。它可能间接影响用户体验：当前输入事件的自定义处理会被中断，文字、中文输入法预编辑内容、技能 Token 或光标状态可能出现不同步；之后如果提交失败，用户可能同时看到另一条独立的会话/API 错误提示。

当前按可能性排序的假设：

1. 某些浏览器/WebView 与中文输入法组合产生了缺少 `inputType` 的 `beforeinput` 事件；
2. 某个宿主环境、兼容层或外部脚本注入了不完整的事件对象；
3. 用户看到的弹窗来自会话错误、HTTP 错误或 Toast 链路，与该输入事件异常同时发生，但不是由它直接创建。

后续处理边界：

- 输入事件异常作为“前端运行时兼容性”修复项单独处理，不把所有 `window.error` 统一升级为 Modal；
- 在访问 `.includes` 前增加运行时类型保护，对未知 `inputType` 采取保守路径，不猜测成 `insertText`；
- 增加 `inputType: undefined`、中文输入法组合、技能 Token 邻接和 disabled 状态的回归覆盖；
- 错误详情展示可以记录这类客户端异常的错误名、消息、源码位置、浏览器/WebView 版本、语言/输入法和缩放环境，但要避免把原始 DOM 内容或用户输入写入反馈数据。

现有相关测试只覆盖文档模型、粘贴和 IME 结构约束，没有覆盖缺失 `inputType` 的真实事件。当前已执行：

```powershell
bun test ui/src/renderer/components/chat/ComposerSkillTokenInput.test.ts ui/src/renderer/components/chat/ComposerSkillTokenInput.structure.test.ts ui/src/renderer/components/chat/ComposerSkillTokenInput.paste.test.ts
```

结果：18 项通过；该结果不能证明 malformed `beforeinput` 场景已经安全。

## 七、错误后的编辑重发与输入清空

本节先记录实施前的状态机行为和问题定位；实施后的立即清空/安全失败恢复契约见第十三、十四节。

### 1. 当前调用链

错误卡片的“编辑”按钮并不会把错误详情文本写入 Composer。`useErrorEdit` 会找到最近一条右侧用户消息，去除文件标记后通过 `sendbox.edit` 回填这条用户消息：

- `ui/src/renderer/pages/conversation/Messages/components/MessageTips.tsx:80`
- `ui/src/renderer/pages/conversation/Messages/components/MessageTips.tsx:97`

SendBox 收到事件后会：

1. 设置 `editingMsgId` 和目标消息时间；
2. 清除 `replyQuote` 与 Skill chips；
3. 把原用户文本放回 Composer 并聚焦；
4. 在跨树编辑状态 store 中登记 `phase: 'editing'`。

主要位置：`ui/src/renderer/components/chat/SendBox/index.tsx:413`

如果用户说的“引用错误信息”是 Composer 顶部的引用预览，那么这条路径已经调用 `setReplyQuote(null)`，理论上应该被清除。如果指的是对话历史中的原用户消息和 `MessageTips` 错误卡片，它们会在确认重发成功前继续显示，这是当前 reconciliation 时序的结果。

### 2. 为什么点击发送后文本仍然存在

普通发送会调用 `composeAndClear`，在请求前清空 Composer，并在失败时按快照恢复。编辑重发在 `sendMessageHandler` 中使用独立分支：

- 不调用 `composeAndClear`；
- 在后端确认成功前保留编辑文本；
- 只有 `success + revisionUnchanged` 才清空输入并退出编辑态。

主要位置：

- `ui/src/renderer/components/chat/SendBox/index.tsx:1627`
- `ui/src/renderer/components/chat/SendBox/index.tsx:1715`
- `ui/src/renderer/components/chat/SendBox/index.tsx:1768`

纯决策逻辑当前规定：

| 结果 | 当前 Composer 行为 | 当前设计意图 |
| --- | --- | --- |
| `success` | 清空并退出编辑态 | 重发已确认成功 |
| `safe_failure` | 恢复已提交文本，保持编辑态 | 请求未改变会话，可直接重试 |
| `post_mutation_failure` | 不清空文本，退出编辑态 | 会话可能已截断，保留草稿避免数据丢失 |
| 用户在请求期间重新输入 | 保留用户的新输入 | 旧回调不能覆盖新草稿 |

位置：`ui/src/renderer/components/chat/SendBox/editResubmitOutcome.ts:58`

Nomi 的编辑重发在确认到会话已发生变更、但替代回复失败时返回 `post_mutation_failure`。此时会先做消息 reconciliation，再把失败结果交给 SendBox，因此错误卡片和输入文本可能同时保留：

- `ui/src/renderer/pages/conversation/platforms/nomi/NomiSendBox.tsx:966`
- `ui/src/renderer/pages/conversation/platforms/nomi/NomiSendBox.tsx:1023`

截图中的 `USER_LLM_PROVIDER_INVALID_TOOL_SCHEMA` 很可能落入这条路径，但仅凭截图不能确认最终的 `resolution.kind`。后续实现前应记录 operation ID、编辑阶段、`resolution.kind`、输入 revision 和错误卡片 message ID。

### 3. 推荐的编辑重发状态契约

用户期望“点击发送后 Composer 清空”，而系统又需要避免草稿丢失。两者应拆成两个状态，不再用 Composer 当前文本同时承担“已提交内容”和“恢复快照”：

| 状态 | Composer | 错误卡片和历史 |
| --- | --- | --- |
| 编辑中 | 显示可编辑草稿 | 标记“发送后替换本次请求” |
| 提交中 | 立即清空，事务内保存已提交草稿快照 | 显示“正在重新生成”，锁定重复编辑/重试 |
| 安全失败 | 从快照恢复草稿，保持编辑态 | 显示失败原因和重试入口 |
| 会话已变更但生成失败 | 默认保持空输入，提供“恢复草稿” | 保留错误详情和恢复入口 |
| 成功 | 清空快照并退出编辑态 | 清理旧用户消息、旧错误卡片和旧响应尾部 |

恢复快照必须带 operation ID 和输入 revision。用户在请求期间输入的新内容不能被旧的成功/失败回调清空或覆盖。

## 八、渲染崩溃错误是另一类问题

React 渲染异常由 `RouteErrorBoundary` 或更局部的 ErrorBoundary 处理，已经直接展示：

- `error.name`
- `error.message`
- `error.stack`
- React component stack

主要位置：

- `ui/src/renderer/components/layout/RouteErrorBoundary.tsx:66`
- `ui/src/renderer/components/layout/SettingsSiderErrorBoundary.tsx`

这类错误详情已经可选中复制，不应和异步 API/模型错误共用同一套弹窗修复方案。

## 九、反馈上报链路的缺口

以下是实施前的反馈缺口记录，当前安全 `detail` 扩展和展示组件见第十三、十四节。

错误卡片的反馈确认弹窗目前只展示错误码：

- `ui/src/renderer/features/supportChat/SupportChatProvider.tsx:438`

`buildConversationErrorReportMetadata` 会上传 `message`、`code`、`ownership`、重试状态和 resolution，但没有上传 `detail` 字段：

- `ui/src/renderer/features/supportChat/conversationErrorReport.ts:17`

日志压缩包可能仍然包含上下文，但结构化反馈元数据本身不足以让开发快速定位问题。

## 十、已确认与未确认事项

### 已确认

1. 会话错误详情在后端产生、持久化、实时转发和前端转换过程中都存在。
2. 结构化会话错误的技术详情默认收起。
3. 非结构化错误正文默认折叠到 48px。
4. 普通 HTTP 错误的 `details` 被 `BackendHttpError` 保留，但很多调用方没有读取。
5. ACP Toast 会自动消失，当前常见时长为 6 秒或 8 秒。
6. 反馈确认弹窗和反馈元数据没有完整展示/携带 `detail`。
7. 当前 exact `Modal.error/Modal.warning` 调用点较少，主要集中在两个模块。
8. `ComposerSkillTokenInput` 存在对缺失 `inputType` 调用 `.includes` 的运行时风险，该异常没有直接连接到错误弹窗。
9. 编辑重发路径与普通发送路径不同，当前只在确认成功时清空 Composer；`post_mutation_failure` 会保留文本。
10. 错误卡片的编辑事件会清除 Composer 的普通 `replyQuote`，但对话历史中的错误卡片会保留到 reconciliation 完成。

### 尚未确认

1. 用户反馈中的“弹窗”具体是 `Message.error`、`Modal.error`，还是会话中的 `MessageTips`。
2. 用户设备上是否存在额外 CSS、旧构建产物或 WebView 样式差异。
3. 超长 `Modal.error` 内容在目标浏览器/WebView 中是否已经实际发生裁切。
4. 某次具体错误的原始 payload、发生时间和运行环境。
5. 用户反馈中的弹窗是否与 `beforeinput` 异常在同一时间发生，以及触发时使用的浏览器/WebView、输入法和缩放环境。
6. 截图对应的编辑重发最终是 `safe_failure`、`post_mutation_failure` 还是 `success`，以及用户所说的“引用错误信息”是 Composer 引用预览还是对话历史错误卡片。

## 十一、后续修复建议

建议按以下优先级实施，不直接把所有错误都改成全局 modal：

### 前置阶段：修复对话输入运行时异常

- 为 `ComposerSkillTokenInput.handleBeforeInput` 增加 `inputType` 的运行时保护；
- 未知或缺失类型不执行自定义 `preventDefault`，交由后续 `input` 同步逻辑处理或安全忽略；
- 增加 malformed `beforeinput` 的回归测试，至少覆盖 `inputType: undefined`、`isComposing`、中文输入法组合结束和技能 Token 邻接；
- 在 Edge/Chromium、Tauri WebView2 和目标中文输入法环境下验证，记录该异常是否还会出现；
- 将该异常与会话/API 错误分别归类，避免同一个错误既触发全局弹窗又触发会话错误提示。

### 前置阶段：编辑重发状态与草稿语义

- 将“提交后清空 Composer”和“失败后可恢复草稿”拆成两个独立状态；
- 在编辑提交前创建带 operation ID、目标消息 ID、输入 revision 的恢复快照；
- 提交成功后清空 Composer、恢复快照和编辑状态；
- `safe_failure` 恢复快照并保持编辑态；
- `post_mutation_failure` 默认不把旧文本重新塞回 Composer，改为提供明确的“恢复草稿”动作；
- 用户在请求期间输入的新文本优先级高于任何旧操作回调；
- 编辑提交中锁定同一错误卡片的重复编辑和重试，避免两个 destructive operation 竞争；
- 保留 `replyQuote` 清理契约，并增加测试确认错误历史卡片不会被误当成 Composer 引用。

### 设计阶段：用户摘要与开发诊断融合

截图中的主要对象是会话内错误卡片，不是全局 Modal。建议保留错误卡片作为主承载，采用“一张卡片、两层信息”的设计：

1. 用户摘要层始终可见：发生了什么、影响是什么、下一步怎么做；
2. 诊断摘要始终可见：错误码、`incident_id` 和一行已脱敏的技术摘要；
3. 完整技术详情按需展开：完整 `detail`、归属方、阶段、必要的 Provider/模型信息和复制按钮。

设计约束：

- 不把所有错误升级为全局 Modal；长详情优先使用卡片内展开，Modal 只作为查看完整诊断的补充；
- 增加“复制诊断”动作，复制内容使用后端已脱敏字段，不包含原始用户输入、Token 或完整响应体；
- 详情区域支持最大高度、内部滚动、自动换行和窄窗口布局；
- 错误码不只依赖省略号，提供 Tooltip 或复制入口；
- 操作按钮按优先级排列为“重试”“编辑重发”“复制诊断”，反馈入口作为次级动作；
- 编辑中和提交中状态要在错误卡片上可见，并锁定会造成重复请求的操作；
- 当前 `messages.css` 的彩色侧条和渐变背景应改为完整细边框、轻微语义底色和状态图标，避免侧条成为主要视觉焦点；
- 不使用重复嵌套卡片，用户摘要、诊断摘要和详情通过间距、细分隔线和文本层级区分；
- 所有新增动作补齐 hover、focus-visible、loading、disabled、error 状态，并保持中英文和 320px 窄窗口可读。

### 第一阶段：会话错误详情可见

- 结构化错误卡片默认展示 `code + detail + incident_id`；
- 默认折叠时也展示一行诊断摘要和“复制诊断”，避免用户截图完全缺少定位信息；
- 技术详情设置最大高度和内部滚动；
- 增加复制详情按钮；
- 保留用户友好的标题、恢复建议和重试操作；
- 非结构化长错误至少显示完整摘要，并提供展开入口。

### 第二阶段：统一 HTTP/Modal 错误展示

- 新增窄范围的错误详情展示组件或 formatter；
- 优先读取 `BackendHttpError.backendMessage`、`code`、`details`；
- 所有展示内容继续使用已脱敏的数据，不直接展示原始 response body；
- 仅迁移关键 Modal 和关键操作 Toast，避免一次性修改所有 `Message.error` 调用点。

### 第三阶段：反馈信息补齐

- 反馈确认弹窗显示错误码、关联 ID 和技术详情摘要；
- `buildConversationErrorReportMetadata` 增加安全的 `detail` 字段；
- 增加结构化报告测试，确认详情没有在上报边界被丢弃。

### 回归测试

至少覆盖：

- 结构化会话错误默认可见；
- 非结构化长错误的折叠/展开；
- `BackendHttpError.details` 的展示和脱敏；
- Modal 长文本换行、滚动和复制；
- 320px 到桌面宽度、浅色/深色主题；
- Toast 自动关闭、手动关闭和详情入口；
- 反馈元数据包含安全的 `detail` 和 `incident_id`。
- 错误卡片编辑后 Composer 正确回填并清除 `replyQuote`；
- 编辑提交后 Composer 立即清空，安全失败恢复草稿，会话变更后失败提供“恢复草稿”；
- 编辑提交期间用户新输入不被旧操作回调覆盖；
- 编辑成功后旧用户消息、旧错误卡片和旧响应尾部被一致清理；
- 编辑中、提交中、失败恢复和成功状态下的按钮可用性与焦点行为正确；
- 错误码、`incident_id`、技术摘要和完整脱敏详情在中英文、深浅色和窄窗口下均可读。

## 十二、探索验证记录

执行命令：

```powershell
bun test ui/src/renderer/pages/conversation/Messages/components/MessageTips.structure.test.ts ui/src/renderer/pages/conversation/Messages/components/MessageTips.retry.structure.test.ts ui/src/common/chat/chatLib.test.ts ui/src/common/adapter/httpBridge.test.ts
```

结果：70 项通过。

额外执行了针对“技术详情默认展开”的只读红灯检查，结果为失败，输出：

```text
RED: structured technical details are not configured to open by default
```

该检查证明当前代码确实没有默认展开详情，但还不是完整的真实浏览器复现。后续实现前应补充实际 DOM 级测试，并使用一个真实错误 payload 做浏览器验收。

本轮针对编辑重发、错误卡片、会话 reconciliation 和编辑状态契约执行：

```powershell
bun test ui/src/renderer/components/chat/SendBox/editResubmitOutcome.test.ts ui/src/renderer/components/chat/SendBox/editResubmitLifecycle.test.ts ui/src/renderer/pages/conversation/Messages/editResubmitState.test.ts ui/src/renderer/pages/conversation/Messages/conversationReset.test.ts ui/src/renderer/pages/conversation/Messages/components/MessageTips.structure.test.ts ui/src/renderer/pages/conversation/Messages/components/MessageTips.retry.structure.test.ts ui/src/renderer/pages/conversation/Messages/components/MessageText.structure.test.ts
```

结果：55 项通过。现有测试已经明确固定了 `post_mutation_failure` 保留草稿的旧行为，因此后续修复必须同步更新纯决策测试、编辑重发状态测试和真实浏览器交互验收。

## 十三、方案实施记录（2026-08-18）

本轮已在 `fix/error-modal-details` 分支实现计划中的前端闭环，仍未提交；`Cargo.lock` 的既有脏改动保持不变。

### 已实施的行为契约

1. 新增 `ui/src/renderer/utils/ui/errorDiagnostics.ts`，统一处理 Agent 错误、`BackendHttpError` 和未知异常：
   - 摘要优先取安全详情的第一行，没有详情时回退到安全消息；
   - 完整复制/展开文本包含 code、incident ID、归属方、HTTP status、可重试状态、resolution 和脱敏详情；
   - 限制详情长度，清理 Authorization/Bearer、Token、URL query、工作区路径、用户输入字段和 profile 路径；
   - 不读取或序列化原始 response body、JavaScript stack 或 `workspacePath`。
2. `ComposerSkillTokenInput.handleBeforeInput` 先验证运行时 `inputType` 是否为字符串；缺失值直接交给浏览器默认行为，不再对 `undefined` 调用 `includes`。
3. 编辑重发在 operation admission 成功后立即通过 `ComposerSkillTokenInput.clear()` 或同一受控 commit 路径清空文本、Skill chips、临时 DOM 引用和 `replyQuote`。清空后的 revision 作为“用户是否重新输入”的比较基线：
   - safe failure 且没有新输入：恢复提交文本并保持编辑态；
   - safe failure 且有新输入：保留新输入并保持编辑态；
   - success：退出编辑态；
   - post-mutation failure：保持空输入并退出编辑态，错误卡片的“编辑”入口从权威消息重新回填；
   - 陈旧 token 的 success/failure/finally 不修改 Composer 或 loading。
4. 结构化和旧格式对话错误统一进入 `MessageTips` 诊断卡片：摘要常显，技术详情默认折叠；默认头部显示错误码但隐藏关联 ID，incident ID 仍保留在展开详情、复制诊断和反馈元数据中；复制诊断使用共享 `CopyIconButton`；详情使用 `pre-wrap`、内部滚动和 `overflow-wrap:anywhere`。
5. 错误卡片移除 5px 彩色侧条、侧条渐变和背景渐变，改为完整细边框和轻微语义底色；重试时不再同时插入配置修复动作，保持“重试 → 编辑 → 复制诊断 → 反馈”的顺序。
6. 新增 `ErrorDiagnosticContent`，迁移 `KnowledgeTagManagementModal`、`ArtifactPreviewPanel` 和 `SupportChatProvider` 的关键 Modal/反馈确认入口。Modal 内容显示安全摘要和定位元数据，完整详情默认折叠并支持复制/滚动；反馈 `schemaVersion: 1` 保持不变，仅增加可选安全 `error.detail`，并对 message/code/resolution 做安全格式化。
7. 新增开发态 `#/test/error-surface` 和 `scripts/check-error-surface.mjs`，覆盖结构化/旧格式/无 code/无 detail/超长 detail、详情折叠、主题、语言、窄屏、DPR 和 Modal 诊断内容。入口受 `import.meta.env.DEV` 与精确 hash 双重限制。

### 当前自动化验证

已通过：

```text
bun test <本轮 Composer、errorDiagnostics、conversationErrorReport、编辑重发、MessageTips、Modal 结构测试>  → 70 pass / 0 fail
bun run check:i18n   → passed
bun run check:theme  → passed
```

曾尝试执行完整 UI 测试套件；当前已知基线失败为
`ui/src/renderer/pages/conversation/execution/readOnlyConversation.structure.test.ts` 中
“every projected platform chat is read-only” 的源码契约断言（1 fail / 3 pass），涉及
`useConversationResponseMessages(conversation_id)` 的既有只读会话实现，不由本轮错误诊断、Composer
或编辑重发改动引入。受影响测试集合本身仍为 70 pass / 0 fail。

类型检查和 UI 构建已执行，但当前仓库基线仍有与本轮无关的失败：

- `bun run typecheck`：既有 `MessageText` provider 类型、`ProcessTraceItem` animated props、MediaPipe 缺失、视频 Canvas 类型、Arco `Popover showArrow` 等错误；没有本轮新增文件的错误。
- `bun run build:ui`：既有 `@mediapipe/tasks-vision` 无法解析导致 Vite 构建停止。

Edge headless 探针已执行，但当前机器的 Edge `--dump-dom` 在本项目现有 `check:button-layout` 和新增 `check:error-surface` 中均以退出码 0、空 stdout 返回，无法生成 `data-ready="true"` 报告；因此不能把本轮 Edge 矩阵记为通过。脚本会对 Windows Edge 临时 profile 的 `EBUSY` 清理重试，最终无法确认清理时会失败退出，需在能输出 DOM 的 Edge/CI 或目标设备上重新执行：

```powershell
bun run check:error-surface -- --url http://127.0.0.1:5173 --smoke
bun run check:error-surface -- --url http://127.0.0.1:5173 --full
```

Tauri WebView2、中文输入法 malformed `beforeinput`、100%/125%/150%/200% 系统缩放和人工视觉验收仍属于未完成的设备级验收，不由上述单元测试或 Edge 脚本替代。

## 十四、第一轮 Code Review 修复收口（2026-08-18）

本节记录第一轮 code review 后完成的基础收口，仍在
`fix/error-modal-details` 分支，未提交；`Cargo.lock` 及其他无关脏改动保持原状。

### 已完成的 review 修复

1. 诊断 formatter 不再硬编码 `Error code`、`Detail`、`Unknown error` 等用户可见技术标签。
   `getErrorDiagnosticLabels` 从 `zh-CN`/`en-US` i18n 获取标签，展开文本和复制文本共用同一套本地化 formatter。
2. SupportChat 的反馈确认 Modal 已移除旧的 `.conversation-error-report-code` 字体/间距样式，改用
   `ErrorDiagnosticContent` 的独立支持场景类；旧选择器不会再污染新的诊断面板。
3. `ComposerSkillTokenInput` 增加可直接运行的 beforeinput 边界 helper 测试，覆盖缺失 `inputType`、IME 组合和普通插入/删除；
   编辑重发清空动作也抽为生产 helper，测试 token document、受控草稿 fallback、DOM 片段和 reply quote 的清理顺序。
4. 新增真实 React SSR 渲染测试：
   - `ErrorDiagnosticContent.render.test.tsx` 验证摘要常显、详情默认折叠、复制入口、元数据本地化和路径/Token 脱敏；
   - `MessageTips.render.test.tsx` 在真实 `ConversationProvider`、`MessageListProvider`、`MemoryRouter` 下渲染错误卡片，验证重试 → 编辑 → 复制 → 反馈顺序及旧格式回退。
5. `ErrorSurfaceProbe` 不再手工仿制错误卡片，改为挂载真实 `MessageTips` 和消息上下文；详情展开由真实 Arco Collapse DOM 交互驱动后再测量。
6. `check:error-surface --full` 增加默认运行上限，避免 6 个宽度 × 2 个高度 × 4 个 DPR × 2 个语言 × 2 个主题模式 × 6 个主题 × 4 个 fixture × 2 个展开状态 × 2 个 CSS 场景的笛卡尔矩阵在失败重试下失控。
   默认最多执行 128 个 case；任意手工运行也不能超过硬上限 256 个 case，且每个 case 最多重试 2 次，不提供无上限模式。

### Review 后验证记录

已通过：

```text
bun test <ComposerSkillTokenInput、errorDiagnostics、conversationErrorReport、编辑重发、MessageTips、Modal 诊断测试> → 77 pass / 0 fail
node scripts/generate-i18n-types.mjs --check → passed
git diff --check → passed
```

直接执行 `ui` TypeScript 编译后，本轮新增文件和新增测试没有产生新的类型错误；剩余错误仍为基线中的
`MessageText` provider 类型、`ProcessTraceItem` animated props、MediaPipe、视频 Canvas 和 Arco Popover
`showArrow` 等问题。

Edge headless 仍需单独记录：此前本机 Edge `--dump-dom` 会以退出码 0、空 stdout 结束，无法生成
`data-ready="true"` 报告。因此新增真实探针和默认矩阵上限已经落地，但不能把本机 Edge 结果记为通过；
应在能正常输出 DOM 的 Edge/CI 环境重新执行：

```powershell
bun run check:error-surface -- --url http://127.0.0.1:5173 --smoke
bun run check:error-surface -- --url http://127.0.0.1:5173 --full
```

剩余人工验收边界不变：Tauri WebView2、中文输入法 malformed `beforeinput`、浏览器/系统 100%/125%/150%/200%
缩放、浅色/深色内置主题，以及截图中的 `USER_LLM_PROVIDER_INVALID_TOOL_SCHEMA` 真实编辑重发链路仍需在目标设备完成。

## 十六、最终 Code Review Required 修复（2026-08-18）

针对后续 review 发现的 4 项 Required 问题，本轮继续完成以下修复：

1. `buildUnknownErrorDiagnostic` 遇到 `BackendHttpError` 且没有安全的 `backendMessage` 时，改用本地化 fallback 和已脱敏的 `details`，不再把 `Error.message` 中的完整响应体或请求路径带入摘要、Modal、复制内容和反馈诊断。
2. 诊断 formatter 增加整段 `Authorization` 值脱敏，保留认证方案名称但不保留 Basic、Digest 等非 Bearer 凭据；新增响应体回退和非 Bearer Authorization 回归测试。
3. `ErrorSurfaceProbe` 改为挂载真实 Arco `simple Modal`，同时测量 Modal Portal 的详情展开、视口边界和长文本溢出；新增 `config-recovery` fixture 覆盖“修复配置 → 编辑 → 复制 → 反馈”顺序。
4. 探针现在把操作按钮顺序、按钮禁用状态和 Modal 状态纳入 `report.ok`，并新增结构契约测试，避免浏览器 gate 只记录而不判定这些字段。

本轮验证记录：

- formatter、反馈元数据、错误卡片、Composer、编辑重发、主题和探针结构测试通过；
- 直接 UI TypeScript 检查仍只报告既有 MessageText、ProcessTraceItem、MediaPipe、视频 Canvas 和 Arco Popover 基线问题；
- Edge `--dump-dom` 仍需在能输出 DOM 的 Edge/CI 环境重新执行，不能用本机空 stdout 结果替代；
- `Cargo.lock` 继续作为无关脏改动保留，不纳入本轮提交。

## 十五、错误卡片 UI polish（2026-08-18）

根据实际截图对错误卡片和共享 Modal 诊断内容做了第二轮细节打磨，目标是保持“Quiet Kinetic Workspace”的安静层级，同时让主题只改变氛围，不改变状态含义：

1. 默认信息层级收紧：错误卡片和 Modal 头部不再直接展示关联 ID，避免状态标签、错误码和长 ID 挤在同一行；关联 ID 仍由安全 formatter 生成，并在技术详情展开后、复制诊断文本和反馈元数据中保留。
2. 归属方颜色改用现有主题语义变量 `--info`、`--warning`、`--danger` 和 `--color-text-3`，移除错误卡片中按明暗模式写死的蓝灰色；重试标签也改为主题化成功色/中性色。
3. 诊断卡片的边框、底色、摘要区、详情区和 Modal 元数据全部使用现有 Flowy surface/border/focus token；没有新增主题专用色，也没有把第三方 antd 按钮纳入这套规则。
4. 技术详情折叠标题增加 hover、键盘 focus-visible 和 reduced-motion 处理；复制按钮增加主题化 hover、focus、active 状态；错误卡片操作栏允许换行，避免 320px 窄屏横向溢出。
5. 错误图标改为继承卡片语义色，避免 Icon Park 的固定填色覆盖内置主题；共享 Modal 诊断的 code/status 元数据使用可换行的轻量标签，详情仍保持滚动和脱敏。

本轮新增/更新的回归约束包括：默认渲染不出现卡片头部关联 ID、技术详情仍保持渐进披露、主题语义变量存在、详情标题和操作按钮具备 focus/interaction 样式。受影响测试当前为 77 pass / 0 fail；浏览器矩阵和 Tauri WebView2 人工验收仍按第十四节记录的边界单独执行。

## 十七、第二轮 Code Review 全部修复（2026-08-18）

本节记录第二轮 review 对边界问题的收口，仍在 `fix/error-modal-details` 分支；无关的
`Cargo.lock` 脏改动继续保留，不纳入本任务。

### 已完成的安全与一致性修复

1. `scripts/check-error-surface.mjs` 现在限制 URL 只能是无凭据的
   `http(s)://localhost`、`127.0.0.1` 或 `[::1]`，移除 `--no-sandbox`，维护活跃 Edge 子进程集合，
   在正常结束、超时、中断和 profile 清理前统一终止进程树；临时 profile 未确认删除时直接失败，
   不再留下“延迟清理后继续”的成功路径。
2. Edge 矩阵增加 256 case 硬上限、128 case 默认 full 上限和每 case 两次尝试；full 矩阵采用
   代表性 case 加步进采样，避免截取笛卡尔积前 256 项而漏掉语言、DPR、主题或宽度维度。
   `check:error-surface-contract` 作为不启动浏览器的静态门禁加入 `bun run check`，真实 Edge DOM
   和 WebView2 仍是独立验收。
3. `ComposerSkillTokenInput` 对 `insertText` 的 `data` 只接受字符串；缺失、数字或对象等 malformed
   `beforeinput` 直接交给浏览器默认行为，再由后续 `input` 事件同步草稿，不再抛出运行时异常。
4. 编辑/重试重发的终态判断不再允许活动的 `op-b` 被旧的权威 `op-a` 回调提交；安全失败会退休
   已结束 operation，终态墓碑保留最近 256 项，避免既拦截旧回调又无限增长。
5. 诊断 details 改为有深度、节点、字符串和总长度预算的序列化；敏感键、`stack`/`trace`、用户输入、
   带空格的凭据、UNC 路径、`file:` URI、工作区路径和常见绝对路径均在 formatter 层脱敏。新增
   循环对象、空格凭据、用户输入和路径回归测试。
6. 知识库标签颜色和排序失败不再只有 `console.error`，改用统一安全诊断 Modal。排序新增
   `POST /api/knowledge/tags/reorder`，由 SQLite transaction 和内存测试仓储一次交换两个
   `sort_order`，避免两个独立 PUT 造成部分更新；API、hook、Modal 和中英文文案已同步。

### 验证边界

- 已加入 Composer malformed data、operation tombstone、formatter 脱敏/循环对象、标签 Modal
  结构和原子 reorder 服务测试；本轮受影响前端聚焦测试最终 `38 pass / 0 fail`，
  错误 formatter 最后回归为 `8 pass / 0 fail`。
- `node --check`（两个错误面板脚本）、静态 safety contract、i18n、`git diff --check`
  和 `cargo fmt --all -- --check` 已通过；SQLite 原子交换测试和知识服务 reorder 测试各
  `1 pass / 0 fail`，API 类型测试 `1 pass / 0 fail`。
- 本轮不再启动 Edge，也不把本机历史 Edge 进程异常后的人工清理当作自动化通过；需要在专用 Edge/CI
  环境重新执行 `bun run check:error-surface -- --url http://127.0.0.1:5173 --smoke`，再单独完成
  Tauri WebView2、中文输入法、系统缩放和目标主题的人工验收。
- `bun run check` 和 `bun run build:ui` 在当前受限环境中会在 Bun workspace 子脚本阶段报
  `Operation not permitted`；直接执行 UI TypeScript 检查时只看到既有 `CourseWorkspace` 按钮契约、
  MediaPipe、Arco Popover、视频 Canvas 和 provider 类型错误，直接启动 Vite 构建时另遇 Bun
  `node_modules` 二进制 remap 错误，均未归因于本轮修改。
