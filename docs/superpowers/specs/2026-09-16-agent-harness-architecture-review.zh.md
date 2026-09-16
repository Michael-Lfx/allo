# Flowy 架构评审报告 —— 重大问题与行业一流 Agent Harness 对标

> 日期：2026-09-16
> 基线：HEAD `40d80c532`（2026-09-15）
> 方法：6 个 agent 团队并行深挖（核心循环 / 工具能力面 / 上下文·记忆·eval / 后端耐久性 / 前端 UX / 既有研究 + 行业 SOTA），每份报告的关键结论均回到源码逐条复核。
> 性质：只读评审，未修改任何代码。

---

## 一、结论摘要

Flowy 的「正确性工程」在多数维度上达到甚至超过商业 harness 的水准——流协议校验、编辑正确性机器、准入层持久化、前缀缓存纪律。但它存在三个系统性问题：

1. **执行面安全在主力平台（Windows）上完全缺席**；
2. **持久化的边界停在「准入」而非「执行」**，崩溃即丢进行中的 turn；
3. **组织性债务**：无 CI、god 模块持续膨胀、投入结构严重向 UI 倾斜。

一个关键参照：Claw-SWE-Bench 研究（arXiv 2606.12344）实测**同样的模型，仅换 harness 适配层，Pass@1 就相差 27.4 个百分点**（OpenClaw 19.1%→73.4%）。这正是 Flowy 这类自研 harness 的价值所在，也是当前短板最值钱的地方。

---

## 二、重大问题（按严重度分级）

### 🔴 High

