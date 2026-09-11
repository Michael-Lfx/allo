# 开放决策书（Open Decisions）

> 状态：**已拍板（2026-09-10：「全部按建议」）**。用途：把 `16` §5.2「剩余任务总表」中**必须由人拍板**的事项集中在一页，一次回复即可解锁连续推进。
> **拍板结论**：`D1=A D2=B D3=B D4=A D5=C D6=A D7=A D8=A D9=A D10=A D11=A D12=A`。执行顺序见文末「拍板后的连续推进顺序」。
> 每条注明**解锁哪些 R**；未在本表出现的 R（R3 / R6 机制项 / R10 / R11 / R19 / R25 / R29 / R31）**无卡点**，批准/解除口径后我直接做，不需你逐条过。
> 拍板结果请直接改「选择」列（或回复编号），我按结果更新本文件并执行。

---

## 0. 速览

| # | 决策 | 选项 | ⭐ 建议 | 解锁 | 拍板结果 |
| --- | --- | --- | --- | --- | --- |
| **D1** | 口径是否解除 | A 全解除 / B 只解 SDK 面 / C 维持现状 | **A** | R1–R33 全部 | **A** ✅ |
| **D2** | 站点托管与域名（Q2） | A 保持裸 IP / B 自定义域名 + HTTPS（现 VPS）/ C EdgeOne / D GitHub Pages | **B** | R7、R32、R6 部署项 | **B** ✅（**落地延后：2026-09-11 用户决定「站点部署卡点相关问题延后」，R7 / R32 / R6② 一并延后、不再挂计划**） |
| **D3** | 审批策略来源 | A 不做 / B `config.toml` 声明 / C 内置分级 | **B** | R8 | **B** ✅ |
| **D4** | 多标签订阅归属 | A 各标签独立 + 全局副作用选主 / B 单写者 / C 不协调 | **A** | R13 | **A** ✅ |
| **D5** | 凭据值存放位置 | A DB 加密列 / B OS keychain / C `config.toml` + env（不落库） | **C** | R22 | **C** ✅ |
| **D6** | 破坏性阻断（依赖 / `strict`） | A 按规范实现 + 配置逃生口 / B 只做 `strict` / C 只改规范 | **A** | R23、R24 | **A** ✅ |
| **D7** | `auto_update` 执行逻辑 | A 仅官方源 / B 全部源 / C 不实现 | **A** | R27 | **A** ✅ |
| **D8** | 本地化变体优先级 | A 按界面语言回退 / B 保持透传 / C 以 `_en` 为主 | **A** | R28 | **A** ✅ |
| **D9** | `Finish` 是否等待蒸馏 | A 不等待（后台 child）/ B 维持 + UI 分级 / C 维持 + 默认关 | **A** | R30 | **A** ✅ |
| **D10** | 兼容性承诺口径 | A beta 期不承诺向后兼容 + 写升级指引 / B 承诺 0.x 兼容 / C 不写 | **A** | R4 | **A** ✅ |
| **D11** | 协议增量批准 | A 全部（additive 优先）/ B 只批准 additive / C 全不批 | **A** | R12、R15、R17、R20、R21、R25 | **A** ✅（R17 的 `skill/create\|update\|delete` 已于 2026-09-11 批 4 按 additive 落地：WS-only、不进 SDK 包、无 HTTP 绑定；未新增第 4 个方法——`skill/copy` 与编辑面所需的 `skill/source` 都需新拍板，见 `16` R17 落地记录） |
| **D12** | 数据源口径（3 项子决策） | 见 §D12 | **A** | R5、R8、R14 | **A** ✅ |
| **D13** | 卡点处置四档（2026-09-11 卡点审查后定执行顺序；**同日 C 档二次复核改判**） | A 全做（R17 三动词 UI + R16 `agent` 开关 + 措辞订正）→ B 全做（R17 编辑字段级 patch + 复制原语 + R14 加列迁移）→ D（R22 两步同批）→ C **按性质分别处置**（原写「维持不动，仅 R33 开立项页」）：R23/R24 → 已批准待排期；R20 → 拆 R20a 可做 / R20b 等待；R15 → 载体选型独立拍板；R33 → 立项页已建；R6① → 改判为产品取舍 | **按此顺序** | R6①、R14、R15、R16、R17、R20、R22、R23 / R24、R33 | **已批准并执行（2026-09-11，用户指令）** ✅——判定与逐条依据见 `16` §5.3「卡点处置四档」。**改判说明**：C 档原「维持不动」的表述被复核推翻——它把**排期**误写成**决策阻塞**（R23/R24 的 `21` D6=A 早已批准）、把**可做部分**一起挂死（R20 归属/R15 载体的数据其实已具备）、把**不存在的守卫面风险**当阻塞理由（R6①，docs 全文已内联进 bundle）、且**唯一承诺动作未兑现**（R33 立项页当时并不存在）。改判只改定性与解锁条件，「未做」这一事实不变 | **执行结果（2026-09-11，本会话）**：**A 档全部完成**（R17 三动词 WebUI + R16 `agent` 分区开关 + 措辞订正）；**B 档 ①② 完成**（`skill/update` 服务端字段级合并、`skill/copy` + 目录级复制原语，含 UI），**B 档 ③ R14 ✅ 已完成（2026-09-11 R14 轮）**（新迁移两列 + 写入点 / 仓储 / 读回投影 / 前端重载回填全链路；验收＝重载后仍显示上一轮 token 与金额，真实读数见 `16` §5.3「R14 逐轮 usage 持久化 · 落地记录」）；**D 档 R22 已完成（2026-09-11）**（两步同批落地：值不入快照 + 启动按引用注入，见 §D5「落地情况」）；**C 档立项页已兑现**（`22-webui-productionization.zh.md`，四组 + 验收 + 假保护红线 + §6 V1–V4）。真实读数：`nomifun-app-server --lib` **94/0**、`nomifun-extension --lib` **444/0/5 ignored**、`cargo check --tests --workspace` **exit 0**、`web test` **347 passed/1 skipped**、`typecheck`/`build` 均 exit 0；明细见 `16` §5.3「A / B 档与 C 档立项页 · 本轮落地记录」。（**2026-09-11 R14 轮追加的 B 档 ③ 真实读数**：`nomifun-db --test app_server_context_usage_repository` **5 passed / 0 failed**、`nomifun-app-server --lib` **96 / 0**（+2 例）、`nomifun-extension --lib` **444 / 0 / 5 ignored**、`cargo check --tests --workspace` **exit 0**、`web test` **362 passed / 1 skipped**、`typecheck` / `build` 均 exit 0、client 两文件 **19 passed**；明细见 `16` §5.3「R14 逐轮 usage 持久化 · 落地记录」。**同轮发现既有基线红（非本项、已证伪归属、未修）**：`cargo test -p nomifun-db` 整包在 `tests/id_schema_contract.rs` 有 2 例失败——表数硬编码 `110` 实际 **114**；`oauth_tokens.registration_id` 期望 TEXT 实际 **INTEGER**（出自迁移 `052`），把 `058` 移出迁移目录重跑同样失败） |

