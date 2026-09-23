# 会话阅读位置保持技术方案与调研设计

文档日期：2026-09-23  
状态：已实现 / 已合并基线验证  

---

## 一、需求背景

在多会话（Multi-Session）并发与长对话场景下，用户经常遇到以下交互痛点：
1. **多任务切换时阅读位置丢失**：用户在会话 A 中向上翻阅长代码块或历史决策，此时切换至会话 B 查看进展；当再次切回会话 A 时，列表强制滚到最底部，导致用户需要重新手动向上滑动寻找刚才阅读的位置，打断心流体验。
2. **流式输出（Streaming Output）与历史阅读的冲突**：当后台模型持续吐字输出时，消息列表高度不断膨胀。如果用户正在查看上方历史，频繁盲目置底会将视口拉扯回底部；如果用户在置底状态，则需要平滑自动跟随最新生成内容。
3. **虚拟列表渲染抖动与白屏**：引入虚拟滚动（如 `react-virtuoso`）后，如果切回会话时采用异步恢复或分步恢复，容易产生“先置底、再闪烁跳到历史位置”或“历史视口空白未挂载”的视觉瑕疵。

### 核心目标
- **会话独立记忆**：为每个会话维护独立的阅读滚动状态快照。
- **无感精准恢复**：切换回会话时，在首帧绘制（paint）前同步恢复滚动位置，虚拟列表瞬间装载对应分片，杜绝抖动与白屏。
- **智能意图仲裁**：
  - 若离开前在查阅历史：切回后保持历史位置锁定，若有新消息生成则在右下角提供“回到底部”悬浮按钮及新内容提示；
  - 若离开前处于置底：切回后继续自动跟随底部；
  - 若用户在当前会话主动发送新消息：无条件重置历史锁定，平滑置底跟随。

---

## 二、业界方案调研与对比

在技术方案设计前，对业界主流对话产品（ChatGPT、Claude Web、Slack、VSCode Chat、Discord）及虚拟滚动技术规范（React Virtuoso、DOM Scroll Anchoring）进行了深度调研：

| 维度 | ChatGPT / Claude Web | Slack / Discord | 本方案 (Flowy / Allo) |
| :--- | :--- | :--- | :--- |
| **脱离跟随判定** | 用户发生**向上滚动（`delta < 0`）**且超过阈值脱离置底；仅内容向下增长不判定为脱离 | 滚动条距离底部 > 阈值（如 10~20px）且有向上手势 | **双因子判定**：`delta < -2px`（向上滚动）且 `bottomGap > 12px`，有效屏蔽流式输出向下撑高引发的误脱离 |
| **会话切换恢复** | 内存 Map 缓存每条 thread 视口 offset，切换时恢复 | IndexedDB / Redux 记录 channel last_read_id | **轻量内存 LRU 注册表**（容量 200 会话），基于 `scrollTop` + `userScrolled` 标记进行同步恢复 |
| **恢复时序与防抖** | 同步渲染 / CSS 锚定 | 锚定到最近已读 message_id | **`useLayoutEffect` 绘制前同步定位**：配合 `react-virtuoso` 外部滚动容器（`customScrollParent`），浏览器首帧即渲染对应数据分片 |
| **用户主动发消息** | 强制取消历史锁定并平滑置底 | 强制置底 | **检测到新发送的 User Message ID 变更**时，主动重置阅读锁定并双帧 `scrollToBottom` |
| **内存与销毁安全** | 限制缓存会话数量 | 定期清理非活动 channel 状态 | **严格 LRU 淘汰机制**（上限 200 条），防止无上限会话导致内存泄漏 |

---

## 三、系统架构与核心设计

系统由三大核心模块协同组成：

