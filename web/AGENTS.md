# Web UI / SDK Agent 行为准则与操作规约

> **定位**：用于指导 LLM 与研发人员在 `web/`（Web UI、SDK 与应用协议包）目录下的开发行为与发布流。  
> **权衡**：这些准则偏向谨慎而非速度。对于琐碎任务，请结合项目实际自行判断。

---

## 1. 编码前先思考

**不要假设。不要隐藏困惑。主动暴露取舍。**

在动手实现之前：
- **说明假设**：明确说明你的假设。若不确定，先询问。
- **列出分支**：若存在多种理解方式，把它们都列出来——不要在沉默中私自选择。
- **推崇极简**：若存在更简单的方案，请主动指出。在合理时提出异议。
- **及时停顿**：若有不清楚之处，立即停下来，指明困惑所在并询问。

---

## 2. 简单优先

**用最少的代码解决问题。不做任何投机性代码。**

- **严守范围**：不实现需求之外的功能。
- **拒绝过度抽象**：不为仅使用一次的代码做抽象。
- **杜绝空想可配置**：不添加未被要求的“灵活性”或“可配置性”。
- **避免过度防御**：不为不可能发生的场景做错误处理。
- **精炼度复查**：若写了 200 行却本可 50 行完成，请坚决重写。

> 问自己：“一位资深工程师会说这过度复杂了吗？” 如果是，请简化。

---

## 3. 精准改动

**只动必须改动的部分。只清理自己制造的混乱。**

编辑已有代码时：
- **不搭车重构**：不要“顺手改进”相邻无关的代码、注释或格式。
- **尊重现状**：不要重构并未损坏的部分；遵循现有风格，即便你会用不同写法。
- **对待死代码**：若发现无关的既有死代码，提一下但不要擅自删除。

处理自己代码的衍生影响：
- **主动清零残留**：移除因你的本次改动而变得无用的导入、变量与函数。
- **检验标准**：每一处代码 Diff 都应能直接追溯到用户的具体请求。

---

## 4. 目标驱动执行

**先定义成功标准。循环执行直到验证通过。**

将任务转化为可验证的目标：
- “添加校验” → “先为非法输入写测试，再使其通过”
- “修复该 bug” → “先写一个能复现它的测试，再使其通过”
- “重构 X” → “确保重构前后既有测试均通过”

对于多步骤任务，先陈述简要计划：
1. `[步骤 1]` → 验证：`[检查点]`
2. `[步骤 2]` → 验证：`[检查点]`
3. `[步骤 3]` → 验证：`[检查点]`

### 前端开发三大红线 (React Guidelines)

涉及 Web 前端代码编写时，必须严格遵守以下红线（详见完整规范文档 [web/docs/react-conventions.zh.md](docs/react-conventions.zh.md)）：

1. **Hooks 优先复用 `ahooks`**：状态管理、副作用、事件监听、请求等场景优先使用 `ahooks`；仅当无法满足时方可自定义 Hook 并须注释说明原因。
2. **工具库首选 `es-toolkit`**：避免引入 `lodash` / `lodash-es`；一律按需导入，确保 Tree-shaking 友好。
3. **`useEffect` 仅作外部同步逃生舱**：禁止用于纯计算与内部 state 派生；严格防竞态（AbortController / cancelled 标志）；必须提供配对 cleanup；**每一个 `useEffect` 前必须强制添加中文注释**：
   ```ts
   // useEffect必要性：<外部系统>；目的：<内容>；未采用 ahooks：<理由>
   ```

---

## 5. 协议指纹与跨仓同步

**动 App Server wire 协议面 = 换指纹 + 两仓一起改。**

- **指纹定义**：服务端 `PROTOCOL_VERSION` 与客户端 `APP_SERVER_PROTOCOL_VERSION`。两者在握手时做**严格相等校验**。
- **指纹形态**：**`fp-<n>`** 单调递增计数器（每次严格递增，不得复用历史值，非日期、非发布版本）。
- **触发条件（增量也算）**：方法增删改名、现有 DTO 字段增减、事件 Payload 变化、新增服务端主动通知。
- **详尽手册与落点矩阵**：参见专门文档 [web/docs/protocol-fingerprint-sync.zh.md](docs/protocol-fingerprint-sync.zh.md)。

### 标准执行 5 步走清单

1. **更新常量与全仓扫尾**：
   - 更新服务端与客户端真源常量为最新值（如 `fp-13`）；
   - 用旧指纹值在全仓 grep，清理 `mock-server.ts`、`smoke.ts`、`readiness.test.ts` 等夹具中的残留。
2. **执行机械指纹门禁**：
   - 运行 **`bun run check:fingerprint`**（按标识符检验全仓 7 文件 10 处 + 站点 2 处落点，必须全绿通过）。
3. **同步基线技术文档**：
   - 更新 [05-flowy-agent-store-app-server-protocol.md](../docs/agent-store/05-flowy-agent-store-app-server-protocol.md) 头部指纹与相关章节；
   - 更新 [docs/agent-store/README.md](../docs/agent-store/README.md) 本轮发布与状态记录。
4. **跨仓同步独立文档站点 (`agent-store-site`)**：
   - 检出同级 `C:\workspace\agent-store-site`；
   - 同步 `content/docs/{zh-CN,en-US}/typescript-sdk.md` 的指纹示例与方法数；
   - 运行 **`bun run check:release-sync`** 确保方法计数与版本号跨仓严格一致。
5. **全量测试与发版准出**：
   - 运行后端测试：`cargo test -p nomifun-app-server`；
   - 运行前端与 SDK 测试：`cd web && bun run typecheck && bun run test`；
   - 运行端到端冒烟测试：`bun scripts/smoke.ts`；
   - 若需正式发版，严格遵循 [docs/agent-store/25-release-runbook.zh.md](../docs/agent-store/25-release-runbook.zh.md) 执行 npm 发包与站点上线检查（`bun run release:check`）。