> **拍板记录（2026-09-10，用户回复「全部按建议」）**。随之生效的附加默认值：
> - **D3**：未配置 `[approvals]` 时**完全放行**（对现有行为零影响），只有被显式列出的工具才走审批。
> - **D7**：自动更新**默认关闭**；仅当 `[marketplace] auto_update_interval_hours` 被设置且源为官方时才启用。
> - **D2**：已批准方案，但**落地仍需你提供域名与 DNS 访问权**（我无凭据，不阻塞其余批次）。
> - **D9**：接受「蒸馏延迟/失败不影响会话正确性」，`Finish` 不再等待该 child。
> - **D10**：beta 期不承诺向后兼容，破坏性变更走 minor + changelog。

---

## D1 · 口径是否解除

**现状**：`16` §5.1 写死三条限制——① A3/A4/A5「不再主动做，须真实外部 issue」；② 其余 WebUI「按真实使用反馈排，不按完整性排」；③ 方向四「需单独立项」。R1–R33 里绝大多数被这三条挡着。

| 选项 | 含义 | 代价 |
| --- | --- | --- |
| **A ⭐** | 三条全部解除：R1–R33 全量纳入执行 | 含**协议破坏性改动**（A3 的事件面）与 WebUI 重构；量级数周–数月，需按批次验收 |
| B | 只解除 SDK 面（A3/A4/A5），WebUI 继续等反馈，方向四仍立项 | SDK 公共面会先动，WebUI 后续可能返工 |
| C | 维持现状，只做无卡点项（R3/R6 部分/R10/R11/R19/R25/R31） | 放弃 R1/R2/R5/R8–R18/R20–R24/R26–R28/R30/R32/R33 |

**我的建议：A**。你已明确要"完成剩余任务总表的所有任务"，A 是唯一能满足它的选项；风险靠**批次 + 每批验收 + 已知偏差登记**消化，不靠少做来消化。

> ⚠️ 若选 A，请同时确认你接受：A3 会让 `event_type` 的联合类型收窄（消费者需重新生成类型）；W11/W17 等 UI 会替换现有调试视图。

---

## D2 · 站点托管与域名（`16` Q2）

**现状**：线上是**裸 IP + HTTP** 的 `irm … | iex`；`README` 已对齐真实部署，但信任问题未解。`deploy-site.yml` 的 `push` 触发是注释掉的（仅 `workflow_dispatch`）。已知约束：EdgeOne preset 域名带签名 `eo_token` 且按路径签名，**不能直接做公开源**。

| 选项 | 说明 | 需要你提供 |
| --- | --- | --- |
| A | 保持现状，只把文档写准 | 无 |
| **B ⭐** | 现有 VPS 上挂**自定义域名 + HTTPS**（DNS + 证书 + 反代） | **域名**与 DNS 访问权 |
| C | EdgeOne（或同类）托管 | 域名（preset 域名不可用）+ 账号 |
| D | GitHub Pages | 仓库 Pages 权限 |

**为什么建议 B**：它是**一次性**解决三件事的最短路径——(1) 下载/安装的信任问题、(2) R32 的「换 HTTPS + CDN 缩短镜像代价」、(3) R6 的「市场数据刷新纳入发布流程」（有稳定域名即可做定时发布）。C/D 要么受签名域名限制，要么失去国内可达性。

---

## D3 · 审批策略来源（R8）

**现状**：协议已有 `approval/request` + `approval/respond`，但 `capabilities.approvals` **硬编码 `false`**（`lib.rs:385`），webui 无界面。按 `16` §6「不做假开关」，没有真实策略来源就不该建 UI。

| 选项 | 说明 |
| --- | --- |
| A | 不做，能力维持 `false` |
| **B ⭐** | 策略由 **`~/.agent-store/config.toml` 的 `[approvals]`** 声明（哪些工具/类别需审批、超时、是否记住）——与你「尽量用 config.toml 控制」的口径一致；同时解冻 `capabilities.approvals` |
| C | 内置分级（按工具类别硬编码 读/写/网络），不可配 |

**选 B 需要你补一句**：默认策略是「全部放行、只把显式列出的当审批」还是「只放行只读、其余审批」？我建议**前者**（对现有行为零影响，`[approvals]` 未配置即完全不变）。

---

## D4 · 多标签订阅归属（R13）

**现状**：`19` §3 W8 已落地 Toast 与断线重连；多标签协调当时未做（文档自己标为「需先拍板归属策略」）。

**落地情况（2026-09-10，批 3 R13）**：已按 A 实现——`web/src/lib/global-effects.ts`（选主 + 跨标签去重表 + 三级确定性降级）、`web/src/lib/notice-performers.ts` + `global-effects.runtime.ts`（桌面通知 / 声音，浏览器与 i18n 适配）、`web/src/lib/run-notify.ts`（后台 Run 终态投影）。实现细节、降级阶梯与验证输出见 `16` §5.2 R13 行。

**收口补正（2026-09-11）**：A 的边界「只协调逃出标签的副作用」在通知侧还要求**该副作用真的能到达**——断线重连只重挂了会话订阅，被跟随 Run 的订阅不会自己回来，故断线窗口内进入终态的 Run 没有任何提示。已在 `connect()` 的 `lost` 分支补 `runSubscription.rearm()`（复用 T8 机制），不改变 A 的归属结论。

| 选项 | 说明 |
| --- | --- |
| **A ⭐** | 每个标签页**各自独立**订阅（各自 WS）；只有「全局副作用」（桌面通知/声音/后台 Run 提醒）用 `navigator.locks` 选主，保证**只触发一次** |
| B | 单写者：一个标签页拥有全部订阅，其余标签只读并通过 `BroadcastChannel` 取数 |
| C | 不协调，只在文档说明「避免多开」 |

**为什么建议 A**：改动最小、无需跨标签状态同步协议；B 的复杂度（选主失效、接管延迟）与收益不成比例。

---

## D5 · 凭据值存放位置（R22，`17` P1）

**现状**：MCP 连接器的 `env` **明文写入快照**（`import.rs:1100-1112`），违反 `17` §6「导入时只建立 schema 与引用」。直接改成 `[REDACTED]` 会让 MCP 拿不到凭据而**启动失败**（功能回归），所以必须先定「值放哪」。

| 选项 | 说明 | 影响面 |
| --- | --- | --- |
| A | 值存 DB，但用 DPAPI/OS 加密列 | 仍需落库；密钥管理是新问题 |
| B | 值存 OS keychain（Windows Credential Manager / DPAPI 封装） | 跨平台抽象成本高 |
| **C ⭐** | **不落库**：快照只存**引用名**；值由 `~/.agent-store/config.toml` 的 `[credentials]`（或进程 env）提供，MCP 启动时**按引用解析注入** | 需改 MCP 启动路径加一个 resolver；仓库/快照/日志里**永不出现明文**，且与你「配置集中到 config.toml」的口径一致 |

