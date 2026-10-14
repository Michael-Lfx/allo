# 类别信息全面透出（pack_format 3 · Team/Skill 面）· 技术方案

> **状态**：✅ 已实施（编号 `44`，`43` 的续篇；基线 `fp-15` / `56 / 83` / `pack_format 2` → 实施后 **`fp-16` / `56 / 83` / `pack_format 3`**）
> **核心原则**：分类与标签是**源声明的展示元数据**——只透传、不派生；id 与解析结果分开给（id 永远诚实，解析缺席就是缺席）；pack 的版本与协议指纹**各自独立**演进。

---

## §1 背景与缺口清单

`43` 把分类与标签透出到了商店目录面，但四个面仍有缺口：

| 面 | 现状 | 消费方 | 缺失后果 |
|---|---|---|---|
| `store/list` + `store/categories` | ✅ 完整（fp-15） | WebUI / ZCode | — |
| `agent/list` / `agent/get` | ⚠️ tags + 裸 `category_id` | UI、外部集成 | 解析名要二次 join |
| `team/list` / `team/get` | ❌ 无任何分类字段 | UI | 团无法参与分类筛选 |
| `skill/list` / `skill/get` | ❌ 无 tags | UI | 已安装技能列表无标签 |
| `agent/export` / `team/export`（ExpertPack） | ❌ 无 | **第三方 runtime** | 导出物无法还原市场的组织结构——可移植契约不完整 |

优先级（本轮全做）：**导出面 > Team > Skill**——pack 是给外部 runtime 的契约，缺分类是结构性缺失；Team/Skill 面是自家 UI 的目录页。

## §2 方案

### 2.1 数据源与不变量

- **真源不变**：仍是 `plugin.json` 的 `tags`（`Vec<LocalizedText>`）与 `category_id`。`43` 已确认 importer 把两者写入 **agent** 组件 payload（`attach_plugin_display`）。
- **Team 的分类来自包级**：WorkBuddy 的 plugin.json 是**包级**声明——一个包要么是团（`teamInfo`），要么是专家。`category_id` / `tags` 写进 team 组件 payload 后即团队自身的分类。
- **Skill 的 tags 来自市场索引行**：技能（含连接器）没有 plugin.json，其 tags 的唯一来源是 marketplace.json 行内 `tags_zh/tags_en`。导入器在构建 skill/connector 组件时把索引行 tags 写入 payload。
- **宁缺毋猜**：所有新字段 `skip_serializing_if`，缺席 = 源未声明，绝不派生。

### 2.2 wire 变更（`fp-15` → `fp-16`，计数 `56 / 83 → 56 / 83` 不变）

DTO 字段增减触发指纹（`web/AGENTS.md` §5），方法数不变：

```rust
// AppServerTeamSummary 增量（detail 经 flatten 继承）：
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub tags: Vec<AppServerLocalizedText>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub category_id: Option<String>,

// AppServerSkillSummary 增量：
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub tags: Vec<AppServerLocalizedText>,
```

**`AgentSummary.category` 不加**：agent 面已有裸 `category_id`，解析走 `store/categories` 字典（一次调用全量缓存）；为它单独加 join 需要每个 agent 行重复字典查找，收益不抵协议面膨胀。

### 2.3 ExpertPack（`pack_format 2 → 3`，指纹**不动**）

```rust
// AppServerExpertPack 增量：
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub tags: Vec<AppServerLocalizedText>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub category_id: Option<String>,
```

- pack 与指纹刻意独立（`32` §4.4）：字段增量 bump `pack_format`，外部 runtime 按 format 守门，v2 消费者读 v3 包跳过未知字段照常工作。
- 成员 pack（team 的 `members[]`）**带自己的 tags/category_id**——这是 per-agent 声明，不是 snapshot 作用域资产（与 `assets` 的 D7 恰好相反：分类是成员自身属性）。

### 2.4 importer 写入

