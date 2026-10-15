# 40 · ZCode 桌面 × allo：集成验收记录

> 本文是 **38 号方案**（ZCode 桌面集成方案）与 **39 号文档**（V4 行模型映射）的落地验收记录：
> 记录「桌面应用能不能用、验到了什么、哪些是实测哪些是推断、边界在哪」。
> 结论先行，过程与证据在后。

## 1. 结论

**ZCode 官方桌面（zai-org/ZCode v3.15.1，Electron）已经能把 allo App Server 当作 agent 后端跑起来，
走完整条链路并渲染出真实模型回复。** 端到端证据（本机实测，非推断）：

- **开发态（源树 `pnpm dev:runtime`）**：完整跑通一轮对话（用户消息 → 助手回复），见下方实测表与截图。
- **打包态（`dist/win-unpacked`）**：应用能起窗口、agent 后端确实是我们门面（帧转储 17 帧全部通过官方严格 schema、
  侧栏列出 allo 的真实会话、模型选择器里能看到探针 provider 并可选中）。**安装包（NSIS）那步尚未过**，见 §5.4。

```
用户在 ZCode 桌面输入 → v4/command createSession（带 firstInput）
  → 门面在 allo 建会话、Ack 回 sessionId
  → 宿主订阅 → 门面下发 snapshot 帧 → 兑现挂起的首发（sendText）
  → allo 起轮 → 门面把 allo 事件投影成 V4 行/增量帧
  → 宿主渲染：用户消息 / "已工作 N 秒" / 助手回复
```

实测一轮的真实结果（截图见 `assets/40-zcode-desktop-e2e.png`）：

| 项 | 值 |
| --- | --- |
| 用户输入 | 请用一句话介绍你自己，不超过30个字。 |
| 助手回复 | 我是阿洛，你的桌面工作助手，帮你处理文件、数据和日常任务。 |
| 模型 | `Local Mimo Bridge (probe)/mimo-v2.5-flash`（allo 侧落到 `mimo` provider） |
| 帧序列（会话 `01a13334-…`） | `snapshot, row.appended:turnHeader, row.appended:userInput, state.updated, row.appended:reasoning, row.delta×4, row.appended:assistantText, row.delta×6, row.upserted:reasoning, row.upserted:assistantText, row.upserted:turnHeader, state.updated` |
| schema 复检 | 该轮 26 个 v4 帧全部通过官方严格装配 schema（客户端 assembler 口径） |
| 门面单测 | `node --test test/` 48/48 |
| schema 机器校验 | `npm run test:schema` 58/58 |

## 2. 交付物

### 2.1 协议门面（探针目录 `C:\workspace\zcode-bridge-probe`）

| 面 | 内容 |
| --- | --- |
| 传输 | NDJSON/stdio，`{method, params}` 请求 → `{id, result}` 响应 + 通知帧 |
| 会话面（legacy） | `session/create`、`session/send`、`session/subscribe` 等（38 号方案阶段 1/2 交付） |
| 会话面（V4，官方宿主实际在用） | `v4/conversation/subscribe`、`v4/conversation/resync`、`v4/conversation/unsubscribe`、`v4/command`、`v4/commands/query`，通知 `v4/conversation/frame`（wire 信封 `{wireVersion:3, kind:'complete', deliveryKind, logicalFrameId, logicalFrameOrdinal, topic, subscriptionId, frame}`） |
| 命令面 | `createSession`（新会话/草稿预热的真实创建路径，带 `firstInput`）、`sendText`、`deleteSession`、`renameSession`、`switchModelConfig`、`stop`／`cancelBackgroundWork` → 落到 allo 真语义；官方枚举内其余类型 → `noop`（如实说明"无对应语义、未执行"）；枚举外 → `rejected` |
| 启动握手 | `startup/storageState` 通知（桌面 `requireStorageStartup` 的等待条件） |
| 宿主 housekeeping | `provider/updateAccountConfig`、`workspace/updateInteractionPreferences`、`workspace/updateModelIoPreferences`（只接收 + 记账，不投射到 allo） |
| 冷恢复 | 会话身份直接沿用 allo `conversation_id`；`conversation/get` 现场登记未在册会话 |