**附带效果**：选 C 后 `17` §10 P1 可直接标「已修」，并把「安全存储注入」这一前置从待办中移出。

**落地情况（2026-09-11，`16` R22）**：已按 C 实现。引用语法＝`secret:<KEY>`（沿用浏览器引擎既有 `secret:NAME` 约定，`nomifun-common/src/secret_ref.rs`）；导入期把 MCP `env` 中**敏感键**的值改写为引用（键名判定与 `userConfig` 同一谓词），快照/DB 只留引用；MCP 启动时由装配路径（连接测试 + `factory/nomi.rs` + `factory/acp.rs`）按引用解析注入，缺凭据即**省略**该变量（fail-closed）。值侧存放＝`~/.agent-store/config.toml` 的 `[credentials]`（进程 env 兜底）：宿主 `apps/agent-store` 启动时调用 `secret_ref::set_credentials` 注册进进程；该表**不入** `config/get`（视图是显式投影）也不入 `config/set`（`deny_unknown_fields` 拒绝）——协议面既读不到也写不进凭据。验证：`nomifun-common` 6 例 + `nomifun-importer` 38 例（含「明文不落快照」）+ `nomifun-ai-agent` 2 例 + `nomifun-mcp` 251 例 + `nomifun-app-server` 17 例 + `importer_e2e` 10 例全绿。

---

## D6 · 破坏性阻断：依赖不可满足 + `strict`（R23、R24）

**现状**：`17` §7 承诺「依赖不可满足即阻断」「`strict=true` 且缺 `plugin.json` 即阻断」，但实现里**只登记、从不阻断**（无 `semver` 解析，`PluginManifest` 无 `strict` 字段）。

> **层级订正（2026-09-11 C 档复核）**：上句「无 `semver` 解析」**只对导入层成立**。extension 层**另有**一条链：`nomifun-extension/src/dependency.rs:112-129` 已用 `semver::VersionReq` 解析，且**已接进真实加载路径**（`registry_helpers.rs:46-61` 的 `load_and_validate` 在 `:56` 调用、`:57` 用 `load_order` 排序 ← `registry.rs:111` / `:166`）——**该层只缺「阻断」**（`registry.rs:136-138` 只 `warn!`，`valid` 未当门）。因此本决策的落地不是「实现 SemVer 解析」，而是 **① extension 层补阻断判定 + ② 导入层新写校验**；两侧都按 A 的逃生口形式。

| 选项 | 说明 |
| --- | --- |
| **A ⭐** | 按规范实现阻断，并给**配置逃生口**（如 `[import] strict_dependencies = false` 临时放行）；**规范未发版，故实现即生效、不是破坏性变更、无需 changelog 公告** |
| B | 只实现 `strict` 阻断，SemVer 满足性单独做 |
| C | 只改规范，标注「当前不阻断」，实现不做 |

**为什么建议 A + 逃生口**：规范已承诺了这两条，不做就是永久偏差；逃生口保留是因为它本身有产品价值（用户确实可能想强装不满足声明的依赖），**不是为兼容而设**。

> **落地口径（2026-09-11 C 档复核 + 版本框架订正后）**：本决策**已批准（D6=A），但未排期**。`16` C 档原把 R23/R24 写成「维持 v1.1 队列，解锁条件＝v1.1 解冻」，那是把**排期**误写成**决策阻塞**——D6 已满足，不存在待解冻的决策。**落地形式＝阻断默认开 + 逃生口**：规范未正式发版、无既有消费者，故实现阻断不会破坏任何已发布契约（原先"默认关以保兼容"的前提不成立，`16` §7 决策 4）。语义 + 反向用例先落地即可，不需要触发条件和公告窗口。

---

## D7 · `auto_update` 执行逻辑（R27）（当前仅一个标记，无任何行为）

| 选项 | 说明 |
| --- | --- |
| **A ⭐** | **仅对官方源**启用后台自动更新（定时 + revision/ETag 短路，沿用 `18` §5.2）；第三方来源**只保留标记、不自动拉取**——与 `18` §7 现有表述一致 |
| B | 全部来源都自动更新 |
| C | 不实现（维持「仅标记」） |

**选 A 需你给一个默认值**：刷新间隔。我建议**默认关闭、`[marketplace] auto_update_interval_hours = 6` 才启用**（避免后台流量与 D-SDK-1 ④ 的镜像带宽问题叠加）。

---

## D8 · 本地化变体优先级（R28）

**现状**：真实 skills 市场有 `description_zh/en`(×268)、`name_zh/en`、`category_zh/en`、`legacy_tags_zh/en` 等字段，当前**全部透传不消费**（`18` §4）。

| 选项 | 说明 |
| --- | --- |
| **A ⭐** | **按界面语言回退**：`{field}_{lang}` → `{field}`；`tags_zh/en` 优先于 `legacy_tags_zh/en`；缺失时回落到基线字段 |
| B | 保持透传，只在详情页额外展示（不参与排序/搜索） |
| C | 以 `_en` 为默认主字段 |

**为什么建议 A**：这是唯一能让中文用户看到中文名的做法，且回退链明确、不破坏现有基线字段。

---

## D9 · `Finish` 是否等待记忆蒸馏（R30）

**现状**：post-session 蒸馏（一次额外模型调用）在 `Finish` **之前**被 `await`，注释明写 `Finish is forbidden until this child closes`——这是**刻意设计**。代价是「回答已可见但仍显示正在处理 6–15 秒」。

| 选项 | 说明 | 风险 |
| --- | --- | --- |
| **A ⭐** | `Finish` **不再等待**蒸馏 child（改为同生命周期内后台 spawn，失败走既有 recovery 观测）；「回答完成」＝「轮次结束」 | 动 durability 顺序；需你确认「蒸馏延迟/失败不影响会话正确性」 |
| B | 维持现状，只做 R31 的 UI 文案分级（「正在收尾…」） | 行为不变，仅改善观感 |
| C | 维持现状，且**默认关闭**蒸馏（本机已是此状态） | 默认失去记忆蒸馏能力 |

**如果你不想动 durability 语义，请选 B**——我照样能连续推进，R31 就是它的实施项。

**落地（2026-09-11，批 5，按 A）**：`Finish` 不再等待蒸馏 child，改为**同轮次取消域内后台 spawn**（`distill::spawn_distill_exact_turn`）；child 持本轮 `turn_cancel` 的 clone，`await_exact_turn_child` 的取消契约不变，spawn 前先判 `is_cancelled()`（已取消则不建 child，`Cancelled` 分支仍是唯一终态出口）。「回答完成」＝「轮次结束」由此成立。失败仍走**既有**观测出口（tracing 的 debug/warn，无新事件类型）；**反向核实**蒸馏结果没有被任何后续步骤同步消费（usage 记账、DB、teardown 均不要求 child join，详见 `16` 卡点决策表后的「R30 落地记录」）。**已知残余风险**：同 workspace 的记忆索引并发写（`append_index_entry` 读全文→写全文）——该风险改动前就存在，未在本轮引入新机制。**唯一未做的确认**：宿主侧仍没有把 `[memory].distill_enabled` 接上 `set_distill_host_override`（`16` R16 的 agent 分区解锁条件），与本项无关。