```mermaid
flowchart TD
    subgraph UI ["视图层 (MessageList)"]
        ML[MessageList 组件]
        Virtuoso[react-virtuoso 虚拟列表]
        Spacer[message-list-end-spacer 锚点]
    end

    subgraph Hook ["控制层 (useAutoScroll)"]
        UAS[useAutoScroll Hook]
        SwitchDetect[会话切换判定 useLayoutEffect]
        IntentJudge[用户交互与滚动方向判定]
        FollowPin[流式输出同步钉底]
    end

    subgraph Store ["存储层 (sessionScrollRegistry)"]
        SSR[SessionScrollRegistry 单例]
        LRUCache[Map LRU 缓存 (Max: 200)]
    end

    ML -->|传入 conversationId, list| UAS
    UAS -->|挂载 customScrollParent| Virtuoso
    UAS -->|同步读取/保存快照| SSR
    SSR -->|LRU 存储| LRUCache
    IntentJudge -->|向上滑动| UAS
    SwitchDetect -->|恢复历史视口 / 置底| UAS
    FollowPin -->|钉住尾部锚点| Spacer
```

### 1. 滚动快照模型与 LRU 注册表 (`sessionScrollRegistry.ts`)
```ts
export interface SessionScrollSnapshot {
  scrollTop: number;           // 垂直滚动像素偏移
  userScrolled: boolean;        // 用户是否主动离开底部处于阅读历史状态
  updatedAt: number;            // 记录时间戳
}
```
- **LRU 淘汰保证**：上限固定为 200 个会话。每次 `get` 或 `save` 时通过 Map key 重插入刷新新鲜度；超限时安全淘汰最久未访问的快照。
- **边界防御**：输入参数过滤 `null`/`undefined`，`scrollTop` 自动收敛到 `>= 0`。
- **快照仅依赖像素偏移**：早期设计中的 `anchorMessageId` 备用锚点字段从未被消费，已从模型中移除；如需跨高度估算漂移的锚点级恢复，见 §六 演进建议。

### 2. 自动滚动与精准恢复控制 (`useAutoScroll.ts`)
- **生命周期时序收拢**：
  - 将会话切换检测从 `useEffect` 移至 `useLayoutEffect`。当 `conversationId !== previousConversationIdRef.current` 时：
    1. 同步将离开会话的快照写入 `sessionScrollRegistry`——偏移取自 `handleScroll` 实时跟踪的 `lastScrollTopRef`，**不读取 DOM**（切换瞬间 `scrollerEl` 可能已随骨架屏卸载、或已属于新会话，读 DOM 会得到 0 或错误会话的位置）；且仅当旧会话实际展示过（`initialScrollDoneRef`）才写回，避免 A→B→C 快速跳变时把 A 的位置记到从未展示的 B 头上；
    2. 重置 `initialScrollDoneRef`、清理防抖定时器，并置位 `swapBaselinePendingRef`；
    3. **列表归属门控**：`loadedConversationId !== conversationId` 时跳过恢复（会话 id 翻转比异步拉取换列表早一个 commit，对旧会话 DOM 恢复会被高度钳制吞掉偏移，钳制产生的 scroll 事件还会把垃圾位置写进新会话快照）；
    4. 门控通过后先播种 `previousLastUserIdRef = findLastUserMessageId(messages)` 并消费 `swapBaselinePendingRef`，再按快照恢复——这样 A→B 的列表交换与后台 stale 会话的流式更新都不会被发消息检测误判为“新发送的用户消息”而把恢复好的视口拉回底部；
    5. 若当前会话历史状态为 `saved.userScrolled === true`，同步赋值 `scrollerEl.scrollTop = saved.scrollTop` 并激活历史锁定；
    6. 若当前会话为置底状态，同步重置 `showScrollButton` 与 `hasNewContentBelow` 为 `false`，并调度平滑置底。
- **写入节流**：scroll 事件逐帧触发（用户滚动 + 流式跟随钉底），注册表写入做 200ms 防抖；切换与卸载路径则从 `lastScrollTopRef` 同步保存，保证最终位置不丢失。
- **流式输出防抖动**：
  - 依赖 `.message-list-end-spacer` 计算 sub-pixel 偏差并实时校准，关闭 Virtuoso 内部的 `followOutput`，避免思考过程折叠/工具调用展开时导致的视口跳变。
