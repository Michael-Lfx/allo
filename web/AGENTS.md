用于减少常见 LLM 编码失误的行为准则。
请结合项目特定的说明一并参考。

**权衡：** 这些准则偏向谨慎而非速度。对于琐碎任务，请自行判断。

## 1. 编码前先思考

**不要假设。不要隐藏困惑。主动暴露取舍。**

在动手实现之前：
- 明确说明你的假设。若不确定，先询问。
- 若存在多种理解方式，把它们都列出来——不要在沉默中私自选择。
- 若存在更简单的方案，请说出来。在合理时提出异议。
- 若有不清楚之处，停下来。指出困惑所在，并询问。

## 2. 简单优先

**用最少的代码解决问题。不做任何投机性代码。**

- 不实现需求之外的功能。
- 不为仅使用一次的代码抽象。
- 不添加未被要求的"灵活性"或"可配置性"。
- 不为不可能发生的场景做错误处理。
- 若你写了 200 行却本可 50 行完成，请重写。

问自己："一位资深工程师会说这过度复杂了吗？"
如果是，请简化。

## 3. 精准改动

**只动必须改动的部分。只清理自己制造的混乱。**

编辑已有代码时：
- 不要"顺手改进"相邻代码、注释或格式。
- 不要重构并未损坏的部分。
- 遵循现有风格，即便你会用不同写法。
- 若发现无关的死代码，提一下——但不要删除它。

当你的改动产生孤立代码时：
- 移除因你的改动而变得无用的导入/变量/函数。
- 除非被要求，否则不要移除原本就存在的死代码。

检验标准：每一处改动都应能直接追溯到用户的请求。

## 4. 目标驱动执行

**先定义成功标准。循环执行直到验证通过。**

将任务转化为可验证的目标：
- "添加校验" → "先为非法输入写测试，再使其通过"
- "修复该 bug" → "先写一个能复现它的测试，再使其通过"
- "重构 X" → "确保重构前后测试均通过"

对于多步骤任务，先陈述简要计划：
1. [步骤] → 验证：[检查点]
2. [步骤] → 验证：[检查点]
3. [步骤] → 验证：[检查点]

清晰的成功标准让你能独立循环推进。
模糊的标准（"让它跑起来"）则需要不断澄清。

### 前端开发约定（Hooks 与工具库）

- **Hook 复用优先级**：涉及状态管理、副作用封装、事件监听、请求、节流防抖等场景时，优先评估并使用 `ahooks` 提供的现成 Hook。
- **自定义 Hook 约束**：仅当 `ahooks` 无法满足业务需求时，才新增自定义 Hook；新增时需在描述中说明“未采用 ahooks 的原因”。
- **工具库约束**：需要使用 `lodash` 能力时，优先使用 `es-toolkit` 对应能力；避免新增 `lodash` / `lodash-es` 依赖与新引用。
- **导入规范**：优先按需导入，避免整库导入，确保包体积与 tree-shaking 友好。
- **一致性要求**：同类能力保持统一实现路径，避免同一项目同时混用多套同类工具函数方案。

### 前端开发约定（useEffect 使用说明）

- **定位原则**：`useEffect` 仅作为 React 逃生舱使用，用于与 React 外部系统同步（如：网络请求、IPC/浏览器 API、订阅与事件监听、第三方库实例同步）。
- **优先级原则**：在确有副作用需求时，优先评估并使用 `ahooks` 的现成能力（如 `useRequest`、`useEventListener`、`useDebounceFn`、`useThrottleFn`、`useMount`、`useUnmount` 等）；仅在 `ahooks` 无法覆盖或不适配当前场景时，才回退到原生 `useEffect`。
- **禁止场景**：禁止用 `useEffect` 做纯渲染派生（props/state 计算）、链式状态推导、仅为同步本地镜像 state 的更新。
- **替代方案**：纯计算放在渲染阶段；昂贵计算使用 `useMemo`；用户交互逻辑放到事件处理函数；可通过 `key` 重置的状态优先使用 `key` 方案。
- **清理要求**：涉及异步请求、订阅、监听器、定时器时，必须提供 cleanup（如取消请求、取消订阅、移除监听、清除定时器）。
- **依赖要求**：依赖数组必须与 effect 内实际引用保持一致，避免缺失依赖或无效依赖导致重复执行或陈旧闭包问题。
- **注释强制**：每个 `useEffect` 前必须添加中文注释，说明“为何必须使用 effect、同步的外部系统是什么、为何不能使用 `ahooks` 替代（如适用）、不使用 effect 的替代方案为何不适用”。
- **注释模板**：`// useEffect必要性：<外部系统>；目的：<同步内容>`。