---

## D10 · 兼容性承诺口径（R4）

**现状**：C2 要求写「升级与迁移指引」，但这等于对外承诺兼容策略，目前没有。

| 选项 | 说明 |
| --- | --- |
| **A ⭐** | 明确 **beta 期间不承诺向后兼容**：破坏性变更走 minor 号 + changelog 明示；给出逐版本升级步骤与「固定版本」建议 |
| B | 承诺 0.x 内 minor 不破坏（成本高，会束缚 A3 这类改动） |
| C | 不写升级指引（避免承诺，但读者无法判断能否升级） |

**注意**：选 B 会与 D1=A（A3 收窄 `event_type` 类型）**直接冲突**，需二选一。

---

## D11 · 协议增量批准（R12、R15、R17、R20、R21、R25）

| 选项 | 说明 |
| --- | --- |
| **A ⭐** | **全部批准，additive 优先**：R25 / R21 补字段（`market/get` 安装快照、移除前投影）；R17 新增 `skill/create\|update\|delete`；R12 先确认「同幂等键重放」是否已支持，不支持则走加法；R20 Artifact Phase 单独立项；R15 附件 content 模型待**载体选型**拍板（2026-09-11 改判：不依赖协议词汇对齐，Q3 的「等协议下一版」已作废） |
| B | 只批准 additive（R25 / R21），其余维持 |
| C | 全部不批 |

---

## D12 · 数据源口径（R5、R8、R14）

| 子项 | 选项 | ⭐ 建议 |
| --- | --- | --- |
| **费用展示**（R14） | A 用 models.dev catalog 定价，无价则只显示 token / B 自维护定价表 / C 不做费用 | **A**（已有实现基础：`MoaSlotPrice`，需补普通 turn 的取价路径）（批 6 追加：费率与**逐轮 token** 都在时还算「本轮金额」，token 取自 App Server 对 `turn_completed` 的 additive 投影，见 `16` R14 ⑥） |
| **工具风险分级**（R8） | A 由 `config.toml [approvals]` 声明 / B 内置按类别分级 / C 不做 | **A**（与 D3=B 配套；选 C 则 D3 只能选 A） |
| **MCP 官方接入路径**（R5） | A 以现有 `.codebuddy-connector` + `mcpServers` 为官方推荐并写进文档 / B 等原生格式定义后再写 | **A**（不写就只能是我编，文档宁缺勿编） |

---

## 附 · 拍板后的连续推进顺序（无需再问）