- **用户发消息置底优先级**：
  - 监听最新的 User Message ID。只要产生新的一轮提问，即刻解除锁定并置底；`swapBaselinePendingRef` 置位期间（会话交换未完成）一律跳过该判定。

### 3. 组件挂载接入 (`MessageList.tsx`)
- 从 `ConversationContext` 获取当前 `conversation_id` 并传入 `useAutoScroll`。
- 结合已有上拉翻页（`onLoadOlder`）和 `prependAnchorRef`，保证旧消息向前追加与会话恢复逻辑互不干扰。

---

## 四、关键优化事项与边界处理

### 1. 解决 React 异步生命周期导致的切会话失效
- **问题**：`useEffect` 在浏览器 paint 之后异步执行，而 `useLayoutEffect` 在 paint 之前同步执行。当组件保持挂载仅 `conversationId` 改变时，`useLayoutEffect` 会先于 `useEffect` 运行，读取到过期的 `initialScrollDoneRef.current === true`，导致恢复逻辑被跳过。
- **优化**：在 `useLayoutEffect` 中统一收拢会话切换与旧会话暂存判定，全流程在绘制前同步完成，杜绝首屏跳变。

### 2. 切换至置底会话的按钮残留清理
- **问题**：从查看历史的会话 A（悬浮按钮显示中）切入置底的会话 B 时，异步 `scrollToBottom` 可能会导致悬浮按钮残留闪烁 1 帧。
- **优化**：在 `useLayoutEffect` 进入置底分支时，立即同步调用 `setShowScrollButton(false)` 和 `setHasNewContentBelow(false)`。

### 3. HiDPI / 亚像素与向上滑动方向识别
- **问题**：在 Windows 高分屏缩放（125%、150%）下，`scrollHeight - clientHeight - scrollTop` 常产生 1~3px 的亚像素舍入误差；且流式吐字时高度不断增加，向下滚动容易被误判为“用户手动滚动”。
- **优化**：
  - 设定 `FOLLOW_BOTTOM_THRESHOLD_PX = 12` 吸收亚像素误差；
  - 严格限制 `delta < -2`（只有用户主动向上滚动）才触发 `userScrolledRef.current = true`，向下增长与内容变高绝对不会中断自动跟随。

### 4. 显式胶囊气泡 Tips 视觉升级 (Pill Badge)
- **设计升级**：
  - **常规翻看历史**：保持 40×40 紧凑圆形毛玻璃箭头按钮；
  - **下方有新内容/新回复生成**：自动平滑横向展开为胶囊徽标态（`height: 38px, padding: 0 14px 0 10px`），显式显示 `↓ 查看最新内容` 文案与主题色高亮边框/文字，直观呈现未读新内容。
  - **主题与动效**：支持深浅主题无缝切换（毛玻璃 `backdrop-filter` + 微色调背景混合），配备平滑指数曲线过渡。

### 5. 切换/卸载保存不读取 DOM（`lastScrollTopRef` 同步保存）
- **问题**：
  - 卸载保存位于被动 effect cleanup，执行时滚动元素已经 detach，`scrollerEl.scrollTop` 读回 0——若直接保存会把上一会话快照污染成 `{scrollTop: 0}`；
  - 会话切换的 commit 中，`scrollerEl` 可能已随骨架屏/空态 early-return 卸载，或 ref 回调已指向新会话元素，读 DOM 得到的同样是 0 或错误会话的偏移。
- **优化**：`handleScroll` 持续把最新偏移跟踪进 `lastScrollTopRef`；切换保存与卸载保存统一从该 ref 读取，完全不触碰 DOM。配套守卫：
  - 仅当 `initialScrollDoneRef.current === true`（旧会话真实展示过）才写回，快速 A→B→C 跳变中从未展示的会话不会被写入他人位置；
  - 切换与卸载时清理 200ms 防抖定时器，避免卸载后定时器触发把脏位置写回。