### 2.2 ZCode 侧本地补丁（未提交上游，`git checkout` 可还原）

| 文件 | 改动 | 为什么必须 |
| --- | --- | --- |
| `packages/services/src/zcode-agent/zcodeAgentProcessManager.ts` | env 分支读 `ZCODE_AGENT_SERVER_STORAGE_ENTRY`，据此声明 `supportsStorageStartup` + `storagePreparationEntry` | 桌面强制要求 agent 命令声明存储启动契约，否则 `unsupported_runtime`：窗口起得来但工作区永远打不开。官方 E2E 是**删除** env 绕过；本机选择补声明（存储准备交给官方 `zcode.cjs`，协议面仍由门面承担） |
| `packages/desktop/scripts/dev.mjs` | 透传 `ZCODE_DEV_ELECTRON_ARGS`（附加 Electron 开关） | 无人值守时用 CDP 驱动 renderer 做端到端验证（`app.isPackaged=false` 时端口固定 9229，无需本补丁；本补丁用于额外开关） |
| `packages/zcode-server-cli/tsup.config.ts` | external 补 `yazl` + `yauzl` | `zcode serve` 构建产物报 `Dynamic require of "fs" is not supported`（上游 `packages/server/tsup.config.ts:39,46` 同坑已修，server-cli 漏改） |

### 2.3 校验与运维工具

| 工具 | 用途 |
| --- | --- |
| `tools/schema-conformance.mjs`（`npm run test:schema`） | 用官方 `@zcode/shared` 的 zod（strict）逐个 parse 门面产出的帧形状 |
| `tools/validate-frame-dump.mjs` | 用**实跑帧转储**（`ALLO_BRIDGE_FRAME_DUMP=<path>`）复检客户端 assembler 口径的严格 schema——合成样例查不出内容级失败 |
| `tools/start-zcode-desktop.mjs` | 一条命令：起 Electron 桌面 + 把 agent 后端指到门面 + 注入存储契约 + 开 CDP |
| `tools/cdp-eval.mjs` / `tools/cdp-screenshot.mjs` | 无人值守驱动与取证（锁屏也能用：作用在页面上下文/页面截图） |

## 3. 本轮真机抓出的坑（都已修，且都补了回归）

按发现顺序，每条都保留了"症状 → 根因 → 修法"：