| 批次 | 内容 | 前置 |
| --- | --- | --- |
| **批 0**（立即可做） | R3 protocol 单测 · R31 忙态文案分级 · R6 中英同步校验脚本 · R4 文档互引用与平台矩阵 · **R29 已完成** · **R4 余项（升级与迁移指引 + dist-tag 说明）✅ 已完成（2026-09-11）** · **R6 的 changelog 页 ✅ 已完成（2026-09-11，批 4）** | 无 |
| **批 1** | R25 `market/get` 安装快照 · R21 移除前投影 · R26 DB 列（D5 无关）· R27 自动更新执行（D7）· R28 本地化回退（D8） | D11、D7、D8 |
| **批 2** | R1 A3 事件面 → R5 事件参考 → R18 W14 瘦身 · R2 A4 · R3 A5 余项 | D1、D11 |
| **批 3** | R8 审批（D3）· R13 多标签（D4）· R11 W6 · R10 W4 · R12 W7 · R19 W1b · R9 W3 · R14 W9（D12） | D1、D3、D4、D12 |
| **批 4** | **R16 W11 设置：provider 部分已完成（2026-09-11）**——`config/get` / `config/set` 白名单写 `default_model`（最小改动 + 写后重读），nav 八→二，其余六分区按 §6 不渲染（判定见 `16` 卡点决策表）· **R6 的 changelog 页已完成（2026-09-11）**——`site/content/docs/{zh-CN,en-US}/changelog.md`（R6 剩余项之「changelog / release notes 页」，见下落地记录）· R17 W12 技能管理（D11）**后端写面已完成（2026-09-11）**——`skill/create|update|delete`（WS-only 宿主管理面，纯加法，无 HTTP 绑定、不进 `web/packages/client`）+ 读面增量字段 `origin` / `writable`；归属 / 可写性 / 同名不静默覆盖 / 卸载仍归 `install/uninstall` 落进 `05` §4.11 与 `16` R17 落地记录；**UI 面与 `skill/copy` 未做**（需先拍板 `skill/source` 全量正文回读与目录级复制原语）· R22 凭据（D5）· R23/R24 阻断（D6）**未排期但已批准——2026-09-11 C 档复核改判为「已批准待排期」，落地形式＝阻断默认开 + 逃生口（见上 D6 的落地口径）**· R7/R32 域名与 CDN（D2）**已按用户决定延后（2026-09-11：站点部署卡点相关问题整体延后，不再挂计划）**· **R6 余两项（站内搜索 / 市场数据刷新纳入发布流程）本轮不做；其中「站内搜索」的阻塞理由已于 2026-09-11 改判为「产品取舍」（docs 全文已内联进 bundle，不存在构建期索引守卫面），见 `16` 卡点决策表 R6 行** | D2、D5、D6、D11 |
| **批 5** | **R30 `Finish` 语义（D9=A）已完成（2026-09-11）**——后台 spawn + 取消域保留 + 3 例 manager 级 / 4 例 distill 级回归测试（`16` R30 行）· R15 W10 附件（**2026-09-11 C 档复核改判：改为「单独拍载体选型」，选型不依赖协议词汇对齐**）· R33 方向四立项（**✅ 立项页已建（2026-09-11）：`22-webui-productionization.zh.md`**，含四组拆分 + 逐条验收条目 + 假保护红线） | D9 ✅、协议词汇对齐、R33 立项页 ✅ |
| **批 6** | **R14 W9 余项「按 turn 的费用 / token」已完成（2026-09-11）**——逐轮 `usage` 由 App Server 投影 additive 带上 `message.activity`（`turn_completed`），客户端解码 + 记 `stream.turnUsage`，Composer 显示「本轮 ↑X ↓Y · $Z」（**费率与 token 都在才显示金额**）；~~边界：逐轮 usage 未持久化，重载后不显示（要回放需 DB 迁移，未做）~~ → **2026-09-11（R14 轮）已消除该边界**：迁移 `058` 给 `app_server_context_usage` 加 `last_turn_input_tokens` / `last_turn_output_tokens`（可空、未上报写 NULL），`conversation/get` 的 `context_usage` additive 带回、前端 `loadConversation` / `refreshConversation` 回填（金额仍按目录费率现算）；**仍不做**的只有「逐轮历史回放」（需另立表，不属 R14 范围）· 同批复核：R8 审批门 / R22 凭据门未动 | D12 ✅、批 3（费率已上 wire）、R14 轮（持久化） |
| **批 7** | **D-TEST-1 ② 已完成（2026-09-11，批 7）**——基线 `cargo test` 的编译门（`nomifun-db` 集成测试 `oauth_token_repository` 6 处 `UpsertOAuthTokenParams` 缺 `principal_id` / `registration_id`）：先按源码判定两列语义（注册链接 vs 预留 principal，`None` 为合法 legacy/单设备态），6 处改用最小构造器 `new(...)`，并新增 5 例真断言（legacy 两列 NULL + 查不到 / 按 registration 读回 / principal 透传 / 查询不串行 / upsert 全行写则保链-省略即清链）；`cargo check -p nomifun-db --tests` 101→**0**、`--test oauth_token_repository` **15 passed / 0 failed**、`-p nomifun-db --lib` **448 passed / 0 failed**。**同批查明并同日收口（D-TEST-1 ④）**：修好 ② 后全仓 `cargo check --tests` 曾被 `nomifun-ai-agent/tests/acp_agent_integration.rs:189` 的 `OutputDiscarded` 穷尽 match 挡住（HEAD 既有）——先做归属核验证明**无活跃 owner**（该测试文件 `git status` 未被改动、mtime 2026-08-12；`OutputDiscarded` 生产端未提交改动只落在 `manager/nomi/agent.rs`（03:34 后静止）与 `distill.rs`，`capability/backend_output_sink.rs` 完全未改；`session_turn_leases` 0 行、无其它 allo 活会话、无 `cargo.exe`），随后只在该测试文件补一行 `AgentStreamEvent::OutputDiscarded(_) => "OutputDiscarded",`：`cargo check --tests -p nomifun-ai-agent` **101→0**、四 crate 宽门 **exit 0**（2m46s）、**整仓** `cargo check --tests --workspace` **仍 exit 101**（剩 2 处既有断点在 `crates/agent/nomi-agent/tests/`，见 D-TEST-1 ⑤）、`--test acp_agent_integration` **1 passed / 0 failed / 11 ignored** · **D-TEST-1 ⑤（批 7 续 2）已收口（2026-09-11）**：`crates/agent/nomi-agent/tests/` 那 2 处既有编译错修好——`bootstrap_test.rs:393` 改挂真实字段 `output_max_tokens: Option<u32>`（`max_turns` 是轮次上限，不是 token 上限）、`autocompact_test.rs:79` mock 去掉多余 `Some`；**顺带发现并修第三条陈旧期望**（同文件 `:704`：输出上限公式已由 `window/32`（顶 4096）换成 `window/8`（顶 20k，`aac092873`），128k 实际 `Some(16_000)`，原断言 `Some(4000)` 属旧公式遗留、自 `aac092873` 起从未运行过）。**真实输出**：`cargo check --tests -p nomi-agent` 101→**0**；**整仓** `cargo check --tests --workspace` **exit 0**（2m25s，`^error` 计数 0）；`cargo test -p nomi-agent --test bootstrap_test --test autocompact_test --no-fail-fast` → **15 + 32 passed / 0 failed**；`rustfmt --edition 2021 --check` 两文件干净 · 本批**未改迁移、未改 src 语义、未动 R8 / R22** · **D-TEST-1 ③ 收口（2026-09-11，本批续 3）**：`cargo test -p nomifun-ai-agent --lib` 的 7 例运行时失败全部转绿——`manager/nomi` 3 例是**测试期望陈旧**（宿主侧 auto-continue 已被 `cbe698ff2` 删除：截断现在是引擎内的 resumable round，且只在 `LlmEvent::ToolUseTruncated` 证据下重开；图片例只是多了轮次尾部 `[Context]` 块，实测定形为 `[Context, Text(问题), Image(png)]`），`factory/nomi` 3 例是**生产回归**（chat 全 URL 分支把 `api_path` 设为 `Some("/chat/completions")`，与函数文档 / openai-responses 分支 / `openai.rs:845` 的 `format!("{base_url}{api_path}")` 拼接三处口径矛盾 → 全 URL 会被重复拼路径；改回 `Some(String::new())` 一行，快照与 2 个单测未改即绿），`factory/acp` 1 例是**环境相关**（旧断言只列举 `/npx` 与 `/npx.cmd`，本机 shim 是 `.exe`；改为剥掉 `.cmd`/`.exe`/`.ps1`/`.bat` 后等值断言 `npx`，测试不含任何本机私有路径）。**真实输出**：主门 `cargo test -p nomifun-ai-agent --lib --no-fail-fast` 修前 **992 passed / 7 failed（exit 101）** → 修后 **999 passed / 0 failed / 3 ignored（exit 0）**；回归门 `cargo check --tests --workspace` **exit 0**；旁证 `cargo test -p nomifun-ai-agent --tests --no-fail-fast` **exit 0**（14 个 target 全 ok）。逐例证据见 `16` 卡点决策表 D-TEST-1 行 ③ | 无卡点（D-TEST-1 ②③④⑤ 全部收敛；整仓 `cargo check --tests --workspace` 编译门绿，`nomifun-ai-agent` 的 lib 运行时门 999 passed / 0 failed / exit 0） |
| **批 8** | **R14 逐轮 usage 持久化已完成（2026-09-11，R14 轮，`21` D13 B 档 ③ 收口）**——**新迁移 `058_app_server_context_usage_last_turn.sql`** 给 `app_server_context_usage` 加 `last_turn_input_tokens` / `last_turn_output_tokens`（INTEGER、可空、无默认值、`CHECK (>= 0)`；**未改 001–057**）→ 写入点 `stream_relay.rs` 的 `persist_app_server_context_usage(metrics)` 取 `TurnCompleted` 真值（**两侧皆 0 → NULL，绝不写 0**，也不拿上下文占用顶替）→ 仓储两处 + `ContextUsageView` additive 两字段（`skip_serializing_if` 缺席即不上 wire）→ 前端 `persistedTurnUsage` + `restoreTurnUsage` 在 `loadConversation` / `refreshConversation` 回填（金额仍走批 6 的 `turnCostUsd`，费率与 token 都在才显示）。**本项未新增 WS 方法**（协议计数不变）。真实读数：`nomifun-db --test app_server_context_usage_repository` **5 passed / 0 failed**、`nomifun-app-server --lib` **96 passed / 0 failed**、`nomifun-extension --lib` **444 passed / 0 failed / 5 ignored**、`cargo check --tests --workspace` **exit 0**、`web test` **362 passed / 1 skipped**、`typecheck` / `build` exit 0、client（`http-transport` + `docs-drift`）**19 passed** · **同轮发现既有基线红（非本项）**：`cargo test -p nomifun-db` 整包在 `tests/id_schema_contract.rs` 有 2 例失败（表数硬编码 110 实际 **114**；`oauth_tokens.registration_id` 期望 TEXT 实际 **INTEGER**，出自 `052`），把 `058` 移出迁移目录重跑**同样失败**→ 已证伪归属、**未修**（需改既有迁移或放宽 ID 契约，超出本项边界，交归属方）；**第三例**：`tests/provider_platform_migration.rs:61`（迁移 `030` 回填期望陈旧，用例只应用到 `030`、`058` 不在路径上）。**整包口径**：lib **448 / 0**；**30 个集成目标全绿**；**2 个既有红目标**（3 例） | D13 B 档 ③ ✅、R14 收口 |