### 6. 恢复必须等待列表归属确认（`loadedConversationId` 门控）
- **问题**：会话 id 翻转比 store 异步拉取并替换列表早一个 commit。若在 id 已翻转、列表仍是旧会话的 commit 中执行恢复，会对着旧会话 DOM 设置偏移：被高度钳制吞掉恢复值，钳制引发的 scroll 事件又会把旧会话末尾位置写进新会话快照，造成双向污染。
- **优化**：`useMessageLstCache.loadMessages` 在 `mergeIntoList` 同一批次内调用 `setLoadedId(key)`，经新增的 `MessageListLoadedIdProvider` 下发；恢复分支仅在 `loadedConversationId === conversationId` 后才执行，并在执行时播种发消息基线（`previousLastUserIdRef`）+ 消费 `swapBaselinePendingRef`。无 `conversationId` 的消费方保持旧有非门控行为（`if (conversationId && loadedConversationId !== conversationId) return;`）。

---

## 五、验证与测试矩阵

本次改动通过了完整的质量门禁与自动化测试矩阵：

### 1. 自动化单元与结构测试
- **测试命令**：
  ```bash
  bun test src/renderer/pages/conversation/Messages/sessionScrollRegistry.test.ts \
           src/renderer/pages/conversation/Messages/useAutoScroll.test.ts \
           src/renderer/pages/conversation/Messages/useAutoScroll.structure.test.ts \
           src/renderer/pages/conversation/Messages/MessageList.scrollButton.structure.test.ts
  ```
- **测试结果**：**34 项测试全部通过，163 个断言（expect calls）100% 达标**。
  - `sessionScrollRegistry.test.ts` (7/7 pass): 覆盖增删查改、清空、负数保护、LRU 200 容量上限淘汰与命中刷新、`conversation.deleted` 联动清理。
  - `useAutoScroll.test.ts` (20/20 pass): 覆盖会话切换同步保存、历史视口精确恢复、发消息强制置底、悬浮按钮状态即时清理；新增结构性断言锁定时序防护（`lastScrollTopRef` 无 DOM 保存、`loadedConversationId` 门控、`swapBaselinePendingRef` 交换抑制、`SCROLL_SAVE_DEBOUNCE_MS` 防抖）。
  - `useAutoScroll.structure.test.ts` (2/2 pass): 覆盖用户交互与折叠面板布局变动防护。
  - `MessageList.scrollButton.structure.test.ts` (5/5 pass): 覆盖流式跟随与视口锚点结构。

### 2. 仓库代码门禁（Quality Gates）
- **主题契约检查**：`bun run check:theme`（6 个主题配置全部验证通过）
- **国际化类型检查**：`bun run check:i18n`（10954 个 i18n 键值全部验证通过）

---

## 六、后续演进与扩展建议

1. **会话物理删除联动（已完成）**：已监听全局 `conversation.deleted` 事件，在会话删除时同步调用 `sessionScrollRegistry.clear(conversationId)`。
2. **跨重启持久化（可选）**：当前快照保存在内存中，生命周期与应用运行时对齐。如未来需要“应用重启/刷新页面也能精确恢复阅读位置”，可将 `sessionScrollRegistry` 的底层存储通过 IndexedDB / localStorage 进行轻量持久化。
3. **锚点级恢复（可选演进）**：像素级 `scrollTop` 恢复在 Virtuoso 对未挂载区域使用估算高度时，极端长会话（数千条消息）下可能存在轻微漂移。如需更高精度，可重新引入基于消息 ID 的锚点模型（记录顶部可见消息 ID + 内偏移，恢复时 `scrollToIndex` 到锚点再补偿像素），代价是恢复路径依赖 Virtuoso API 且需要处理锚点消息已被删除的降级。当前像素方案在实测会话规模下精度足够。
4. **待人工验证项**：超长 Virtuoso 会话（1000+ 消息）中的恢复精度、与上拉翻页 `prependAnchorRef` 的叠加表现建议在手测环境走查一遍；自动化矩阵覆盖的是时序结构与注册表行为，不含真实虚拟列表滚动测量。