| # | 症状 | 根因 | 修法 |
| --- | --- | --- | --- |
| 1 | 桌面报 `unsupported_runtime`，工作区打不开 | 宿主强制要求 agent 命令声明存储启动契约，env 覆盖分支故意不声明 | 补 `ZCODE_AGENT_SERVER_STORAGE_ENTRY` 声明（2.2） |
| 2 | `SQLite startup failed: startup_status_timeout` | 命令声明了 `supportsStorageStartup` → 客户端等 agent 上报 `startup/storageState`，门面不发 | 门面启动即上报 `checking → ready`（`lib/storage-startup.mjs`） |
| 3 | 发送失败：`command 类型未实现：createSession` | 桌面 UI 的 composer 走 `v4/command` 的 `createSession`（不是 legacy `session/create`） | 实现命令分派器（§2.1 命令面） |
| 4 | `provider_not_found: no [providers.local-mimo-bridge] entry in ~/.agent-store/config.toml` | 桌面 provider 命名空间（ZCode 配置）与 allo 命名空间（`config.toml` 的 `[providers.*]`）是两套；门面把桌面 id 直接透传 | 加 provider 映射层（`ALLO_MODEL_MAP` 可显式映射），映射不了就**如实降级**用 allo 默认模型并记账；allo 拒绝模型时再退一次（`createAlloConversation`） |
| 5 | 界面 `fault.subscription.recoveryFrameTimedOut` | resync/恢复路径只回 Ack 从不发帧，客户端等"恢复帧"直到超时 | 订阅与 resync 一律下发 snapshot 帧（承诺快照就要发帧） |
| 6 | 界面 `fault.subscription.contentRejected` | `control.activeWorks[0]` 缺必填 `startedAt`（`activeWorkSummarySchema`），整帧被 zod 拒 | 补 `startedAt`；并把这个字段的回归补进 schema 校验（原来的脚本停在 completed 相位，`activeWorks` 恒空，正好绕过了这条约束） |
| 7 | 点开历史会话报"会话不存在" | 侧栏会话来自 allo，门面进程内没有登记；订阅时 `sessions.require` 直接抛 | `ensureSession`：按 allo `conversation/get` 现场登记（冷恢复） |
| 8 | 改了 `~/.agent-store/config.toml` 的模型，桌面模型列表**不动** | 桌面列表读的是它自己的 `~/.zcode/v2/provider_config.json`，与 allo 注册表**没有任何联动**（之前那份是我静态写的探针规则） | 新增 `tools/sync-zcode-models.mjs`：把 config.toml 的 `[providers.*]`/`[models.*]` 投影成桌面规则（provider id 直接用 allo 的名字，选择就能 1:1 透传）；**启动器每次启动自动同步** |
| 9 | 模型选择器点了不生效；日志里 `subscribeConversationV4 FAIL: not_found: Conversation … not found` → 渲染层 `conversation store connect failed` | 桌面会记住 workspace 的上次会话 id，而那个 allo 会话早已被回收（草稿被删）；`ensureSession` 只处理报错，mock 侧回 null 也没兜 | `ensureSession` 把"报错"和"回 null"都当"不存在"，就地重建 allo 会话并保住桌面那个 id（自愈）；订阅调用本身失败时再重建重试一次；自愈**如实记 stderr** |
| 10 | 日志刷 `unsubscribeConversationV4 FAIL: Unrecognized key "unsubscribed"` | 官方 unsubscribe 的结果是 strict 空对象（`zcodeProtocolEmptyResultSchema`），我多回了一个 `unsubscribed` 字段 | 回空对象（事实记 stderr）；并把这类"无对应语义面"的响应形状抽到单一来源 `lib/v4-surfaces.mjs`，纳入 schema 机器校验 |
| 11 | agent 回答的工作目录不是桌面选的项目目录（桌面选 `…\Documents\cowork`，agent 报 alloc 的 `app-server-workspaces\<会话id>`） | 桌面的 `v4/command createSession` 带 `workspaceId`（用户选的项目目录），门面**完全没理它**，`conversation/create` 也没带 `workspace` → allo 自建目录 | 门面把该路径经 `workspace/create` 登记成 allo workspace（带缓存），并把 `workspace.id` 填进 `conversation/create`；登记失败不阻断建会话但如实记 stderr。真机断言：会话 `workspace_id` 读回 == 该目录登记出的 id（`tools/probe-workspace-wiring.mjs`） |
| 12 | 侧栏不断冒出 "allo session" 噪音条目（几十秒里新增十几行任务） | sessions-index 把 allo 侧**所有**会话都推给桌面，桌面会给每条物化一行任务元数据——包括桌面从没打开过的空会话（草稿预热、建了没用） | sessions-index 只推**有实际消息**的会话（`conversation/messages` 首页非空，`page_size=1`）；负面结果只缓存 10s，本门面发过消息的会话直接标记为"有消息"**立即可见**；查询失败按"有消息"处理并如实记账（宁可多一条，不丢真实会话）。实测：allo 106 条会话里 57 条为空，索引只下发 49 条 |
| 13 | **打开历史会话是空白面板**（侧栏有它、点进去没内容） | 门面进程随会话起，只投影实时事件；而订阅的第一帧是快照 —— 快照只含本进程投影出的行，**从不回放 allo 历史** | 订阅/ resync 前先 `conversation/messages` 回放历史：把历史消息合成同一套 allo 事件流喂给行模型（**复用** `applyEvent`，不手抄第二套行形状）。真机：28 条消息 → 28 行（turnHeader/userInput/assistantText/reasoning/toolCall），用户消息 6/6、助手文本 7/7 对上，帧过官方严格 schema |
| 14 | 上面 12 已挡住新增，但**已物化的空会话行仍显示** | 官方源码注释：`session.removed` 只移除实时 detail，**持久行删除仍由 tasks-index row/tombstone 决定**；侧栏是「本地任务行 ∪ 后端索引」的并集 | 新增 `tools/probe-sidebar-noise.mjs`（只读体检）+ `tools/clean-sidebar-noise.mjs`（预演/`--apply` 打 `deleted=1` 墓碑，可逆）。真机：54 条噪音行（6 条 allo 已不存在 + 48 条无消息）清掉后复测 **0 条** |
| 15 | 侧栏分组不对：`cowork` 项目里混进一堆不属于它的会话 | 门面对**任何** workspace topic 都推全量会话 → 每条会话归到哪个分组只取决于"当时桌面开着哪个工作区" | 按会话自己的 `workspace_id` → `canonical_path`（`workspace/list` 缓存 60s）精确分发：匹配到桌面工作区 → 只进那个分组；匹配不到（allo 早期自建的 `app-server-workspaces/<id>`）→ 落默认工作区（侧栏「任务」）。真机：cowork 分组只剩 1 条真正属于它的会话，跨 topic 重复 0（`tools/probe-index-buckets.mjs`） |
| 16 | 修 15 时暴露：同一条会话会同时留在两个分组 | 分桶依赖"**已订阅**的工作区集合"；先订阅 default 时 cowork 的会话看起来是未归类 → 落 default，cowork 随后订阅到位它又进了 cowork | 新增 sessions-index 订阅时，**重算其它订阅的快照**（快照是权威状态、客户端整块替换）；增量侧补 `session.removed`（本工作区不再包含的会话显式移除） |
| 17 | allo 里空会话越堆越多（116 条里 57+ 条是空的） | 桌面的归档/删除**只改本地任务库、不下发 allo**（日志实测 `deleteSession`/`conversation/delete` 零次）；allo 侧没有"归档"概念，只有硬删除 | 新增 `tools/clean-allo-empty-sessions.mjs`（默认预演；`--apply` 才真删；只删**消息数 0** 的会话，删除前二次确认；`status=running` 的被 allo 拒绝并如实报告）。真机：一次清掉 63 条，复测 allo 侧 `总数 55 / 有消息 55 / 空 0` |
| 18 | 索引侧分桶改对了，侧栏分组**还是老的** | 侧栏分组读的是本地任务行的 `workspace_key`；早期物化写死的归属不会自己变（主键是 `(workspace_key, task_id)`，改归属还会撞重复行） | 新增 `tools/reassign-task-workspaces.mjs`：按同一条规则重算本地行的归属（撞到同 task 已有目标行时，把旧行走廊打墓碑）。真机：cowork 分组只剩 1 条真正属于它的会话 |
| 19 | 需求：把桌面「归档」映射到 allo 删除（allo 没有归档） | ①归档只写本地、不通知后端 → 无法实时同步；②**归档可逆、allo 删除不可逆**；③同一会话在本地可能有多行（跨工作区），一条归档一条可见 | 新增 `tools/sync-archived-to-allo.mjs`，接入启动器每次启动跑一次。**口径（用户决定）：不做备份，直接删**（`--force --apply`，含"仍有可见行"的重复行）；等 allo 有归档功能再换成语义对齐的映射。默认（不带 `--force`）保留护栏：仅删"纯隐藏"的会话。真机：`--force` 一次删除 49 条 |
| 20 | 空 allo 会话越堆越多（实测堆到 10 条）——桌面上完全看不到 | 桌面每开一次「新建任务」/每次启动都会 `createSession` 一条**无首发**的草稿会话；门面早期**无条件新建** allo 会话 | 草稿语义改成"最多一条"：`createSession`（无首发）先 `acquireEmptyDraft()` 复用已存在的空会话，没有才新建；同一次扫描里清掉**任何工作区**里超过 10 分钟没说话的空草稿（阈值避免误删其他窗格刚预热的）。草稿一旦收到输入或被删除即移出草稿集合。真机：一次扫描把空会话 10 → 4、`probe-draft-reuse` 断言两次草稿返回同一 sessionId |
| 21 | **思考等级没有正确映射**（注册表声明 `support_efforts = ["low","high"]`，桌面只显示「关闭/开启」） | 四处各自缺一环：① 桌面档位控件读模型注册表的 `optionSpecs.reasoningLevel.values`，而同步工具只投影了 provider/模型清单，没投影 `support_efforts` → 控件退化成 关/开；② v4 没有独立思考档位命令，档位随 `switchModelConfig` 的 `thought` 字段一起来，而门面只转发 `model`、把 `thought` 丢掉；③ 门面快照的 `settings.thoughtLevel.available` / v4 `config.thoughtLevels` 恒为空数组、`enabled` 恒 `true`；④（顺带挖出）v4 `switchModelConfig` 载荷是 `{provider, model, thought}`，而映射函数只认 `providerId/provider_id` → 换模型一直以 `incomplete_selection` 被拒 | 新增 `lib/thought-levels.mjs`（词表 / 读声明 / 规范化）+ 回归 `test/thought-levels.test.mjs`：① 同步工具把 `support_efforts` 投影成 `optionSpecs.reasoningLevel`（按 ZCode codec 必填补齐 `properties`/`maxOutputTokens`，codec 校验通过）；② `switchModelConfig` / `createSession.config.thought` / `session/setThoughtLevel` / `session/resume` 统一翻译成 allo 的 `reasoning_effort`（低/中/高/超高直通；开/关落成"不指定"并记账；**怪值拒绝**，不伪造成功）；③ 快照与 v4 行投影报出**声明过的**档位；④ 修 `{provider, model}` 载荷解析；⑤ 真机 allo 回的是 provider **注册 UUID** 而非配置键 → 加身份反查（由门面自己的请求学习 + 唯一模型名兜底，有歧义就不猜）。真机：`probe-thought-level` 断言 allo 侧 `reasoning_effort=high` 读得回来、怪值被拒且不写入；打包版 UI 档位控件从「开启」变为「高」 |