- `attach_plugin_display`（agent + team 组件共用）已写 `tags`/`category_id`——team 组件 payload **自动获得**（走同一函数），无需改。
- skill/connector 组件：importer 构建时从**市场索引行**（`MarketIndexInfo.tags`，`43` M1 已解析）读取 tags 写入 payload。入口在 store 安装链的 import 请求（`declared_version` 同路的索引信息）。

### 2.5 SDK（protocol 包）

- `TeamSummary` 增 `tags?: LocalizedText[]`、`category_id?: string | null`；
- `SkillSummary` 增 `tags?: LocalizedText[]`；
- `ExpertPack` 增同名字段 + `PACK_FORMAT = 3`。

## §3 决策矩阵

| 编号 | 决策点 | 选定方案 | 放弃的替代方案与理由 |
|---|---|---|---|
| D1 | 指纹 | `fp-15` → `fp-16`（Team/Skill DTO 字段增） | ❌ 不 bump：`web/AGENTS.md` §5 明确「DTO 字段增减」触发；pack 字段不触发（format 独立） |
| D2 | pack_format | `2` → `3` | ❌ 保持 2 塞新字段：外部 runtime 按 format 守门，静默改形状违反其存在意义 |
| D3 | 成员 pack 是否带 tags | **带**（per-agent 属性） | ❌ 仿 `assets` 做 snapshot 作用域：分类是成员自身声明而非包级资源，去重语义不成立 |
| D4 | Agent 面加 `category` 解析形态 | 不加，走字典 join | ❌ 每行重复字典查找；UI 已可一次 `store.categories()` 缓存后 O(1) join |
| D5 | Skill 面是否也带 `category_id` | 不带（技能市场没有分类**字典**，只有扁平 tags） | ❌ 无中生有：字典不存在，加了只会永远是 `None`。**2026-10-14 修正**：真实归档里 268 行有 **1 行**带扁平的 `category_zh/category_en`（`Excalidraw Diagram`），所以准确说法是「没有字典，但个别行可能有扁平分类标签」；仍不读该字段，理由见 §6.3 |

## §4 验收

| 编号 | 断言 |
|---|---|
| TC-CAT-009 | 带分类的团条目安装后 `team/list` 含 `tags`/`category_id`（值 = plugin.json 原样） |
| TC-CAT-010 | 市场索引声明 tags 的技能安装后 `skill/list` 含 `tags`（双语配对值逐字一致） |
| TC-CAT-011 | `agent/export` 的 pack 为 `pack_format: 3` 且带 `tags`/`category_id`；成员 pack 各带自己的值 |
| TC-CAT-012 | v2 消费者读 v3 pack（未知字段跳过）功能不受损 |
| TC-CAT-013 | 无分类源：所有新字段缺席（不出现 `null`/空串占位） |

验证梯子：`cargo test -p nomifun-importer` → `cargo test -p nomifun-app` → `cargo test -p nomifun-app-server` → `cd web && bun run typecheck && bun run test` → `check:fingerprint`（fp-16）+ `check:release-sync`（56/83 不变）。

---

## §5 实施记录（2026-10-13）

### 5.1 落地形状与一处设计收敛

计划里 skill tags 的入口写的是「store 安装链的 import 请求」；实施时收敛为**显式参数**，因为 skill 组件 payload 是在导入时固化的，投影期再回查市场会引入「同一个已安装技能，市场改了索引行它就变」的漂移：

- `ImportRequest` 增 `index_tags: Option<Vec<LocalizedText>>`（+ `with_index_tags` 链式 setter），由 `AppServerMarketplaceProvider::import_entry` 从 `EntryFacts.index_info.tags`（`43` M1 已解析的按位配对结果）填入；
- `ComponentBuilder` 增 `index_tags` 字段 + `with_index_tags` + `index_tags_payload()`，**只在 skill / single-skill 两个 builder 里**写进组件 payload（plugin 导入忽略它——`plugin.json` 的 tags 更权威）；
- 读取侧：`skill/list` / `skill/get` 经 `AppServerEntryAssets::tags_for_skill(snapshot_id, slug)` 走与头像同一条 provenance 链（运行时目录 → `agent-store/<snapshot_id>/<slug>` → 快照组件 payload），**不碰市场活树**。

