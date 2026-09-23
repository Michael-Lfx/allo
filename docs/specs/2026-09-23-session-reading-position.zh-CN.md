# 会话阅读位置保持技术方案与调研设计

文档日期：2026-09-23  
状态：已实现 / 已完成 Impeccable 主题与时序重构验证  

---

## 一、需求背景

在多会话（Multi-Session）并发与长对话场景下，用户经常遇到以下交互痛点：
1. **多任务切换时阅读位置丢失**：用户在会话 A 中向上翻阅长代码块或历史决策，此时切换至会话 B 查看进展；当再次切回会话 A 时，列表强制滚到最底部，导致用户需要重新手动向上滑动寻找刚才阅读的位置，打断心流体验。
2. **流式输出（Streaming Output）与历史阅读的冲突**：当后台模型持续吐字输出时，消息列表高度不断膨胀。如果用户正在查看上方历史，频繁盲目置底会将视口拉扯回底部；如果用户在置底状态，则需要平滑自动跟随最新生成内容。
3. **虚拟列表渲染抖动与白屏**：引入虚拟滚动（如 `react-virtuoso`）后，如果切回会话时采用异步恢复或分步恢复，容易产生“先置底、再闪烁跳到历史位置”或“历史视口空白未挂载”的视觉瑕疵。

### 核心目标
- **会话独立记忆**：为每个会话维护独立的阅读滚动状态快照。
- **无感精准恢复**：切换回会话时，在首帧绘制（paint）前同步恢复滚动位置，虚拟列表瞬间装载对应分片，杜绝抖动与白屏。
- **智能意图仲裁与极简视觉**：
  - **正常历史翻看（`isProcessing === false`）**：展示 36×36 极简圆形 `[↓]`，安静不打扰，点击平滑置底；
  - **对话生成输出中（`isProcessing === true`）**：若用户向上翻看历史，动态平滑展开为微胶囊态 `[↓ ● 查看最新内容]`，带柔和主题色呼吸点；
  - **在最底部阅读**：按钮完全隐藏；
  - **用户发送新消息**：无条件重置历史锁定，平滑置底跟随。

---

## 二、业界方案调研与对比

在技术方案设计前，对业界主流对话产品（ChatGPT、Claude Web、Slack、VSCode Chat、Discord）及虚拟滚动技术规范（React Virtuoso、DOM Scroll Anchoring）进行了深度调研：

| 维度 | ChatGPT / Claude Web | Slack / Discord | 本方案 (Flowy / Allo) |
| :--- | :--- | :--- | :--- |
| **脱离跟随判定** | 用户发生**向上滚动（`delta < 0`）**且超过阈值脱离置底；仅内容向下增长不判定为脱离 | 滚动条距离底部 > 阈值（如 10~20px）且有向上手势 | **双因子判定**：`delta < -2px`（向上滚动）且 `bottomGap > 12px`，有效屏蔽流式输出向下撑高引发的误脱离 |
| **会话切换恢复** | 内存 Map 缓存每条 thread 视口 offset，切换时恢复 | IndexedDB / Redux 记录 channel last_read_id | **轻量内存 LRU 注册表**（容量 200 会话），基于 `scrollTop` + `userScrolled` 标记进行同步恢复 |
| **恢复时序与防抖** | 同步渲染 / CSS 锚定 | 锚定到最近已读 message_id | **`useLayoutEffect` + 恢复锁（`isRestoringScrollRef`）**：同步定位与 Virtuoso 双帧测量确认，阻断高度计算中间态误清空 |
| **用户主动发消息** | 强制取消历史锁定并平滑置底 | 强制置底 | **检测到新发送的 User Message ID 变更**时，主动重置阅读锁定并双帧 `scrollToBottom` |
| **内存与销毁安全** | 限制缓存会话数量 | 定期清理非活动 channel 状态 | **严格 LRU 淘汰机制**（上限 200 条）+ 监听 `conversation.deleted` 事件联动销毁 |

---

## 三、系统架构与核心设计

系统由三大核心模块协同组成：