**另外顺手补齐的三个只读面**（原来是 `Method not implemented`，日志噪音 + 面板降级）：`workspace/readPresentation`、
`v4/conversation/plans`、`v4/conversation/workflowRuns` —— 都按 allo 的**真实能力**回：
工作区引用 + 模式 + **空**斜杠命令表、**空**计划集合、**空**运行列表。空集合是合法结果，
不是编造内容（allo 没有命令表面、没有 plan 引擎、没有工作流投影）。

**方法论教训（值得沿用）**：第 6 条是"合成样例全过、真机被拒"的典型。合成样例来自同一套工厂函数但喂假数据，
schema 校验只能证明"形状工厂没写错"，证明不了"真实内容合法"。因此新增
`ALLO_BRIDGE_FRAME_DUMP` + `validate-frame-dump.mjs`：**先落盘实跑帧，再用官方 schema 复检**。

## 4. 已知边界（不遮掩）

| 边界 | 现状 | 影响 |
| --- | --- | --- |
| 交互式审批 | 决策 A：桌面会话**不做**交互审批；工具权限由 allo 引擎静态策略决定 | 需要人工确认的工具调用在桌面无法确认；桥只对非终态工具事件发 stderr 告警 |
| `workspace/readPresentation` | 已实现，但只提供 allo 有的东西 | 回工作区引用 + `mode: build` + **空**斜杠命令表（allo 无命令表面）；文件树的富信息仍缺，面板按空表渲染 |
| `v4/conversation/plans`、`v4/conversation/workflowRuns` | 已实现，如实回**空**集合 | 计划面板 / 工作流摘要显示"没有"，不再刷 `Method not implemented` |
| 未实现的编辑器类命令 | `applyFileRewind`/`editUserQuery`/`forkAssistant`/`retryTurn`/队列微调/目标类 → `noop` | 桌面这些入口点了没有效果（如实回"无对应语义"，不假装成功） |
| 模型切换 | provider id 与 allo 同名时可 1:1 透传（`sync-zcode-models.mjs` 生成的就是这种）；否则 `rejected` 带原因 | 桌面模型选择器显示的模型 = 实际下发给 allo 的模型（同名透传时一致） |
| 文本分片 | 超 1MB 的帧**不发**并记日志（分片路径未实现） | 极长工具输出/超长上下文可能丢帧 |
| 已建会话的工作目录 | 修 workspace 接线**之前**建的 allo 会话仍挂在 allo 自建目录上（`extra.workspace_id` 是建会话时写死的，事后改不了） | 这些老会话的 agent cwd 仍是 `app-server-workspaces\<会话id>`；**新建会话**才会用桌面选的项目目录 |
| 打包形态 | 见 §5 | — |