> 「导入时固化」是刻意的：快照本身不可变，所以已完成安装的技能，其标签与当时的市场声明一致；市场后来改标签要走一次升级（`store/update-entry`）才生效——与技能内容本身的语义完全一致。

### 5.2 team 面零改动即生效

`team/list` / `team/get` 只是把已有事实读出来：`attach_plugin_display`（agent 与 team 组件共用）在导入时就把 `tags`/`category_id` 写进了 team 组件 payload，`team_summary()` 原先只读了 `lead_agent_id`/`member_agent_ids`。因此 Team 面**只加了两行投影**，不涉及导入器改动。

### 5.3 改动清单

| 层 | 文件 | 改动 |
|---|---|---|
| DTO | `nomifun-api-types/src/app_server.rs` | `TeamSummary` 增 `tags`/`category_id`；`SkillSummary` 增 `tags`；`ExpertPack` 增 `tags`/`category_id`；`APP_SERVER_EXPERT_PACK_FORMAT 2 → 3` |
| 导入器 | `nomifun-importer/src/import.rs` | `ImportRequest.index_tags` + `with_index_tags`；`ComponentBuilder.index_tags` + payload 注入（skill market / single skill 两处） |
| 导入器夹具 | `nomifun-importer/tests/importer_tests.rs` | 26 处 `ImportRequest` 字面量补 `index_tags: None` |
| 组合根 | `nomifun-app/src/app_server_marketplace.rs` | 安装链传 `facts.index_info.tags` |
| 组合根 | `nomifun-app/src/app_server_store.rs` | `MarketIndexInfo.tags` 提升为 `pub(crate)` |
| 组合根 | `nomifun-app/src/app_server_entry_assets.rs` | 新增 `tags_for_skill`（provenance 链 + payload 解析） |
| 组合根 | `nomifun-app/src/app_server_catalog.rs` | `skill_summary` 增 tags 参数；`SkillCatalog::list`/`get` 解析并透传 |
| 组合根 | `nomifun-app/src/app_server_importer.rs` | `team_summary` 读 `tags`/`category_id`；`localized_list` 提升为 `pub(crate)`；手动导入链补 `index_tags: None` |
| 组合根 | `nomifun-app/src/app_server_expert_export.rs` | agent pack 与 team pack 两处字面量填 `tags`/`category_id` |
| 指纹 | `nomifun-app-server/src/lib.rs` | `PROTOCOL_VERSION = "fp-16"` + 变更说明段 |
| SDK | `web/packages/protocol/src/protocol.ts` | `SkillSummary.tags`、`TeamSummary.tags`/`category_id`、`ExpertPack.tags`/`category_id`、`APP_SERVER_EXPERT_PACK_FORMAT = 3`、指纹注释补 `fp-16` 段 |
| SDK | `web/packages/sdk/src/export.ts` | 模块文档补 `pack_format 3` 的展示元数据说明（`expert-pack.json` 原样携带，代码路径不变） |
| SDK 夹具 | `web/packages/sdk/src/export.test.ts`、`web/scripts/sdk-live-expert-export.ts` | `pack_format` 期望值 `2 → 3` |
| 门禁夹具 | `nomifun-app-server/src/{lib,catalog,team_run}.rs` | 测试字面量补新字段 |
| 站点 | `agent-store-site/content/docs/{zh-CN,en-US}/typescript-sdk.md` | 指纹 `fp-16`（方法计数仍 `56 / 83`，未变） |

### 5.4 验证读数

| 门禁 | 结果 |
|---|---|
| `check:fingerprint` | ✅ `fp-16`（本仓 7 文件 10 处 + 站点 2 处） |
| `check:release-sync` | ✅ `56 / 83` 两语言同值（方法数未变，仅指纹变） |
| `cargo test -p nomifun-importer` | 74 passed（48 单测 + 26 集成，含新增 `tc_cat_010`） |
| `cargo test -p nomifun-app-server` | 191 passed（含 pack_format 3 形状守卫） |
| `cargo test -p nomifun-app --lib` | 409 passed / 1 failed（唯一失败为既有基线 `every_production_conversation_service…`，已用 stash 在干净 main 上复现确认；本轮新增 2 个用例：team 投影、pack 展示元数据） |
| `cd web && bun run typecheck && bun run test` | 564 passed / 1 skipped |