## 5. 协议指纹与跨仓同步

**动 App Server wire 面 = 换指纹 + 两仓一起改。** 指纹是 `APP_SERVER_PROTOCOL_VERSION`（`web/packages/protocol`）与 `PROTOCOL_VERSION`（`nomifun-app-server`）：握手与 SDK 对它做**严格相等**校验，取值只需「与上一次不同」。**现行形状是 `fp-<n>`（从 `fp-1` 起）的计数器**——每次 bump 递增，**不得复用任何历史值**。它**不是版本号、不是发布日期、也不是变更日期**：`fp-1` 之前用的是日期戳，日期戳同样只是标签（连续改动每次加一天，所以常超前于日历），换成计数器就是为了一次性去掉这层误读。

触发分支（**增量也算**）：方法增删改名、现有 DTO 加字段、事件 payload 变化、新增通知。

1. 先改两个常量，再用**旧值全仓 grep** 收尾——落点比「三个权威位置」更广，`web/scripts/mock-server.ts`、`web/scripts/smoke.ts`、`web/packages/sdk/src/readiness.test.ts` 这类夹具最容易漏（历次落点与偏差登记见 `docs/agent-store/16` §7 决策 4）。
   **这步现在有机械门禁**：`bun run check:fingerprint`（`scripts/check-protocol-fingerprint.mjs`，已进 `bun run check`）按**标识符**比对全部落点——不按形状扫，因为仓库里另有 MCP 协议版本 `2025-11-25`、`published_at` 夹具与故意的 `2000-01-01`，形状相近但语义无关：本仓 7 个文件 10 处 + 站点 2 处（站点不在时跳过并提示）。**任一落点不一致会失败；某个抽取模式一处都匹配不到也会失败**——后者是刻意的：模式失配意味着门禁其实什么都没查，那比没有门禁更糟。新增落点 = 往脚本的 `MIRRORS` 表（站点在 `SITE_MIRRORS`）加一行，形状常量是脚本里的 `FP_SHAPE`。
2. 正文同步：`docs/agent-store/05-flowy-agent-store-app-server-protocol.md`（头部指纹 + 对应章节）与 `docs/agent-store/README.md` 的本轮记录。
3. **跨仓改独立仓 `C:\workspace\agent-store-site`**：`content/docs/{zh-CN,en-US}/typescript-sdk.md` 的 §2 常量示例随指纹改（中英各一处），方法计数、`ServerNotification` 枚举与 `changelog` §4 的未发布台账按本次改动同步；两语言结构必须一致。**其中两项现在也有机械门禁**：方法计数与版本锁步由 `bun run check:release-sync`（`scripts/check-agent-store-release-sync.mjs`，已进 `bun run check`）两边比对——计数真源是本仓 `web/packages/client/src/http-transport.test.ts` 的 `DOCUMENTED_ROUTE_SPLIT`，版本权威是 `web/packages/protocol/package.json`（站点 `content/release.json` 必须同值）。
4. 完成标准：旧值在本仓代码里归零（只剩历史散文），且 `bun run check:fingerprint` 绿（第 1 步那条门禁的机械形式，已进 `bun run check`）；`cargo test -p nomifun-app-server` 与 `cd web && bun run typecheck && bun run test` 绿；站点仓 `bun run check:docs-sync` 报 `0 drift`、`bun run test:docs-sync` 通过。
5. **发版**（不是每次改 wire 都要发版）：完整有序清单见 `docs/agent-store/25-release-runbook.zh.md`，它同时管 npm 四包与站点仓（GitHub Release + 站点上线）。一条命令的门禁是本仓 `bun run release:check` + 站点半边 `bun run release:check:site`（需要同级 `agent-store-site` checkout，或用 `AGENT_STORE_SITE_DIR` 自行跑站点那条）。