## 5. 验收与复现

### 5.1 前置

1. allo App Server 在跑：`C:\workspace\allo\target\release\agent-store.exe`（监听 `127.0.0.1:8787`，WS 路径 `/api/app-server/ws`）。
2. ZCode 官方仓已 `pnpm install`（全量，桌面打包链需要子工作区链接）且已构建过 `@zcode/desktop`。

### 5.2 开发态启动（本机已实测）

```bash
node C:/workspace/zcode-bridge-probe/tools/start-zcode-desktop.mjs
```

该命令会：起 `pnpm dev:runtime`（tsup + vite + electron），并把
`ZCODE_AGENT_SERVER_COMMAND=node`、`ZCODE_AGENT_SERVER_ARGS_JSON=["<门面>","--backend","real","--allo-endpoint","ws://127.0.0.1:8787/api/app-server/ws"]`、
`ZCODE_AGENT_SERVER_STORAGE_ENTRY=<官方 zcode.cjs>` 注入；CDP 端口 9229（ZCode 主进程自己 `appendSwitch`）。

### 5.3 自动化验收

```bash
# 门面单测
node --test C:/workspace/zcode-bridge-probe/test/

# 帧形状的 schema 机器校验（官方仓 schema）
cd C:/workspace/zcode-bridge-probe && npm run test:schema

# 实跑帧复检（先设 ALLO_BRIDGE_FRAME_DUMP，跑一轮后）
node C:/workspace/.schema-tools/node_modules/tsx/dist/cli.mjs \
  C:/workspace/zcode-bridge-probe/tools/validate-frame-dump.mjs \
  C:/Users/15165/AppData/Local/hermes/cache/scratch/zcode-frame-dump.jsonl
```