| # | 问题 | 证据（已核实） |
|---|---|---|
| H1 | **无 CI**。`.github/workflows/` 只有 `release-modelscope.yml`（发版用）。全仓库 0 处自动跑测试 / lint / typecheck。项目有 ~15,900 个测试 + 15 个质量门禁脚本（`bun run check`），全部手动执行；pre-commit hook 需手动 `bun run setup:git-hooks` | 亲自 grep 确认 |
| H2 | **Windows / Linux 无进程沙箱**。唯一真实隔离 `MacSeatbelt` 是 Unix-only；Windows 上它直接返回 `CapabilityDenied`——即**开启沙箱会让 Bash 完全不可用**，关闭则是 `UnrestrictedLocalOwner` 零隔离。产品主力平台恰是 Windows Tauri 桌面 | `nomi-process-runtime/src/platform/windows.rs:2876-2881`；`bootstrap.rs:717-725` |
| H3 | **skill 内嵌 shell 绕过权限链**。权限链第 3 步判定「安全」的依据是 `hooks_raw.is_none() && allowed_tools.is_empty()`，但正文里的 ` ```! ` 代码块和 `` !`inline` `` 既不是 hooks 也不在 allowed_tools 里，被无审批执行。一个空 frontmatter 的 skill 内含 `` !`curl … \| sh` `` 会被自动放行 | `nomi-skills/src/permissions.rs:96-101`；`nomi-skills/src/shell.rs:32-68`。**缓解**：`LoadedFrom::Mcp` 的 skill 豁免执行，攻击面限于本地导入 |
| H4 | **崩溃后运行中的 turn 不恢复，只被结算**。持久化边界在准入层（收据、租约、幂等），turn 本身无检查点；崩溃后 boot recovery 把 `status='work'` 的消息 terminalize，已生成内容丢弃 | `nomifun-conversation/src/boot.rs:39,110-158`；`nomifun-ai-agent/src/orphan_recovery.rs:119` |
| H5 | **无 per-user 并发隔离**。并发控制是 per-conversation；会话路由只挂 `auth_middleware`，无限流器。单用户可占满 agent 构建槽位，饿死其他人 | `nomifun-app/src/router/routes.rs:754-755` |
| H6 | **默认 eval harness 是自证循环**。未知 case 会把 scorer 的期望 marker 直接回写进 transcript（注释原文："so unknown demo cases still have a chance to pass offline"）。真正的 `LiveNomiHarness` 存在但从无任何流程自动调用 | `nomi-agent-eval/src/harness.rs:64-86` |
| H7 | **无语义 / 向量记忆**。`companion_memories` 表有 `embedding BLOB` + `embedding_model` 列，但检索路径**零次 cosine / 向量距离查询**，纯 FTS5 BM25。更痛的是：三元分词器无法匹配 <3 字符的词（如「咖啡」），只能降级 LIKE 全表扫描——对中文面向产品是双重打击 | `nomifun-companion/src/memory_search.rs:1-16`；`store.rs:562-563` |
| H8 | **god 模块持续膨胀**。`execute_turn_inner` ~1700 行（三重嵌套循环）；`service.rs` 17,191 行、`stream_relay.rs` 13,611 行、`ipcBridge.ts` 8,257 行、`SendBox/index.tsx` 2,513 行、`manager/nomi/agent.rs` 6,524 行。且结构性债务在增长：`openai.rs` 从 3,731→4,288 行，`manager/nomi/agent.rs` 6,418→6,524 | 亲自 `wc -l` 核实全部数字 |

### 🟡 Med

| # | 问题 | 证据 |
|---|---|---|
| M1 | **无真实 tokenizer**：`chars/4` 估算，中文低估 4–8 倍，水位只在请求成功后才被 provider 值向上修正。首次溢出仍要付一次失败请求 | `nomi-agent/src/compact/estimate.rs:5-20` |
| M2 | **429 限流不重试**，注释明写 "rate limits are surfaced immediately"，直接回滚整个 turn | `nomi-providers/src/retry.rs:20-22` |
| M3 | **`context-archive` 无 GC**，`.flowy/context-archive/` 无界增长，全仓库无清理逻辑 | 亲自 grep 确认 |
| M4 | **`CompactState` 不持久化**，会话恢复后断路器 / 停滞闩锁 / 水位历史全部丢失，summarizer 若已损坏会重新猛打 provider 而非机械折叠兜底 | `nomi-agent/src/session.rs:25-65` vs `engine/mod.rs:796` |
| M5 | **MCP 工具无 per-tool 权限闸门**，只有粗粒度 category / session 级审批；`mcp__` 是保留前缀而非白名单边界 | `nomi-mcp/src/tool_proxy.rs` |
| M6 | **MCP 客户端不完整**：无 elicitation / sampling / prompts / completions / roots，协议版本停在 `2025-03-26`（当前 spec 为 2026-07-28）。依赖 elicitation 做认证的服务端静默降级为 tool-only | `nomi-mcp/src/protocol.rs:216-237` |
| M7 | **验证是字符串匹配而非真实解析**：`NEEDLES` 启发式猜「这是验证命令」，无法区分「编译通过 / 测试通过 / lint 干净」，建立在启发式上的 HardGate 会双误 | `nomi-coding/src/verify.rs` |
| M8 | **配额字段是 no-op**：`nomifun-channel` 的 `rate_limit` 唯一读取点是测试断言；webhook fire-and-forget 无 outbox 无重投；abandoned admission 重试无总超时，可永久空转 | `nomifun-channel/src/types.rs:827` |
| M9 | **背压即断流**：客户端慢则被踢下线 "durable resync"，LLM turn 无服务端 replay cursor，`Lagged` 是终态——turn 被终止以保终态一致。正确但 UX 代价真实 | `nomifun-conversation/src/stream_relay.rs:2252-2264` |
| M10 | **CLI 取消路径有缺陷**：`nomi-cli` 的 `Stop` 裸 `break`，未调用 `abort_current_turn`，会在 Anthropic 协议上留下无配对 `tool_result` 的 `tool_use` | `nomi-cli/src/main.rs:666`（见下方更正 1） |

### 🟢 Low

- 无工具版本 / 能力协商（`ToolDef` 无 version 字段）。
- 前端死依赖（shadcn 装了零引用、antd 仅 1 处）、4 套 canvas 子系统（xyflow / excalidraw / tldraw / three.js）推高包体积。
- 审批组件重复实现（`MessagePermission` vs `MessageAcpPermission`，两套并行 i18n key 映射）。
- 2,224 处中文 `defaultValue` 兜底，且 i18n `--check` 未进 CI。
- a11y 是装饰性的（`role="button"` div、无 a11y 测试）。
- trace 无 latency / cost 字段、无 OTel 导出。
- 338 处生产 `unwrap`（多数在 `#[cfg(test)]`）。

### 投入结构（独立统计）