### 5.5 仍未做（明确边界）

- **`skill/get` 的 tags 依赖 `assets` provider 被接线**：未接线 `AppServerEntryAssets` 的宿主（测试替身、极简宿主）技能面 tags 恒空——与头像字段的既有行为一致，不是新缺口；
- **`store/categories` 与 pack 的解析**：pack 只带 `category_id`，外部 runtime 若要显示分类名，需要自己持有等价字典（本仓的字典在 `store/categories`，跨宿主不通用）；
- **连接器面未加 tags**：`connector/list` 的消费方（抽屉开关）目前不展示标签，且**连接器市场索引里根本没有标签数据**——实测 `connectors.json` 228 行的键集（`id`/`name`/`name_en`/`description*`/`source`/`auth_mode`/`type`/`examples_*`/`visible_in`/`version`/…）**无一 tag 字段**，连接器也没有 `plugin.json`。所以这不是「留待有需求时再加」，而是**没有数据源**。（本轮修正：原措辞写了「数据侧已具备」，与实测不符。若将来市场补上，`read_index` 已经在读 `tags_zh`/`tags_en`，无需改代码。）

---

## §6 字典分发：核实、兜底与随归档发布（2026-10-14）

### 6.1 结论先行

`43`/`44` 把「分类字典」当作随手可读的事实，但它**没有随市场归档分发**——专家市场归档里根本没有那个文件。本轮把两件事一起做掉：宿主侧立刻可用（内置兜底词表），站点侧治本（字典随归档发布）。

### 6.2 核实方法与结论

不下载 289 MB 归档，而是用 HTTP Range 只取 zip 的**中央目录**与需要的条目（合计约 2.4 MB），逐条核对：

| 问题 | 结论 | 证据 |
|---|---|---|
| `experts.zip` 根有什么？ | 只有 `.codebuddy-plugin/` 与 `plugins/` | 14,713 条中央目录，根级文件 **0** 个 |
| 市场清单带分类吗？ | 不带 | `.codebuddy-plugin/marketplace.json` 顶层键 `name`/`description`/`plugins`，381 行每行 `name`/`source`/`description` |
| 条目引用分类吗？ | 引用 | `plugins/stock-partner-team/.codebuddy-plugin/plugin.json` → `categoryId: "08-FinanceInvestment"` + `tags[]` |
| 字典在哪？ | WorkBuddy **应用侧**目录索引 | `~/.workbuddy/app/cache/experts/manifest.json`(1.2 MB)：`categories[15]` + `experts[448]` |
| 词表可信吗？ | 是闭集，且与归档同源 | 448 行**无一**引用未声明的 id；377 个插件名能对上归档的 381 个 |
| 技能市场那一半呢？ | **随归档分发，且完整** | `skills.zip` 根有 `.codebuddy-skill/marketplace.json`：顶层 `tags_zh/en` 各 11 项，268 行全部带行内 tags，154 行带 legacy |

根因是**两个子系统各持一份文件**：市场树（`~/.workbuddy/plugins/marketplaces/experts`，我们镜像的）与应用目录索引（`~/.workbuddy/app/cache/experts/manifest.json`，字典在这）从不交叉。

### 6.3 三处处理

**① 宿主内置兜底**（`app_server_categories_builtin.json` + `app_server_categories.rs`）

- 15 行**逐字**取自上游索引，带 `_source`/`_source_file`/`_source_lastUpdated`/`_source_sha256` 溯源，不做任何人工改写；
- **只在一个市场都没声明字典时启用**；有市场声明则以市场为准，且**不合并**（两套词表混用会产出同名不同义的 id）；
- 退化诚实：新 id 出现时查不到就显示裸 id，绝不从 `NN-Name` 文本里猜名字；
- `skill` 行里的扁平 `category_zh/category_en`（268 行里的 1 行）**不读**：它既不是 id 引用也没有字典，为 1/268 加字段属于投机（`44` 原 D5 据此修正措辞，而不是加功能）。