> **批 7 落地记录（2026-09-11 · `16` R34 / D-TEST-1 ②）**：**基线编译门收敛**。修前 `cargo check -p nomifun-db --tests` → **exit 101 / 6× E0063**（`missing fields \`principal_id\` and \`registration_id\``，全部落在 `crates/backend/nomifun-db/tests/oauth_token_repository.rs` 的 19/62/86/133/155/192 行），即 `cargo test` 在该基线上**根本无法编译**。修法不是机械补 `None`：先按源码与迁移判定语义——`registration_id` 是「铸造该 token 的 `oauth_client_registrations.id`」的**逻辑链接**（v3 schema 禁物理外键；迁移 `052` 注释：legacy 行留 NULL，无注册可解析时按 `requires_reauthorization` 处理），`principal_id` 是「为未来多用户预留、不是当前单设备模式下的 `users` 行」（`id_schema_contract.rs:298-301` 登记为非引用 id 列），生产写点只有 `oauth_service.rs:1172`（refresh：原样带回 `row.registration_id` / `row.principal_id`）与 `:1208`（`persist_token`：`registration_id` 来自授权流程、`principal_id: None`）——故 6 处 URL-keyed 用例用最小构造器 `UpsertOAuthTokenParams::new(...)` 正确（两列 `None` 正是文档口径的合法态），并**新增 5 例真断言**把语义钉住（legacy 行两列 NULL 且 `get_by_registration` 查不到 / 带注册的行按 registration 读回同一行、其它 id → `None` / `principal_id` 值透传 / 注册查询不串行 / upsert 为 `ON CONFLICT(server_url) DO UPDATE ... = excluded.*` 的**全行写**：refresh 式带上则保链、省略即清链，`created_at` 保留）。**真实输出**：`cargo check -p nomifun-db --tests` → **exit 0**（`Finished \`dev\` profile ... in 4.15s`）；`cargo test -p nomifun-db --test oauth_token_repository` → **15 passed / 0 failed / 0 ignored**（10 原有 + 5 新增）；`cargo test -p nomifun-db --lib` → **448 passed / 0 failed / 0 ignored**；`rustfmt --edition 2021 --check <该文件>` → 干净。**边界与未做**：未改迁移文件、未改任何 `src`、未动 R8 审批门 / R22 凭据门；`principal_id` 仍无生产写点（预留），本项只保证仓储透传；全仓 `cargo check --tests` 当时仍不绿——`crates/backend/nomifun-ai-agent/tests/acp_agent_integration.rs:189` 的 `event_type_name()` 缺 `AgentStreamEvent::OutputDiscarded(_)` 分支（**HEAD 既有**），已登记为 D-TEST-1 ④，**已于同日收口（见下条落地记录）**。**② 的改动文件**：`crates/backend/nomifun-db/tests/oauth_token_repository.rs`；**未 commit / 未 push**。

> **批 7 落地记录（续 · D-TEST-1 ④ 收口，2026-09-11 · `16` R34 行 ⑤ / D-TEST-1 ④）**：**`nomifun-ai-agent` 的测试编译断点已收敛（`cargo check --tests --workspace` 整仓仍未绿，剩 2 处既有断点在 `crates/agent/nomi-agent/tests/`，见下 ⑤）**。① **归属核验先于动手**（证明无活跃 owner，全程未用 `git stash`）：a) 该测试文件 `git status --porcelain` 为空、mtime 停在 **2026-08-12**（未被任何线改动）；b) `OutputDiscarded` 生产端未提交改动只落在 `manager/nomi/agent.rs`（mtime **03:34**，实测时已静止约 4 小时）与 `distill.rs`（属批 5 的 R30），`capability/backend_output_sink.rs` **完全未改**（mtime 2026-08-25）；c) `state.db` 的 `session_turn_leases` **0 行**、`sessions` 中除本会话外无 `C:\workspace\allo` 活会话、进程表无 `cargo.exe`。② **最小改动**：只在该测试文件补一行 `AgentStreamEvent::OutputDiscarded(_) => "OutputDiscarded",`（**未改 `src/`、未改迁移、未碰 `057_*`、未动 R8 / R22**）。③ **真实输出**：`cargo check --tests -p nomifun-ai-agent` 修前 **exit 101 / 1× E0004**（`error[E0004]: non-exhaustive patterns: &AgentStreamEvent::OutputDiscarded(_) not covered`，`acp_agent_integration.rs:189`）→ 修后 **exit 0**（`Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 7.92s`）；四 crate 宽门 `cargo check -p nomifun-ai-agent -p nomifun-db -p nomifun-app-server -p nomifun-app --tests` → **exit 0**（2m46s，仅既有 warning）；**整仓** `cargo check --tests --workspace` → **仍 exit 101**（**不是** exit 0：剩下 2 处**既有**编译错全在 `crates/agent/nomi-agent/tests/`——`bootstrap_test.rs:393` 读已不存在的 `Config::max_tokens`（E0609）、`autocompact_test.rs:79` 把 `Option<u32>` 再包 `Some`（E0308）；该树在工作区未改动 ⇒ HEAD 既有，不在 Agent Store 线，已登记为 **D-TEST-1 ⑤**，本次未修）；`cargo test -p nomifun-ai-agent --test acp_agent_integration` → **1 passed / 0 failed / 11 ignored**（11 例按文件头注明 `requires JSON-RPC mock agent` 跳过）；`rustfmt --edition 2021 --check <该文件>` → 干净（**未**跑 `cargo fmt -p`，以免替并行线格式化 `src/*.rs`）。④ **边界**：只补显示名，不改 `event_type_name` 用途与 `OutputDiscarded` 语义；D-TEST-1 ③ 的 7 例运行时失败与本次无关，仍开放。⑤ **同时订正一条过强的旧口径**：批 7 前文写的「全仓 `cargo check --tests` 只剩 ④ 一处断点」不成立——那是 scoped 命令（`-p nomifun-mcp -p nomifun-ai-agent`）的结果，`--workspace` 下还有 `nomi-agent` 那 2 处；两处文档已同步更新。