近 200 次提交的文件变更分布：`ui/src/` 1282 次（约 80%）、`nomi-vimax` 137、`nomifun-learning` 112；而 agent 核心仅 `nomi-agent` 56、`nomi-tools` 16、`nomi-coding` 13。**产品由 UI 与领域功能驱动，agent 引擎核心长期投入不足**——这解释了为何 UI 精致度一流而 H2 / H6 / H7 这类执行面债务长期存在。

---

## 三、与行业一流 agent harness 的差距（按杠杆排序）

| # | 能力 | 行业标杆（2026-09） | Flowy 现状 |
|---|---|---|---|
| 1 | **内核级沙箱 + 最小权限** | Factory 最强（Seatbelt / bubblewrap+seccomp / HTTP 代理全平台、fail-closed、组织级 deny 不可下游覆盖）；Claude Code（macOS Seatbelt + Linux bubblewrap + per-command `allowed_domains`）；Codex（容器化 + `auto_review` 边界升级） | **最大差距**：macOS-only，Windows / Linux 裸奔；env 继承泄漏宿主 API key；浏览器出网审批未接线（`TODO(E5->F1-egress-approval)`） |
| 2 | **子 agent 编排** | Claude Code：20 并发 / 3 层深、可恢复 ID、per-agent MCP 与权限、worktree 隔离、输出扫描防注入；Devin Managed Devins；Cursor 云端子 agent | 有 `isolated_subagent`（硬编码 depth-1、4–8 turn 上限）和 `local_delegate`（并行），但无一等公民抽象、不持久、无 DAG 编排（其自述承认需平台宿主） |
| 3 | **可恢复的 durable execution** | LangGraph checkpointers（SQLite / Postgres / Mongo / Redis，时间旅行 / fork）；Temporal / Restate 以 workflow 为恢复单元；Codex 云端隔离可复现环境 | **持久化单元是准入收据，不是 turn**（H4）。有 `PendingConversationEffect` outbox 雏形但只覆盖外部效应 |
| 4 | **语义记忆与检索** | Claude Code auto memory（类型化主题文件 + 索引）；Cursor 代码库 + 对话向量检索；OpenAI Agents SDK Sessions | Markdown 文件 + 全索引注入系统提示（200 行 / 25KB 截断），无检索引擎；companion 库的 embedding 列是死 schema（H7） |
| 5 | **MCP 生态时效** | spec `2026-07-28`：无状态、OAuth 2.1（RFC8707 / 9728）、Tasks / Skills / Apps、注册表 ≥1,800 服务端 | 停在 `2025-03-26`，无 OAuth / 重连 / `list_changed` / elicitation（M6） |
| 6 | **深度生命周期 hooks** | Claude Code ~24 个事件（PreCompact / PostCompact / SubagentStart / FileChanged…），支持 command / HTTP / MCP / LLM / 子 agent 处理器 | 3 种 hook（pre / post 工具，pre 可阻断、30s 超时）——有机制但广度差一个量级 |
| 7 | **成本治理** | Pi / dsh：真实 tokenizer + EMA 校准 + cache-breakpoint 归因 + per-turn 成本遥测 | `len()/4`（M1）；credits 纯服务端可算，本地无成本视图 |
| 8 | **异步 / 事件驱动执行** | Cursor Subscriptions、Devin Automations、Claude Routines、PR / Slack 事件触发 | 双端 cron 已有（有持久预约台账，**这块是优势**），但无 PR / Slack 事件接线；webhook 不重投 |
| 9 | **eval 基础设施** | 各家均把 eval 接进 CI；Claude plugin eval suites | 框架骨骼优秀（resume / pass@k / 13 种 scorer / 隔离 live harness），但默认 harness 自证、且无 CI 自动跑（H1 + H6） |

> **值得强调**：**完成验证（evidence gate）这一项 Flowy 反而领先**——`CodingHarness` 默认 `VerificationMode::HardGate` + EvidenceRequired，比 Cursor Review / Bugbot、Devin Review Autofix 更严格。这是真正的差异化资产，不该被其他短板掩盖。

---

## 四、真正的强项