**② 缓存按文件身份**（`43` §3.3 的措辞现已与实现一致）

按 `(路径, mtime, 长度)` memoize 解析结果：市场扫掠是**原地**替换 live 根里的文件，只按路径做键会永久供应过期字典。不落盘、不写库。

**③ 字典随归档发布：决定不做**（2026-10-14 拍板）

站点仓是三个市场的生产者（`market-source/` 镜像 → `pack:market` → ModelScope 归档），因此「让归档带上字典」在技术上可行，做法也做过并验证通过：

- `sync-market-tree.mjs` 引入**生成物**（`GENERATED`）：从上游应用侧索引派生市场根 `manifest.json`（仅 `categories[]` + 溯源，约 6 KB），写进镜像 → 进 `_files.txt` → 进归档；`prune` 不删它、上游索引缺席时保留已提交那份、生成物不走上游拷贝路径；
- 实测：`_files.txt` 14,713 → 14,714，`--only experts --check` 重打包 `files=14714`、两次摘要一致（确定性成立），新归档 sha256 `5d2827ab…`（旧 `5cd0ab94…`）。

**最终决定：不发布。** 兜底词表已经把这件事做完了（15 行是闭集、随二进制分发、无下载成本），而发布归档要付：一次 289.6 MiB 的重打包与上传、全量客户端重新下载、`content/market-hosts.json` 摘要变更、以及给市场仓永久增加一个「非上游文件」。**站点侧该改动已撤回**（备份留在实现记录里，需要时可原样还原），站点仓因此只剩市场数据的日常同步。

代价与边界（如实登记）：

- 上游若新增分类 id，内置词表不认识它 → 该条目的 `category` 缺席、退回显示裸 id（`08-FinanceInvestment` 这种 `NN-Name` 形态仍可读）。补救是一条明确动作：更新 `app_server_categories_builtin.json`（逐字取自上游索引，带 `_source_sha256` 溯源）并随下次发版生效；
- 市场自己带字典时依旧以市场为准（源 1 优先），所以将来任一方补上字典都不需要改宿主。

### 6.4 真机读数（2026-10-14，宿主 `fp-16`）

启动本机 debug 宿主（隔离数据目录；`AGENT_STORE_CONFIG` 指向无默认市场的临时配置、`AGENT_STORE_MARKET_DOWNLOAD=none`，因此不会去下 289 MiB 官方归档），把站点仓的 `market-source/experts`（14,713 文件的**真实**镜像，且**不带**字典）注册成 `directory` 市场，再读两个面：

```text
initialize: protocol=fp-16 store=true
market/add: id=experts

store/categories: items=15 marketplace_tags=0
  13-TencentZone       => 腾讯专区 / Tencent Zone
  01-ProductDesign     => 产品设计 / Product Design
  02-Engineering       => 技术工程 / Engineering
  08-FinanceInvestment => 金融投资 / Finance Investment
  ...(+11 more)

store/list: items=381  with_category_id=361  resolved_category=361
distinct category ids seen: 15
  02-Engineering (38)       => 技术工程 / Engineering
  04-DataAI (34)            => 数据智能 / Data & AI
  08-FinanceInvestment (26) => 金融投资 / Finance Investment
UNRESOLVED items: 0
sample: experts/senior-developer
  category_id=02-Engineering
  category={"id":"02-Engineering","name":{"en":"Engineering","zh":"技术工程"},...}
  tags=[{"en":"Senior Development","zh":"高级开发"}, ...]
```

四点值得记下：