```mermaid
flowchart TD
    subgraph UI ["视图层 (MessageList)"]
        ML[MessageList 组件]
        Virtuoso[react-virtuoso 虚拟列表]
        Spacer[message-list-end-spacer 锚点]
        ScrollBtn[36px 圆形 / 34px 胶囊徽标]
    end

    subgraph Hook ["控制层 (useAutoScroll)"]
        UAS[useAutoScroll Hook]
        SwitchDetect[会话切换判定 useLayoutEffect]
        RestoreLock[isRestoringScrollRef 恢复锁]
        IntentJudge[用户交互与滚动方向判定]
        FollowPin[流式输出同步钉底]
    end

    subgraph Store ["存储层 (sessionScrollRegistry)"]
        SSR[SessionScrollRegistry 单例]
        LRUCache[Map LRU 缓存 (Max: 200)]
    end

    ML -->|传入 conversationId, list, isProcessing| UAS
    UAS -->|挂载 customScrollParent| Virtuoso
    UAS -->|同步读取/保存快照| SSR
    SSR -->|LRU 存储| LRUCache
    IntentJudge -->|向上滑动| UAS
    SwitchDetect -->|恢复历史视口 / 置底| UAS
    RestoreLock -->|阻断中间态误重置| IntentJudge
    FollowPin -->|钉住尾部锚点| Spacer
    UAS -->|showScrollButton, hasNewContentBelow| ScrollBtn
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
- **会话删除联动**：监听全局 `conversation.deleted` 事件，在删除会话时同步释放快照。
- **边界防御**：输入参数过滤 `null`/`undefined`，`scrollTop` 自动收敛到 `>= 0`。

### 2. 自动滚动与精准恢复控制 (`useAutoScroll.ts`)
- **恢复锁（Restoration Lock）与 Virtuoso 时序对齐**：
  - 针对虚拟列表高度尚未就绪时 DOM clamp 导致的误置底问题，引入 `isRestoringScrollRef`。
  - 在恢复期间，阻断 `handleScroll` 写入中间脏数据、阻断 `updateBottomState` 误清空 `userScrolledRef`、阻断 `followContentGrowth` 强制拉到底部。
  - 结合 `virtuosoRef.scrollTo` 与 rAF 双帧校验，确保真实高度计算就绪后锁定目标位置。
- **状态机分流（`isProcessing` 感知）**：
  - 仅在 `isProcessing === true` 且离开底部时激活 `hasNewContentBelow = true`；
  - 正常浏览历史时 `hasNewContentBelow = false`，保证按钮为纯净小巧的 36×36 圆形 `[↓]`。
- **生命周期时序收拢与全写点门控**：
  - 会话切换在 `useLayoutEffect` 中同步完成，从 `lastScrollTopRef` 无 DOM 安全读取旧会话快照；
  - 四个写点（`handleScroll`、`pauseAutoFollow`、`scrollToBottom`、`hideScrollButton`）全面加入 `ownsVisibleList` 归属门控。

### 3. Impeccable 主题与视觉规范 (`messages.css`)
- **彻底剔除硬编码色值**：完全基于项目标准 CSS 变量（`--bg-elevated`、`--border-base`、`--color-primary`、`--color-text-1`、`--color-text-2`）。
- **深浅模式自适应**：
  - 亮色下展现为通透纯净白/浅灰浮岛（`box-shadow: 0 2px 6px rgba(0,0,0,0.04), 0 6px 18px rgba(0,0,0,0.08)`）；
  - 暗色下（包含 `rhythm-dark` 酒红暗黑、冷灰、落日等全部 6 种预设）浑然一体，自适应当前主题底色与主色。
- **无障碍 A11y 规范**：全量子元素完整支持 `prefers-reduced-motion: reduce`。

---

## 四、验证与测试矩阵

- **自动化测试**：`bun test src/renderer/pages/conversation/Messages` 全量通过（**521 pass, 0 fail, 2102 expect() calls**）。
- **主题契约检查**：`bun run check:theme` 6 大预设主题全量通过。
- **国际化类型检查**：`bun run check:i18n` 10954 个多语言键值全量通过。