1. **流协议 fail-closed 校验**：~30 项检查（重复 / 空 `tool_use_id`、未宣告工具名、Done 时 preview↔complete 对账），工具卡片仅在 `Done` 提交后发布——中断的流不留幽灵状态。
2. **编辑正确性机器**：anchor + hash 校验 + 位移 / 过期检测 + 空白容忍回退 + 原子写 + 硬验证门 + 收敛失败追踪。这是多数 harness 做不对的地方。
3. **浏览器 redline 门**：在 Yolo（全自动批准）模式下仍然保持武装，支付 / 删除 / 提交必须带外确认——且注释明确警告不要反转该逻辑。安全直觉在线。
4. **准入层持久化**：SQLite 收据台账 + 触发器强制状态机 + 租约 + 幂等键 + boot 恢复 + crash-loop governor（3 次 / 60s 退避）。DB 是生产级（WAL、busy_timeout、flock 迁移、quick_check）。
5. **Schema 卫生高于行业 norm**：线性时间 regex（抗 ReDoS）、有界遍历预算、仅本地 ref、MCP 工具原子批量注册 + 单调收窄白名单（后注册的不可信来源无法扩大已批准集合）。
6. **前缀缓存纪律**：冻结工具表 + `sent_prefix_len` 字节冻结 + 动态内容（日期 / plan / RAG）走 turn tail 而非系统提示——缓存工程扎实。
7. **测试密度**：前端 772 测试文件 vs 886 tsx；后端目标 crate ~2,800 测试；含对抗用例（redteam、pending-confirmations-recovery）。
8. **trace 系统生产级**：有界队列 + 丢弃计数 + 健康状态 + 128KiB 事件预算 + 1GiB 配额 GC（跳过活跃文件）+ tombstone 生命周期。
9. **统一执行词汇表**（Agent / Conversation / Turn / Participant / Step / Attempt）由 `scripts/check-agent-vocabulary.mjs` 强制——对比竞品的 ad-hoc 编排是差异化。
10. **取消语义在生产路径正确**（见更正 1）：先合成孤儿 `tool_result`、脱敏未发送图片，再回滚 provisional 状态。

---

## 五、优先行动建议

**P0（立即）**

1. **补 CI**：加一个跑 `cargo nextest` + `bun run check` + `cargo clippy` 的 workflow。这是所有质量问题的放大器，成本最低、收益最高。
2. **Windows 沙箱**：要么实现 job-object / AppContainer 隔离，要么在无沙箱平台上把 `Bash` / `exec_command` 强制走审批门（当前等于裸奔）。
3. **修复 eval 自证循环**：移除 `OfflineDemoHarness` 的 echo 分支或改为显式标记 `unknown`，并让 `LiveNomiHarness` 至少在夜跑中被调用。

**P1（本季度）**

4. **turn 检查点**：把 `OutputCheckpoint` 从内存回滚升级为可恢复单元；或更便宜——形式化「durable resync」契约，给 LLM turn 加服务端 replay cursor，让断线客户端能恢复进行中的 turn 而非终止它。
5. **per-user 并发上限 + 真正实施 `rate_limit`**（字段已存在，接上线即可）。
6. **tokenizer**：接入真实 tokenizer（或至少为 CJK 加分支），水位是整个 compaction 的地基。
7. **skill 内嵌 shell 纳入权限链**：任何含 `!` 块的 skill 一律视为非安全。
8. **content_ref 脱敏前置**：`tool_execution.rs:720` 应在持久化之前先过 `redact_secrets_owned`。

**P2（持续重构）**

9. 拆分 `execute_turn_inner` / `service.rs` / `stream_relay.rs` / `ipcBridge.ts`——这是 R1 / R2 拆分的前置条件，也是所有后续改进的摩擦源。
10. 记忆层引入真正的向量检索（embedding 列已就位，接通检索路径即可）。
11. MCP 升级到当前 spec 并补 elicitation / sampling。

---

## 六、新旧账分离：哪些是新发现，哪些是旧账

区分「团队已知未做」和「新发现」，决定该催谁。

### A. 已被既有研究解决（不要重复报告）

✅ 完成证据门（`HardGate` + EvidenceRequired）、✅ 压缩归档（`context-archive`）、✅ `/compact <focus>` 传参、✅ 工具表排序归一化、✅ 空闲压缩路径（`IdleCacheExpired`）、✅ eTLD+1 防火墙 fail-closed、✅ 编辑失败恢复提示（`EditFailureKind`）。