1. **兜底确实生效**：该市场根上**没有** `manifest.json`（正是线上归档的形状），`store/categories` 仍返回 15 行完整双语字典；381 个条目里 361 个声明了 `categoryId`，**全部**解析成功（`UNRESOLVED items: 0`）。
2. **词表覆盖是满的**：条目实际用到的 id 恰好 15 个，与内置词表一一对应——印证「15 个 id 是闭集」不是抽样巧合。
3. **`marketplace_tags=0` 是如实结果**：市场级标签只存在于技能市场的 `.codebuddy-skill/marketplace.json`，而这里注册的是专家市场。
4. **握手面同步**：宿主自报 `protocol_version: fp-16`，与 SDK 常量一致（跨仓落点由 `check:fingerprint` 守着）。

> 复现：`cargo build -p agent-store`，隔离 `--data-dir` 起在 `127.0.0.1:8787`；WS 上依次 `initialize` → `initialized`（**少这一步每个方法都回 `not_initialized`**）→ `market/add{source_kind:"directory"}` → `store/categories` / `store/list`。

#### 6.4.1 技能市场读数，以及探针抓到的两个真问题

换注册站点仓的 `market-source/skills`（4,633 文件，索引带市场级 tags 与行内 tags）：

```text
store/list: items=262  with_tags=262          ← 技能条目的标签全部来自索引行（`43` 的链路）
  sample: 腾讯文档 => [{"en":"Collaboration","zh":"办公协同"}]
```

**问题 A（已修）：`store/categories` 的答案依赖调用顺序。** 同一次会话里：

```text
categories BEFORE any list: items=15 marketplace_tags=0     ← 冷启动，字典读取器还不知道市场根
categories AFTER  a list:  items=15 marketplace_tags=11     ← 这才拿到技能市场的 11 项词汇表
```

根因：`CategoryDictionary` 只在 `StoreProvider::list()` 遍历市场时才知道各市场的根；而 `store/categories` 正是客户端（筛选栏）**可能最先调**的接口。若某个市场自带分类字典，冷启动那次还会把**内置词表**当成答案返给客户端——不只是缺标签，是**错数据**。

修法：把「走上启用的市场、记下它们的根」抽成一个 helper（`remember_market_roots`，只读注册表 + 解析路径，不碰条目），`list()` 与 `categories()` 都先调它；并把 `remember_root` 改为 **`set_roots`（整体替换）**——原实现只累加，市场被停用/移除后它的字典仍会继续参与合并（同一次探针暴露的第二个隐患）。

修后**同一个实验**（重建二进制后重跑，注册同一个技能市场目录）：

```text
categories BEFORE any list: items=15 marketplace_tags=11   ← 冷启动即正确（修前是 0）
store/list: items=262
categories AFTER  a list:  items=15 marketplace_tags=11
```

单测补了 `the_market_tag_vocabulary_follows_the_root_set`（认得的根会被读、`set_roots` 换掉后不再贡献）与 `a_declared_market_wins_and_the_builtin_covers_its_absence` 里的「停用后回落到内置」断言。

**问题 B（已修文档）：`connectors.json` 没有 tags** —— 见 §5.5 的修正，我原先多写了一句不成立的「数据侧已具备」。

> 这两个问题都是「先做真机读数、再回看断言」抓出来的：单测（构造 `CategoryDictionary` 后显式塞 root）永远走不到冷启动那条路径。

### 6.5 剩余事项

1. **不需要发布归档**（§6.3 ③ 的决定）：线上 `experts.zip` 与 `content/market-hosts.json` 保持一致，客户端行为不变。
2. **兜底词表的维护**：上游新增分类 id 时更新 `app_server_categories_builtin.json`（逐字复制上游索引的 `categories[]`，保留 `_source*` 溯源键）。这是唯一需要人跟的地方，且漏跟的后果是「显示裸 id」而非「显示错名」。
3. **本机上游树残缺，别跑真同步**：`~/.workbuddy/plugins/marketplaces/experts` 只有 2,103 个文件，而镜像有 14,713；`bun run sync:tree`（非 `--listing-only`）会把镜像剪掉约 12,857 个文件。本轮只用了 `--listing-only`（只读镜像）。
4. **`pack:market --only <market>` 的截断缺陷已修**（站点仓同批）：现在 `--only` 会读入现有台账并只覆盖选中的那一项，另外两个市场的摘要不再消失；`test:market-zips` 增了两条断言钉住它。
