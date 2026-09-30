# Web 前端开发与 React 规范 (React & Frontend Guidelines)

> **定位**：`web/` 目录下 Web UI、React 组件与 Hooks 的统一开发规范与质量红线。  
> **核心原则**：**“Hook 优先复用 ahooks，工具库统一 es-toolkit，useEffect 仅作外部同步逃生舱并强制四段式中文注释。”**

---

## 1. Hooks 与工具库选型约定

### 1.1 Hook 复用与准入规则
- **优先复用 `ahooks`**：涉及状态管理、副作用封装、事件监听、网络请求、节流防抖、生命周期等场景时，**必须优先评估并使用 `ahooks` 现成提供的成熟 Hook**（如 `useRequest`、`useEventListener`、`useDebounceFn`、`useThrottleFn`、`useMount`、`useUnmount` 等）。
- **自定义 Hook 严格受限**：仅当 `ahooks` 无法满足特定业务场景或缺乏对应抽象时，方可封装自定义 Hook；新增自定义 Hook 必须在顶部 JSDoc 注释中明确说明**“未采用 ahooks 的原因及业务专有性”**。
- **一致性要求**：同类能力必须保持单一实现路径，严禁同一项目内混用多套同类工具 Hook 方案。

### 1.2 工具库选型与导入规范
- **首选 `es-toolkit`**：需要使用数据处理、对象操作、集合遍历等辅助函数时，**统一使用 `es-toolkit`** 对应能力。
- **严禁新增 `lodash`**：坚决避免新增 `lodash` 或 `lodash-es` 的依赖和引用。
- **严格按需导入**：工具函数与第三方组件库一律采用精准命名导入（Named Import），禁止整库通配导入（`import * as ...`），确保包体积与 Tree-shaking 友好。

---

## 2. useEffect 使用铁律与逃生舱规范

React 官方强调 `useEffect` 应当作为**与外部系统同步的逃生舱（Escape Hatch）**，而非组件数据流的中转站。

### 2.1 定位与准入原则
`useEffect` 仅允许用于以下需要与 **React 外部系统**进行同步的场景：
1. **网络与 WebSocket 通信**（建立长连接、订阅消息、发起与请求销毁）；
2. **IPC / 浏览器原生 API**（事件监听、全局键盘绑定、Window 尺寸/全屏监听）；
3. **第三方非 React 库实例同步**（如 CodeMirror、Canvas、Xterm、图表实例挂载与更新）；
4. **真实物理 DOM 交互**（测量元素尺寸 `scrollHeight`、选区控制、光标聚焦）。

### 2.2 绝对禁止的使用场景与正反例

| 禁止场景 | 错误做法 (❌ 反例) | 正确替代方案 (✅ 推荐) |
|---|---|---|
| **纯计算/渲染派生** | 用 effect 监听 `firstName` 和 `lastName`，在 effect 中 `setFullName(...)` | 直接在组件渲染期计算：`const fullName = firstName + ' ' + lastName;` |
| **昂贵计算缓存** | 用 effect 监听数组变化，在 state 中缓存排序过滤后的列表 | 使用 `useMemo(() => list.filter(...).sort(...), [list])` |
| **用户交互逻辑** | 用户点击按钮后改变 state，再通过 effect 捕获该 state 触发提交或通知弹窗 | 直接在点击事件处理函数（`onClick` handler）中执行提交与弹窗 |
| **重置子组件状态** | 用 effect 监听 `userId` 变化并重置表单 state | 给子组件传递 `key={userId}`，由 React 自动销毁重建全新实例 |
| **级联状态同步** | state A 变化触发 effect 1 改 state B，state B 变化触发 effect 2 改 state C | 归并为一个单一的事件处理函数，或使用 `useReducer` 集中管理状态迁移 |

### 2.3 清理要求 (Cleanup) 与配对原则
- **严格配对 Cleanup**：涉及异步请求、事件订阅、DOM 监听器、定时器（`setTimeout`/`setInterval`）的 effect，**必须在返回函数中提供清理逻辑**（如 `abortController.abort()`、`removeEventListener`、`clearTimeout`、取消订阅）。
- **依赖项显式真实**：依赖项必须与 effect 内部引用的外部变量（props、state、外部函数）严格一致，杜绝故意省略依赖或使用无意义的空依赖 `[]` 导致陈旧闭包（Stale Closure）。

### 2.4 异步调用的竞态防范 (Race Condition) 与取消机制
在 `useEffect` 中发起异步请求时，若组件快速重绘、依赖频繁变更或组件突然卸载，迟到的请求结果会导致“旧数据覆盖新数据”或在已卸载组件上触发非法更新。