### 5.4 打包（Windows）

```bash
cd C:/workspace/ZCode/packages/desktop
export PATH="/c/workspace/zcode-bridge-probe/.bin:$PATH"   # 该目录有 pnpm.cmd 垫片，见下

# ① 资源准备（必做，且必须与目标平台一致）：暂存 agent bundle 与 koffi 原生模块
ZCODE_TARGET_OS=win32 ZCODE_TARGET_ARCH=x64 pnpm prepare:agent-bundle

# ② electron-builder 出包（win-unpacked + NSIS 安装包）
ELECTRON_MIRROR=https://npmmirror.com/mirrors/electron/ pnpm bundle -- --os win --arch x64 --skip-prepare --skip-build
```

三个实测坑：

| 坑 | 现象 | 处理 |
| --- | --- | --- |
| `pnpm.cmd` 不在 PATH | 构建脚本报 `'pnpm.cmd' is not recognized`（mise 只提供 `pnpm.exe`） | 在 PATH 前置一个 `pnpm.cmd` 垫片（内容：`@echo off` + `"<mise shims>\pnpm.exe" %*`）；探针目录 `.bin/pnpm.cmd` 即此用途 |
| `pnpm bundle` 默认目标是 macOS | `Build for macOS is supported only on macOS` | 必须显式 `--os win --arch x64` |
| `--skip-prepare` 会跳过 koffi 暂存 | afterPack 报 `CUA plugin koffi runtime is incomplete` | 先跑一次 `prepare:agent-bundle`（会打印 `staged koffi into …`），再带 skip 跑 bundle |
| **打包时应用正在运行** | NSIS 阶段报 `app-builder.exe process failed ERR_ELECTRON_BUILDER_CANNOT_EXECUTE`，日志里真因是 `remove …\dist\win-unpacked\d3dcompiler_47.dll: Access is denied` | electron-builder 要重写 `win-unpacked`，运行中实例锁住了 DLL。**先退出已启动的打包版**（`taskkill /IM "ZCode Preview.exe" /F`）再打包 |