### B. 团队自己的旧账（已知但未做，且更严重了）

| 本报告编号 | 既有研究中的编号 | 状态核实 |
|---|---|---|
| H4 turn 不恢复 | **R1** 事件日志会话持久化 | 仍 open；`session.rs` 仍是 `fs::write` 全量写 |
| H8 god 模块 | **R2 / R3 / R6** 拆分 | **恶化**：`openai.rs` 3731→4288、`manager/nomi/agent.rs` 6418→6524、`execute_turn_inner` 仍在长 |
| H7 无向量记忆 | **R7** `MemoryIndexProvider` | 仍 open；且发现 embedding 列是死 schema |
| M1 CJK tokenizer | 优化清单明列 | 仍 open，`estimate.rs` 无 CJK 分支 |
| M5 / M6 MCP 缺失 | MCP OAuth / reconnect | 仍 open，且协议版本已落后两个大版本 |
| 安全-env 泄漏 | 优化清单 | 仍 open。**核实细化**：注入向量（DYLD / LD / NODE）已被 `dangerous_inherited_environment` 清洗，但宿主 API key 仍被子进程继承 |
| 安全-content_ref | **N3** | 仍 open。已亲自确认：`tool_execution.rs:720` 把未脱敏的 `r.content` 传给 `truncate_result`，脱敏在 `:758` 的另一绑定——密钥先落 `%TEMP%` 才脱敏 |

### C. 新发现（既有研究未覆盖）

- **H1 无 CI**（最高杠杆，此前无人提）
- **H3 skill 内嵌 shell 绕过权限链**（安全漏洞，权限链第 3 步的「安全」定义漏了正文 `!` 块）
- **H5 无 per-user 并发隔离 + `rate_limit` 字段是 no-op**（配额字段定义了却从不执行）
- **H6 eval 自证循环**（默认 harness 把期望答案回写 transcript）
- **M2 429 不重试**、**M3 archive 无 GC**、**M4 `CompactState` 不持久化**、**M7 验证靠字符串匹配**、**M9 背压即断流且无 replay cursor**
- **M10 CLI 取消留孤儿 `tool_use`**（生产路径无此问题，仅 CLI）
- **投入结构倾斜**：近 200 提交 `ui/src/` 占 ~80%，agent 核心（nomi-agent / tools / coding 合计仅 ~85 次）

### D. 对子 agent 结论的两处更正

1. 「`abort_current_turn` 生产中无调用者」→ **错**。`nomifun-ai-agent/src/manager/nomi/agent.rs:1770` 生产路径在 `tokio::select!` 的 cancel 分支**先调 abort 再 break**，并附 fail-closed 生命周期注释。所以生产路径取消是正确的、堪称亮点；缺陷仅限 CLI。故 M10 降级 High→Med。
2. 「env 泄漏」→ **需细化**。注入向量（`DYLD_*` / `LD_PRELOAD` / `NODE_OPTIONS` / `CLAUDECODE`）已被清洗，泄漏的是宿主 API key，不是任意注入。

---

## 七、最终判断

**B 类旧账（R1 / R2 / R6 / R7）比 C 类新发现更危险**——它们是结构性债务，且在过去三周持续恶化，而 C 类多是可独立修补的点。若只能做三件事：**补 CI（H1）→ Windows 沙箱或审批门（H2）→ 修 eval 自证循环（H6）**，因为 H6 让你们无法知道自己有没有在退化。

---

## 附：对标信息来源

- Claude Code：code.claude.com/docs/en/{overview,sub-agents,hooks,memory,sandboxing}；CHANGELOG
- OpenAI Codex：learn.chatgpt.com/docs/{sandboxing,cloud,llms.txt}.md
- Cursor：cursor.com/docs/llms.txt
- Devin：docs.devin.ai；cognition.com/blog
- Factory：docs.factory.ai/autonomy-and-safety/sandbox
- MCP：modelcontextprotocol.io/specification/2026-07-28/；registry.modelcontextprotocol.io
- 开源框架：openai/openai-agents-python、langchain-ai/langgraph、0xPlaygrounds/rig、browser-use/browser-use
- 基准：arXiv 2606.12344（Claw-SWE-Bench）；openbench.run；swebench.com