**规范要求**：在执行异步调用时，必须配备 `cancelled` 标志位或标准 `AbortController` 机制：

```tsx
// 必须采用的防竞态与取消防御范式（参考 web/src/components/catalog/ImportPanel.tsx）
useEffect(() => {
  let cancelled = false;

  const loadData = async () => {
    try {
      const result = await client.listImports();
      if (!cancelled) {
        setImports(result);
      }
    } catch (caught) {
      if (!cancelled) {
        setError(formatError(caught));
      }
    }
  };

  void loadData();

  return () => {
    cancelled = true; // 依赖更新或组件卸载时立即作废本次异步结果
  };
}, [client]);
```

### 2.5 依赖项引用稳定性与防死循环 (Infinite Render Loops)
在 `useEffect` 依赖数组中传入引用不稳定的变量，会导致组件每次渲染都触发 Effect，进而引发**无限重渲染死循环（Infinite Render Loop）**：

1. **函数依赖项**：
   - 依赖的组件内函数必须使用 `useCallback` 包裹，或使用 `ahooks` 的 `useMemoizedFn` 生成持久稳定函数引用；
   - 组件外声明的无状态纯函数可直接提取到组件外部。
2. **复杂对象/数组依赖项**：
   - 严禁在 deps 中传入行内对象字面量（`[ { id } ]`）或行内数组；
   - 复杂计算对象必须使用 `useMemo` 缓存引用。
3. **Zustand 状态订阅原则**：
   - 严禁在组件中写 `const { a, b } = useAppStore(s => ({ a: s.a, b: s.b }))`，每次执行都会创建全新对象导致依赖项永远变化；
   - 正确做法：拆分为细粒度的 Primitive Selector：`const a = useAppStore(s => s.a)`，或使用 `useShallow`。

### 2.6 React 18 / 19 严格模式 (StrictMode) 幂等性要求
React 在开发模式下会对组件执行“挂载 → 立即卸载 → 再次挂载”（Double Mount）以探测副作用残留。

**规范要求**：
- `useEffect` 内部建立的副作用与返回的清理函数必须是**严格幂等（Idempotent）**的；
- 建立 WebSocket、订阅消息或绑定原生 DOM 监听器时，卸载钩子必须将其彻底销毁，确保第二次挂载时系统状态与初次挂载完全一致，绝不允许留下孤立连接或双重监听。

### 2.7 严禁级联状态派发反模式 (Effect Cascading Anti-Pattern)
- **现象**：在 Effect 1 中监听 Store A 的变化去 `setStoreB`，在 Effect 2 中监听 Store B 去调用 `setStateC`；
- **危害**：引发多次额外渲染帧（Layout Thrashing）、界面跳动闪烁，且极端情况下会导致循环触发死锁；
- **解决原则**：状态变更应遵循**“事件驱动（Event-Driven）”**——由具体的外部事件（用户点击、WebSocket 推送消息到达、路由切换）在发生源头直接调用状态迁移方法，严禁通过 Effect 充当状态管道。

---

## 3. 强制四段式中文注释规范

为了防止 `useEffect` 滥用并提升工程可审计性，**每一个 `useEffect` 语句上方必须添加规范的四段式中文注释**（参考 `web/src/components/dialogs/McpSettingsSection.tsx`）：

### 3.1 注释模板
```ts
// useEffect必要性：<说明同步的具体外部系统，如 WebSocket / 宿主文件 / 原生Window事件 / DOM选区>
// 目的：<说明同步的具体业务行为>
// 未采用 ahooks：<说明为何现有 useRequest/useMount/useEventListener 等无法满足语义>
// 替代方案不适用性：<说明为何无法在渲染阶段计算，或为何无法由事件回调直接处理>
useEffect(() => {
  // ...
  return () => {
    // cleanup
  };
}, [/* 显式完整依赖 */]);
```

### 3.2 生产代码实机示范
```ts
// useEffect必要性：宿主文件声明（经 WebSocket 的 config/get）是 React 之外的外部系统；
// 目的：进入本设置分区时读取一次声明文件的校验结果，并在客户端重连后重新读取；
// 未采用 ahooks：请求与状态由 store/settingsConfig 持有，useRequest 会造成状态分裂；
// 替代方案不适用性：配置读取属于异步远端拉取，无法在渲染阶段派生，且进入分区属于路由展示行为无直接按钮点击事件。
useEffect(() => {
  void load(client);
}, [client, load]);
```