> **批 7 落地记录（续 2 · D-TEST-1 ⑤ 收口，2026-09-11）**：**整仓测试编译门转绿**（`cargo check --tests --workspace` **exit 0**）。**语义先于代码**：输出上限的真实字段是 `Config::output_max_tokens: Option<u32>`（`max_turns` 是轮次上限——不采纳编译器的 `max_turns` 建议），请求侧 `LlmRequest::max_tokens` 同样是 `Option<u32>`（`None` ＝ 交给 provider 默认）；两处都按真实类型断言，没有 `assert!(true)` / `let _ =` / 删用例。**改动面（只碰测试文件）**：`crates/agent/nomi-agent/tests/bootstrap_test.rs`（1 处断言 + 注释）、`crates/agent/nomi-agent/tests/autocompact_test.rs`（mock 记录点 + 字段注释 + 1 处陈旧期望）。**第三条陈旧期望（本轮顺带发现）**：`summary_output_cap_follows_context_window` 断言摘要请求的 `max_tokens == Some(4000)`，那是旧公式 `compact_max_output_tokens = (window/32).max(512).min(4096)` 的值（`git show aac092873^:crates/agent/nomi-agent/src/compact/prompt.rs`）；`aac092873` 换成 `window_output_unit = (window/8).min(20_000)` 后 128k 实际为 `Some(16_000)`，而该文件自那次改动起就编不过，所以这条断言从未被运行过。改法是**保留等值断言的强度**、把期望换成当前真实值并在注释里写明公式归属（公式自身由 `nomi-config` 的单测守）。**真实输出**：修前 `cargo check --tests -p nomi-agent` → **exit 101**（E0609 @ `bootstrap_test.rs:393`、E0308 @ `autocompact_test.rs:79`）→ 修后 **exit 0**（`Finished \`dev\` profile ... in 1.65s`）；**主门** `cargo check --tests --workspace` → **exit 0**（`Finished ... in 2m 25s`，149s）；`cargo test -p nomi-agent --test bootstrap_test --test autocompact_test --no-fail-fast` → **15 passed / 0 failed**（bootstrap_test，0.07s）＋ **32 passed / 0 failed**（autocompact_test，0.02s），exit 0；`rustfmt --edition 2021 --check <两文件>` → 干净；旁证 `cargo test -p nomi-agent --lib --no-fail-fast` → **747 passed / 0 failed**（36s，exit 0）——该 crate 的 lib 门与这两个集成测试门同时绿。**教训**：a) `cargo test` 在**第一个失败的 test target 就停**——多 target 必须加 `--no-fail-fast`，否则后面的二进制根本不跑（本轮首跑只见 `autocompact_test` 红、`bootstrap_test` 未被执行）；b) 类型演进由编译器兜住，**公式 / 常量演进不会**——它要等编译门修绿后才第一次被运行暴露，所以「编译绿」不等于「测试绿」。**边界**：未改任何 `src/`、未碰迁移与 `057_*`、未动 R8 / R22、未 commit / push；③ 的 7 例运行时失败（`nomifun-ai-agent --lib`）不在本项范围，仍未修。

> **批 7 落地记录（续 3 · D-TEST-1 ③ 收口，2026-09-11）**：**`nomifun-ai-agent` 的 lib 运行时门转绿**（`cargo test -p nomifun-ai-agent --lib --no-fail-fast`：修前 **exit 101 / 992 passed / 7 failed / 3 ignored**（90.53s）→ 修后 **exit 0 / 999 passed / 0 failed / 3 ignored**（161s 含编译、用例段 99.69s），7 例逐条 `... ok`）。**归属核验先于动手**：`factory/nomi.rs` 与 `factory/acp.rs` 改动前与 HEAD **逐字节相同**（`git diff --quiet` exit 0 ⇒ 失败在 HEAD 基线即存在）；三例 `manager/nomi` 用例的测试体改动前与 HEAD 逐字相同（`git blame`：`27f620d5eb` 2026-07-12 / `8adfe861e6` 2026-07-16 / `52e05c19f` 2026-07-07），工作区对 `agent.rs` 的未提交改动只有 R30 段与追加的 R30 测试块（`@@ -4301,6 +4311,240`），不在这三例上；`session_turn_leases` **0 行**、`sessions` 里最近一条 `C:\workspace\allo` 会话停在 2026-08-27、进程表只有本轮自己的 `cargo.exe`/`rustc.exe`。**逐例判定与修法**：① *`manager/nomi` 3 例 = 期望陈旧* —— 根因是 `cbe698ff2`（2026-08-21「make an output-ceiling truncation a resumable round」）删掉宿主侧 auto-continue（`crates/agent/nomi-agent/src/round.rs:42-44` 明文写着被删的 `MAX_TRUNCATION_AUTO_CONTINUES = 2` 与「截断草稿不可续写、只能带 ledger 重试原始需求」）；被断言的 4 句宿主提示在 HEAD 上**除该用例自身外全仓无引用**（`git grep … HEAD -- crates/` 只命中测试 4 行）；图片例的 provider 载荷实测 `[Text("[Context]\nCurrent date: …"), Text("What is shown?"), Image(image/png, 212 bytes)]`（附件一直到达 provider，只是轮次尾部 `[Context]` 占首块，旧断言按「恰好两块」定形匹配）。修法：图片例改「问题逐字 **且** 恰好一个 `Image(png, 非空)` 块」；另两例改用 `LlmEvent::ToolUseTruncated{…} + Done{MaxTokens}`（并注册 `Write` 使其真被 advertise）→ 引擎在同一轮内重开，**原强度保留**（`provider.calls() == 3`、`requests.len() == 2`、`OutputDiscarded.restart_attempt == vec![2]`、`Start` 恰 1 次）+ 新增反向断言 `!contains("continue where you left off")` 与替代文案断言（`[resumable round 2/3]` / `WHAT WAS CUT OFF:` / `Write (65536 bytes of arguments streamed, NOT executed)` / `Split any large file: write a small complete version first, then edit or append.`）；`max_tokens_…` 按机制改名为 `max_tokens_truncated_write_restarts_without_repeating_the_large_write`。② *`factory/nomi` 3 例 = 生产回归（改 `src/` 一行）* —— chat 全 URL 分支的 `api_path` 与函数文档、openai-responses 分支（`Some(String::new())`）、消费端 `openai.rs:845` 的 `format!("{base_url}{api_path}")` 三处矛盾（全 URL 会变成 `…/v1/chat/completions/chat/completions`），`dispatch_target.rs:11,79` 亦定义 `is_full_url` ＝「base_url is already the complete endpoint」；该写法由 `a833120a9`（2026-07-07）引入（`git log -S` 唯一命中），而快照用例来自另一条并行谱系（`501899b87` 2026-07-29 及其合流点 `b35663c78` 上该分支都是 `Some(String::new())`），汇入 HEAD 时取了 `/chat/completions` 一侧 → 快照自合流起就与实现不一致；改回 `Some(String::new())` 后 2 个单测 + 220 行快照**未改一字**即绿。③ *`factory/acp` 1 例 = 环境相关（改测试为与主机无关）* —— 旧断言只列举 `/npx` 与 `/npx.cmd`，本机 `resolve_command_path` 经 PATH 命中带 `.exe` 的 shim；改为 basename 剥掉 `.cmd`/`.exe`/`.ps1`/`.bat` 后等值断言 `npx`。**旁证**：`cargo test -p nomifun-ai-agent --tests --no-fail-fast` → **exit 0**（186s，14 个 test target 全 `test result: ok`，`factory_provider_integration` 7 passed 覆盖 `factory/nomi.rs` 改动的集成面）；`rustfmt --edition 2021 --check` 三个改动文件干净。**③ 改动文件**：`crates/backend/nomifun-ai-agent/src/factory/nomi.rs`（1 行语义 + 理由注释）、`src/factory/acp.rs`（测试断言）、`src/manager/nomi/agent.rs`（3 例测试，含 1 例改名）。**边界与未做**：未碰迁移与 `057_*`、未改 `crates/agent/**/src/`、R8 审批门 / R22 凭据门未动、未 commit / push；整仓 `cargo test` 全量与 `crates/agent/nomi-agent` 的 lib 门本轮未重跑（③ 不触及该 crate 的 `src/`，其编译面由回归门覆盖）。