产物：`packages/desktop/dist/win-unpacked/`（绿色版，**已实测可启动并连上 allo**）与 `dist/*.exe`（NSIS 安装包）。

> **安装包现状**：首次失败的表象是 `app-builder.exe process failed ERR_ELECTRON_BUILDER_CANNOT_EXECUTE`，
> 真因是**打包时绿色版正在运行**、electron-builder 重写 `win-unpacked` 时被锁住的 `d3dcompiler_47.dll` 拒绝删除
> （`remove …: Access is denied`）。退出运行中实例后重跑即可出包。

打包态的模型选择器依赖个人 provider 配置：应用会在首次启动自建
`~/.zcode/v2/provider_config.json`（空规则），没有可用模型时 composer 会被
「当前没有可用模型」横幅挡住。用下面的工具**合并**一条探针 provider（只做加法，不覆盖应用写的结构）：

```bash
node tools/ensure-zcode-provider-config.mjs --target "C:/Users/<you>/.zcode/v2/provider_config.json" \
  --provider-id local-mimo-bridge --name "Local Mimo Bridge (probe)" \
  --base-url http://127.0.0.1:4500/v1 --models mimo-v2.5,mimo-v2.5-flash
```

> 该 provider 只决定桌面**显示**的模型；实际生效模型由 allo 侧决定（未映射时用 allo 的 `default_model`，
> 可用 `ALLO_MODEL_MAP` 显式映射）。

### 5.5 打包态启动

双击 `C:\workspace\zcode-bridge-probe\tools\start-zcode-allo-backend.cmd`：
自动找 `dist\win-unpacked`（或已安装目录）里的 exe，确保 allo App Server 在 8787 监听，
注入 `ZCODE_AGENT_SERVER_COMMAND/_ARGS_JSON/_STORAGE_ENTRY` 后拉起应用。

> 该 `.cmd` 必须保持 **ASCII-only**：cmd.exe 用系统 ANSI 代码页（本机 GBK）读取 .cmd，
> UTF-8 中文注释会破坏解析（实测把后续行粘成一条非法命令）。

### 5.6 人工验收清单

| 检查项 | 期望 |
| --- | --- |
| 启动后侧栏 | 列出 allo 的**有实际消息**的会话（标题来自 allo 会话名）；空会话（草稿/预热）不出现 |
| 新建任务并发一句 | 出现用户消息、"已工作 N 秒"折叠头、助手回复 |
| 点开历史会话 | 显示历史消息（冷恢复登记成功，不再报"会话不存在"） |
| 界面顶部 | 不出现 `fault.subscription.*` 与"重新连接" |
| 模型 | 显示 ZCode 侧 provider 名（由 `sync-zcode-models.mjs` 从 allo 注册表生成，provider id = allo 的 provider 名）；实际生效模型见门面 stderr 记账 |
| 改模型 | 编辑 `~/.agent-store/config.toml` 的 `[models.*]` → **重启启动器**（会自动再同步一次）→ 桌面模型列表变成新模型 |

## 6. 改动文件清单

- 门面（探针目录）：`bin/allo-zcode-bridge.mjs`、`lib/{v4-protocol,v4-row-model,v4-surfaces,storage-startup,session-store,session-mapping,host-housekeeping,allo-protocol,wire,thought-levels}.mjs`、`tools/{sync-zcode-models,ensure-zcode-provider-config,validate-frame-dump,cdp-eval,cdp-screenshot,start-zcode-desktop,schema-conformance,probe-workspace-wiring,probe-history-replay,probe-session-visibility,probe-sidebar-noise,probe-draft-reuse,probe-thought-level,probe-index-buckets,clean-sidebar-noise,clean-allo-empty-sessions,reassign-task-workspaces,delete-task-rows,sync-archived-to-allo}.mjs`、`test/*`
- ZCode 仓（本地未提交）：`packages/services/src/zcode-agent/zcodeAgentProcessManager.ts`、`packages/desktop/scripts/dev.mjs`、`packages/zcode-server-cli/tsup.config.ts`
- 本文与 38/39 号文档、`assets/40-zcode-desktop-e2e.png`