> **Q6 落地记录（2026-09-11，`16` R16）**：provider 写入目标按 ① 落地——新增 WS 方法 `config/get` / `config/set`（`05` §4.10），白名单只允许 `default_model`，写入用 `toml_edit` 做最小改动（注释 / 排版 / 其余键逐字节保留）、缺键插在文件头注释之后且绝不落进 `[table]`、同目录临时文件 + `rename` 原子替换，响应为**写后重读**；`~/.agent-store/config.toml` 由此成为 provider 默认模型的**唯一可写面**（设置里两个从不回写的本地输入框已删）。凭据方向未变：`api_key` / `base_url` / 路径不在读视图也不在白名单，出现即 `invalid_request`；R22 凭据门（D5）**本轮未动**，仍等宿主侧凭据存储 + 注入 API。

> **D9=A 落地记录（2026-09-11，批 5 · `16` R30）**：`crates/backend/nomifun-ai-agent/src/manager/nomi/agent.rs` 收尾段删除「`await` 蒸馏 child 再判取消」的写法，改为 `distill::spawn_distill_exact_turn(turn_cancel.clone(), cfg, dir, transcript)`，随后仍以 `turn_cancel.is_cancelled()` 决定是否走 `TurnStopReason::Cancelled` 分支（`cancel_active_tool_calls` + `fence_cancelled_processes` + `terminalize` 全部原样）。`distill.rs` 里新增 `spawn_exact_turn_child`（`tokio::spawn` + 同一 token 的 `await_exact_turn_child`，取消即丢 future）替代原 `run_distill_exact_turn`；provider 失败日志由 `debug` 提为 `warn`（仍是既有 tracing 出口，**没有**新事件类型 / 新 wire 方法 / SDK 改动）。测试：manager 级 3 例（不等待 / 取消不建 child 且仍报 `Cancelled` / 蒸馏失败不把回合变成错误）+ distill 级 4 例（spawn 不阻塞调用方 / 取消后到不了 apply / 已取消的轮次不建 child / real provider 失败正常返回且不写盘），另保留原 `await_exact_turn_child` 取消安全用例。


> **R4 余项落地记录（2026-09-11，`16` R4 · 口径 D10=A）**：站点新增 `upgrade` 页（`site/content/docs/{zh-CN,en-US}/upgrade.md`，中英各 172 行、结构对称；`DOC_ORDER` 与两语言 `docs.sections.upgrade` 同步），把 D10=A 的三条口径落成可操作内容：**beta 期不承诺向后兼容**、**破坏性变更走 minor + changelog 明示**、**逐版本升级步骤与固定版本建议**；并给出**发布事实表**（`0.1.0` = 2026-09-09T09:09:04Z / 无 tag，`0.1.0-beta.2` = 09:27:36Z / `latest`，`0.1.0-beta.3` = 2026-09-10T04:44:34Z / `beta`）与自查命令。口径边界写死：**独立 changelog / release notes 页尚未建设**（`16` **R6** 剩余项，本轮**有意未做**；**已于 2026-09-11 批 4 兑现，见下「R6 落地记录」**），本文只登记已发布事实与已核实的未发布差异（`event_type` 收窄尚未随任何版本发布），**不发明版本历史**。验证：`check-docs-sync` **8 页 × 2 语言 / 0 drift** + `--self-test` 9/9、site `tsc --noEmit` 0 错、`react-router build` 通过。**协议与 SDK 源码未动**（无新 wire 方法、未改 `web/packages/*/package.json` 版本号、未发布任何 npm 包）。

> **R6 落地记录（2026-09-11，批 4 · `16` R6 · changelog / release notes 页）**：新增站点 `changelog` 页（`site/content/docs/{zh-CN,en-US}/changelog.md`，slug ＝ `changelog`），把 `upgrade` 页里那句「独立 changelog / release notes 页尚未建设，见 `16` R6」**兑现成页**。**三处同步**：`site/app/lib/docs.ts` 的 `DOC_ORDER`（插在 `upgrade` 之后）+ `DocSections.changelog`、两语言 `site/app/i18n/*.ts` 的 `docs.sections.changelog`（**9 项且顺序一致**）；`docs/:slug` 是动态路由，侧栏与文档索引都由 `DOC_ORDER` 驱动，渲染层无需改动。**内容口径**：只写有据可查的事实——发布序列用本机实测（`npm view @flowy-agent-store/sdk versions dist-tags time --json` → `versions` ＝ `0.1.0-beta.2` / `0.1.0-beta.3` / `0.1.0`，`dist-tags` ＝ `{beta: 0.1.0-beta.3, latest: 0.1.0-beta.2}`，`time` ＝ `0.1.0` 2026-09-09T09:09:04.795Z / `beta.2` 09:27:36.122Z / `beta.3` 2026-09-10T04:44:34.609Z，与 `upgrade` §2/§3 **完全一致**）；每版「改了什么」**只引** `16` R4 行的 tarball 逐字节比对结论与上一条落地记录，**不编条目**；D10=A 落成**格式约定**（本页是**破坏性变更的唯一公告面** / 走 minor 号 / 条目发布后才追加、版本号与发布时间**永不改写** / 写错追加「更正（日期）」/ **dist-tag 移动不产生条目**）；**未发布项**（`event_type` 收窄、wire 方法增量、release 重建与 `beta.4` 挂起）单列一节，**不写成「即将发布」**。`upgrade` 页两语言 §1 两处要点、§9 表格行（并新增「本页与 changelog 的分工」行）、§10 另见同步改为指向新页的**相对内链**。**验证（真实输出）**：`check-docs-sync.mjs` → **9 page(s) in 2 language(s), 0 drift(s)**（新增页后 8 → 9）、`--self-test` → **9/9 as expected**、`bun test scripts/check-docs-sync.test.mjs` → **16 pass / 0 fail**（本轮顺带修掉上一轮漏改的**页码清单**——该用例在 HEAD 上本来就 1 fail，清单停在 7 页）、新页 4 类突变反向测试**均被拒**、site `tsc --noEmit` **0 错**、`react-router build` **通过**并预渲染两语言的 `changelog` 页。**本轮未做（有意）**：站内搜索、市场数据刷新纳入发布流程——理由与解锁条件见 `16` 卡点决策表 R6 行。**协议与 SDK 源码未动**（无新 wire 方法、未改 `web/packages/*/package.json` 版本号、未发布或修改任何 npm 包、未 commit / push）。