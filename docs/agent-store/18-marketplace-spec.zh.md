# 市场规范（兼容层）

> 状态：**现行正文（未正式发版，可改；改动同步更新）**——本规范在发版前只有一个版本（统一称 v1），不设 v1/v1.1/v2 之分（`16-sdk-webui-site-priority-plan.zh.md` §7 决策 4）。覆盖范围：目录布局与发现优先级（§3）、条目模型（§4）、获取与晋升不变式（§5）、`_files.txt`（§6）、注册表与命名空间（§7）、发布自检（§8）、客户端契约（§9），以及机器可校验形态（`docs/agent-store/schemas/marketplace.schema.json` + `scripts/check-agent-store-market.mjs`）。
> 已知偏差见 §11：**D1–D8 全部已处理**（D8 与 D4 于 2026-09-10 批 1 实现，属「实现对齐正文」而非改规范）。**保留的约束**：任何变更走**显式修订 + 偏差登记**，不静默修改本规范。
> 定位：定义**市场的目录形态、清单发现、远程获取与晋升、发布流程、客户端契约**。依 `16` §4 Q5 决策，当前只覆盖 CodeBuddy / WorkBuddy 兼容格式，**不定义 Agent Store 原生市场格式**。
> 代码事实来源：`crates/backend/nomifun-app/src/market_source.rs`、`market_fetch.rs`、`app_server_marketplace.rs`、`scripts/serve-agent-store-market.mjs`、`05-allo-app-server-protocol.md`。

---

## 1. 模型与术语

| 术语 | 含义 |
| --- | --- |
| 市场（marketplace） | 一个可被注册、刷新、投影出条目的目录树 |
| 条目（entry） | 市场内一个可安装单元（插件 / 技能 / 连接器 / 专家） |
| 快照（snapshot） | 一次导入产生的不可变结果（见 `17-plugin-spec.zh.md` §5） |

**市场类型（探测结果，`MarketKind`）**

| `kind` | 探测条件 | 条目粒度 |
| --- | --- | --- |
| `connector-market` | `.codebuddy-connector/connectors.json` | 每个连接器一条 |
| `skill-market` | `.codebuddy-skill/marketplace.json` | 每个技能一条 |
| `plugin-market` | `.codebuddy-plugin/marketplace.json` | `plugins[]` 每项一条 |
| `plugin-root` | `.codebuddy-plugin/plugin.json`（根） | 根本身一条 |
| `cli-connector` | `cli.json`（根） | 根本身一条 |
| `plugin-collection` | 子目录含 `plugin.json` / `cli.json` / `SKILL.md` | 每个子目录一条 |

---

## 2. 市场源类型与地址解析

| `source_kind` | 输入形态 | 归一化 | 拒绝条件 |
| --- | --- | --- | --- |
| `directory` | 本地目录路径 | 由调用方单独处理（不走远程获取） | 目录不存在 |
| `github` | `owner/repo` | `https://github.com/{owner}/{repo}.git` | 非两段路径 / 空段 |
| `git` | `https://` `http://` `git@` `ssh://` `git://` 前缀 | 原样 | 其他前缀且非 `.git` 结尾、且不是已存在目录 |
| `url` | HTTP(S) 清单地址 | 原样 | 非 `http(s)://` |

> `directory` 之外的三种均为**远程源**，走 §5 的获取与晋升流程。

---

## 3. 目录布局与清单发现优先级

探测按**固定顺序**，命中即返回：

1. `.codebuddy-connector/connectors.json` → `connector-market`
2. `.codebuddy-skill/marketplace.json` → `skill-market`
3. `.codebuddy-plugin/marketplace.json` → `plugin-market`
4. `.codebuddy-plugin/plugin.json` → `plugin-root`
5. `cli.json` → `cli-connector`
6. 否则扫描子目录，命中 `looks_like_plugin` 的作为 `plugin-collection`

**「像市场」的判定**（远程获取后的校验）——根目录存在以下任一即通过：

```
.codebuddy-skill/marketplace.json
.codebuddy-connector/connectors.json
.codebuddy-plugin/plugin.json
marketplace.json
cli.json
```

**「像插件」的判定**（子目录）——存在以下任一即通过：`.codebuddy-plugin/plugin.json`、`cli.json`、`SKILL.md`。

**清单基址推导**（`url` 源）：从清单 URL 剥离以下后缀得到 `base`，用于拼接 `_files.txt` 与镜像资源：

```
{base}/.codebuddy-plugin/marketplace.json
{base}/.codebuddy-skill/marketplace.json
{base}/.codebuddy-connector/connectors.json
{base}/marketplace.json
```

**清单自身的结构**：`plugin.json` 字段见 `17-plugin-spec.zh.md` §3；市场清单（`marketplace.json` / `connectors.json`）的条目字段（`name`、`source`、`version`、`strict`、`commands`、`agents`、`skills`、`hooks`、`mcpServers`）与合并/冲突规则见 `02-codebuddy-workbuddy-import-spec.md` §8。**本规范定义市场如何被解析与获取，不重复清单字段。**

---

## 4. 条目模型

条目字段（`market/get` 投影）：

| 字段 | 说明 |
| --- | --- |
| `name` | 条目名（市场内唯一） |
| `source_kind` | 条目来源类型 |
| `source` | **一律相对市场根**的路径（不暴露绝对路径） |
| `version` | 条目声明版本（可缺省） |
| `description` / `keywords` / `category` | 展示元数据（基线字段，单语言） |
| `localized` | 清单里 `<字段>_<语言>` 形式的本地化变体，**原样透传**（§4.1） |
| `snapshot` | 该条目导入后的快照与安装计数（D8，可缺省） |

**硬约束**：条目 `source` 必须是相对路径，保证 `entries/{entry}/import` 在不暴露宿主文件系统的前提下解析。

**跨市场命名空间**：条目 `name` 仅在**市场内**唯一。`store/list` 聚合多个启用市场时，条目标识为 **`{marketplace_id}/{entry_name}`**，因此不同市场的同名条目互不冲突。`market/entries/{entry}/import` 的 `entry` 参数在指定市场内按 `name` 查找，找不到返回 `not_found`。

### 4.1 本地化变体（`localized`，已收编）

真实清单把同一份展示文案按语言写成基线字段的兄弟键（`description_zh` / `description`），本规范**不在服务端选语言**：只有客户端知道读者的界面语言，所以服务端只**原样透传**变体，由客户端按 **D8=A** 回退（决策正文见 `21`）。

**采集规则**（服务端）：键以 `_zh` / `_en` 结尾，且值为**字符串**或**字符串数组**时收进 `localized`；其余一律丢弃（`featured` 是数字、`name_map` 是对象，都不会进入投影）。空串 / 空数组不收，空映射不上 wire。

**wire 形状**（键为清单里的原始字段名）：

```json
"localized": {
  "description_zh": "PDF 工具集",
  "description_en": "PDF toolkit",
  "tags_zh": ["文档"],
  "tags_en": ["documents"],
  "legacy_tags_zh": ["旧文档"]
}
```

**客户端回退链**（`@flowy-agent-store/protocol` 的 `pickLocalized` / `pickVariantText` / `pickVariantList` / `pickEntryTags` / `pickEntryText`）：

1. `<字段>_<界面语言>` → 2. `<字段>_<另一语言>` → 3. 基线字段（`description` / `name` / `keywords`）→ 4. 空。
2. **族优先级**：`tags_*` 整体优先于 `legacy_tags_*`——`legacy_tags_*` 只在 `tags_*` 两种语言都缺失时才顶上（老市场只发了 legacy 字段的情形）。
3. 回退链**按字段**独立求值，不做「整条记录的语言一致性」判断：一条记录里可能出现英文名 + 中文描述。

**消费面**（R28 已接）：`market/get` 条目卡（名称 / 描述 / 标签）、`store/list` 卡片与抽屉（走既有的 `display_*` 结构化字段）、`@` 提及菜单。

**仍未消费的字段（普查 2026-09-10，T20；本地化变体收编后复核）**：

| 字段 | 出现次数（skills 市场） | 现状 |
| --- | --- | --- |
| `description_zh` / `description_en` | 268 / 268 | ✅ 已收编（`localized` + 回退链） |
| `name_zh` / `name_en` | 15 / 2 | ✅ 已收编 |
| `category_zh` / `category_en` | 1 / 1 | ✅ 已收编（透传，UI 暂未展示分类） |
| `tags_zh` / `tags_en` | 268 / 268 | ✅ 已收编（族优先级见上） |
| `legacy_tags_zh` / `legacy_tags_en` | 154 / 154 | ✅ 已收编（降级为 `tags_*` 的兜底） |
| `examples_zh` / `examples_en` | 268 / 268 | ⏳ 已透传、**UI 未展示**（无对应展示位，不新造控件） |
| `featured` | 3 | ⏳ 未收编；注意是**数字**而非布尔 |

> 复现：`node scripts/check-agent-store-market.mjs --census --market <name>=<dir>`。市场清单层面的 `owner`（`{name,email}`）见 §11 D3；marketplace 清单字段的完整普查结果见 §11 D7。

---

## 5. 远程获取与晋升

**不变式**：失败**绝不触碰** last-good 内容根。

### 5.1 Git 源（`github` / `git`）

1. 克隆到独立 staging 目录；
2. 解析到 HEAD commit 作为 `resolved_revision`；
3. 校验 `looks_like_market`，不通过则**阻断并清理 staging**；
4. 与当前 revision 相同 → **no-op**（丢弃 staging，last-good 保留）；
5. 通过 → **原子晋升**（backup + rename），新根就位后才删除备份。

### 5.2 HTTP 源（`url`）

1. **条件请求短路（revision 比对兜底）**：清单请求按上次记录的**原始校验值**发条件头——有 `ETag` 发 `If-None-Match`，有 `Last-Modified` 发 `If-Modified-Since`（两者都有则都发）。服务器返回 `304` → `Unchanged`；若仍返回 `200`，再比对 revision **摘要**（`sha256(ETag|Last-Modified)`，规则未变）——相同同样按 `Unchanged` 处理，作为「忽略条件头的服务器」的兜底；
2. 客户端：User-Agent `allo-agent-store/1.0`，超时 **15s**——清单抓取、`_files.txt` 探测与整树镜像**共用同一个 client 工厂**（§11 D5）；
3. `304 Not Modified` → `Unchanged`（保留 last-good）；
4. `Fresh` → 全量校验清单 → 与当前 revision 相同则丢弃；否则晋升；
5. **若源暴露 `_files.txt`**，镜像整棵条目树（见 §6）；否则条目保持 `manifest-only`，按条目类型标记来源：`skills` / `connectors` 条目解析不到时标 **`external`**（不可镜像），`plugins[]` 条目保留 `directory` + 相对 `source`（导入时再解析并给出缺失报错，§11 D6）。

### 5.3 staging 生命周期

staging 目录由本次获取独占：正常完成时晋升并解除守卫；提前返回或外层超时**必须清理**，避免 `staging-*` 残留堆积。

### 5.4 `content_digest` 算法

✅ **已统一为一套实现（2026-09-10，D2 收口 / T20）**：

| 场景 | 实现 | 算法 |
| --- | --- | --- |
| 导入 / 快照幂等 | `tree_digest`（`nomifun-importer/src/digest.rs`） | 对**按相对路径排序**的每个文件，依次喂入 `relative + "\n"` + 该文件的 SHA-256 十六进制 + `"\n"`，最后整体 SHA-256 |
| 目录市场刷新（变更检测） | **同一个** `tree_digest`，经目录入口 `tree_digest_of_dir(root)` | 同上：先遍历出 `(相对路径, 文件 SHA-256)` 对，再套用同一个 `tree_digest` |

**规范要求（已满足）**：摘要必须**路径敏感、内容敏感、顺序稳定**，不得依赖文件系统遍历顺序；跨路径比对必须用同一算法。排序在 `tree_digest` **内部**完成（不再只依赖 `copy_tree` 的调用约定），并由 `digest.rs` 单测钉住「目录入口与拷贝清单入口对同一棵树给出相同摘要」。

> **迁移影响（一次性）**：目录源市场此前按旧算法写入的 `content_digest` 与新算法不同 → 下一次刷新会判定「内容变了」并重建一次投影。幂等语义、条目内容与安装状态均不受影响。

---

## 6. `_files.txt` 规范

| 项 | 规定 |
| --- | --- |
| 位置 | `{base}/_files.txt` |
| 格式 | UTF-8 纯文本，**每行一个相对路径**，无头部、无注释 |
| 空行 | 忽略 |
| 自身 | 列表中应排除 `_files.txt` |
| 安全 | 消费方必须拒绝绝对路径、含 `..` 的路径、含反斜杠的路径（列表为**不可信输入**） |
| 生成 | `scripts/serve-agent-store-market.mjs --emit-listings`（静态托管如 EdgeOne Pages 需预生成） |
| 语义 | 存在 → **full-tree mirror**（镜像每个列出的文件，保留相对路径）；缺失 → **manifest-only**，条目为 `external` |
| 镜像并发 | 分批（每批 32）并发下载，单文件沿用 15s 超时 |

---

## 7. 注册表与命名空间

| 字段 | 说明 |
| --- | --- |
| `marketplace_id` | 稳定 ID，派生规则见下 |
| `name` / `description` | 展示名与描述 |
| `source_kind` / `source_uri` | 源类型与地址 |
| `version` / `content_digest` | 清单版本与内容摘要（算法见 §5.4） |
| `auto_update` | 自动更新开关（默认值偏差见 §11）。**后台执行已实现（2026-09-10，批 1 / R27）**：宿主 `~/.agent-store/config.toml` 声明 `[marketplace] auto_update_interval_hours` 后，运行时按该间隔在后台轮询；**不声明即关闭**（`0` 亦视为关闭）。轮询对象 = 开关为开 **且** 源属官方镜像——第三方源永不自动拉取（§11 D1）。每次轮询仍走 §5.2 的 revision / ETag 短路，内容未变不重新下载 |
| `enabled` | 是否参与 `store/list` 聚合 |
| `entry_count` | 条目投影数量 |
| `added_at` | 注册时间 |

**`marketplace_id` 派生规则**（规范定义，实现不得偏离）：

1. 取 `market/add` 的显式 `name`；
2. `name` 缺失或空白 → 取 `source` 的 `file_name`（目录名 / 仓库名）；
3. 做 slug 化：**仅保留 `[A-Za-z0-9-_]`**，其余字符替换为 `-`，连续 `-` 合并为一个，首尾 `-` 去除；
4. 结果为空 → 字面量 `market`。

> 稳定性要求：同一 `source` 反复注册必须得到同一 `marketplace_id`；`name` 变化会改变 ID，属破坏性变更（需重新注册）。

- **命名空间**：多市场并存时以 `marketplace_id` 隔离；条目 `name` 仅在市场内唯一（见 §4）。
- **级联移除**：`market/remove` 默认 `cascade=true`，同时卸载由该市场安装的快照并清空组件安装状态。
- ⚠️ **`auto_update` 默认值偏差**见 §11。

---

## 8. 发布流程

1. 按 §3 的布局组织目录（四类资产可各自独立成市场）；
2. 生成 `_files.txt`（`--emit-listings`）；
3. 头像等资产用**相对路径**，随树镜像；
4. 自检清单：
   - 根目录存在 §3 的任一「像市场」清单；
   - 每个条目 `source` 为相对路径；
   - `_files.txt` 覆盖全部需要镜像的文件且不含 `_files.txt` 自身；
   - 非法路径（绝对 / `..` / 反斜杠）为 0。
5. 现有示例：VPS 上的 `experts` / `skills` / `connectors` 三个市场。

> **第 4 步已可自动校验（D2 / T19，2026-09-10）**：`scripts/check-agent-store-market.mjs` 把上述四条实现为机器检查（外加清单发现优先级、条目重名、镜像模式下的 `source` 存在性），每条发现带 `文件#/指针`；`--emit-listings` 写完清单后会串行跑它，不合格即 `exit 1`，因此**发布流程自带这道闸门**。`--self-test` 用 14 个非法样例保证校验器自身不退化成「什么都通过」。参见 `docs/agent-store/schemas/marketplace.schema.json`。

---

## 9. 客户端契约（协议面）

| 方法 | 语义 |
| --- | --- |
| `market/add` | 注册市场；`directory` 立即探测，远程源按 §5 获取 |
| `market/list` | 注册表投影（**不含条目**） |
| `market/get` | 含发现条目与安装快照 |
| `market/refresh` | 重新获取 + 重建投影；按 revision / ETag 短路 |
| `market/remove` | 默认级联卸载 |
| `market/auto-update` | 切换 `auto_update` |
| `market/entries/{entry}/import` | 复用导入管线（`import/*`） |

- 能力协商：`capabilities.marketplaces`；
- 幂等：重复 `market/add` 同一源不产生重复注册；相同内容重复导入返回已有快照；
- 协议面细节以 `05-allo-app-server-protocol.md` 为唯一正文。

---

## 10. 非目标与演进

- 不定义 Agent Store 原生市场格式；
- 不做市场审核后台、签名与信任链（见 `16-sdk-webui-site-priority-plan.zh.md` §6.2.2 明确非目标）；
- 原生格式与市场签名体系待生态起量后单独立项。

---

## 11. 已知偏差

**登记规则（强制）**：规范与实现不一致时，**先在本节登记，再择一修正**——要么改实现，要么改规范，不允许默默不一致。

| # | 现象 | 证据 | 影响 | 待决 |
| --- | --- | --- | --- | --- |
| D1 | **`auto_update` 默认值与文档不符** | `02` §8 称「官方市场默认开启、第三方默认关闭」；`market/add` 恒写入 `auto_update: false`（`app_server_marketplace.rs:527`），不存在官方/第三方区分 | 第三方无法预期自动更新行为；webui 的 auto-update 开关语义不明（W13） | ✅ **已定（2026-09-10）：采纳 ①**——改实现以区分官方/第三方（官方默认开、第三方默认关，V1 不自动更新第三方来源）；`02` §8 表述不动 ✅ **已修（2026-09-10，T14）**：新增 `is_official_source()`（`nomifun-app/src/app_server_marketplace.rs`，与 `AgentStoreConfig::builtin_default_marketplaces()` 同一事实源），`market/add` 的 directory 分支与 `register_remote`（远程源）都改用该默认值；「官方」按**源地址**判定（`source_kind` + `source` 落在 `builtin_default_marketplaces()` 集合内），**不按「由谁声明」判定**：`config.toml [default_marketplaces]` 里指向**其它地址**的源自成第三方（不默认自动更新），而本机 config 声明的三个源正指向官方镜像地址，故按官方处理。实测（重建二进制）：三个官方源 `auto_update=true`，本地第三方 `directory` 市场 `false`。Rust 单测 `official_sources_are_the_builtin_public_mirror` + `cargo check -p nomifun-app --tests` 通过。**注意：当前尚无自动更新后台任务（roadmap 明确留待后续），本项只修默认标记语义** |
| D2 | **两套 `content_digest` 算法** | `tree_digest`（导入）vs `simple_tree_digest`（目录刷新），见 §5.4 | 同一内容在两条路径下摘要不同，跨路径比对不可行 | ✅ **已定（2026-09-10，用户拍板）：采纳 ①——统一为一套算法**。实现：`tree_digest` 内部排序；新增目录入口 `tree_digest_of_dir(root)`（`nomifun-importer/src/digest.rs`），`app_server_marketplace.rs` 的两处调用改走它，`simple_tree_digest` / `collect_files` 已删除（§5.4 已改写）。验证：`cargo check -p nomifun-app -p nomifun-importer --tests` exit 0；`cargo test -p nomifun-importer --lib digest` **4 passed**（含「两条路径摘要一致」新用例）。迁移：目录源市场首次刷新会因摘要变化重建一次投影 |
| D3 | **清单 `owner` 字段本规范未定义，真实数据是对象** | 真实 `skills` 与 `connectors` 市场的 `owner` 为 `{name, email}`（CodeBuddy），而 `17` §3 只固定了 `author`（`string \| {name, email}`），§3 又把清单字段整体让给 `02` §8 | 机器校验若把 `owner` 当字符串，会把两个真实市场判错（T19 首轮即发生，被自检/真实市场跑检验出） | ✅ **已处理（2026-09-10，T19）**：`docs/agent-store/schemas/marketplace.schema.json` 按 `author` 同形接受 `owner`；规范正文不改（该字段归 `02` §8），仅记此观察项 |
| D4 | **清单条件请求的取值不是服务器原始 `ETag`，且从不发送 `If-Modified-Since`** | `market/refresh` 把存库的 `resolved_revision`（= `sha256_hex(etag \| last-modified)`，`market_source.rs:278-282`）当作 `if-none-match` 发出（`market_fetch.rs:88` → `market_source.rs:246-248`）；`Last-Modified` 只参与摘要计算，从未用于条件请求 | 对真实服务器 304 分支不可达 → §5.2 声称的「条件请求短路」在 v1 实际由 revision 摘要比对兜底。**功能结果一致**（清单未变仍判 `Unchanged`、不重镜像），代价是每次刷新多下载一次清单 | ✅ **已实现（2026-09-10，批 1 / R26）**：新增迁移 `057_marketplace_source_validators.sql` 持久化**原始** `source_etag` / `source_last_modified`（内部可追溯字段，与 `resolved_revision` 同级、不上协议）；`fetch_http_market` 改发真条件头（有 ETag 发 `If-None-Match`、有 Last-Modified 发 `If-Modified-Since`），刷新成功后写回。`resolved_revision` 的标记规则**保持不变**（ETag 优先 → Last-Modified 兜底 → `http`），因此升级**不会**让任何 URL 市场「看起来变了一次」。验证：`nomifun-db --lib marketplace` **9 passed**（写入 / 清除回读）、`nomifun-app --lib market_source` **10 passed**（304 条件请求、`If-Modified-Since` 实际发出、标记规则回归） |
| D5 | **清单抓取那条路径漏设 15s 超时** | `fetch_http_market` 自建 `reqwest::Client`（`market_source.rs:241-244`）只设 UA，无 `.timeout()`；同文件 `http_client()`（`:107-113`）才是 UA + 15s | 清单服务器挂起时 `market/refresh` 无自身超时（只剩外层 600s 兜底），与 §5.2 步骤 2 不符 | ✅ **已修（2026-09-10，T20）：改实现**——`fetch_http_market` 改用 `http_client()?`，与探测/镜像共用同一 client 工厂 |
| D6 | **manifest-only 模式下 `plugins[]` 条目未标 `external`** | `market_fetch.rs:226` 的 `key_kind != "plugin"` 例外：skills/connectors 解析不到时标 `external`（`:229-241`），plugins 仍标 `directory`（`:244-255`） | 与 §5.2 步骤 5 原文「条目标记为 external」不符；plugin 市场在 manifest-only 场景下 UI 显示成本地目录，导入时才报缺失 | ✅ **已定（2026-09-10，T20）：采纳 ② 改规范**——§5.2 步骤 5 已按条目类型写明；① 改实现（去掉例外）会改变 `store/list` 投影与既有断言，收益不抵风险 |
| D7 | **真实市场携带规范未列的清单/条目字段** | `scripts/check-agent-store-market.mjs --census`（T20 新增模式）对三个真实市场普查：manifest 层 spec-silent = `plugin`(×7)、`members`(×3)、`license`(×1)（experts）/ `distribution`、`homepage`、`license`、`repository`、`settings`（skills）；**entry 层** = `description_zh/en`(×268)、`examples_zh/en`(×268)、`legacy_tags_zh/en`(×154)、`name_zh`(×15)、`featured`(×3, number)、`name_en`(×2)、`category_zh/en`(×1) | 这些字段当前靠 schema `additionalProperties` 容忍（不报错、不消费）。但 §3/§4 的字段表声称固定「必填/可选/默认」，表里没有它们 → 第三方无法从规范判断哪些会被消费 | ✅ **已定（2026-09-10，T20）：采纳 ①**——在 §3 末尾补「真实市场已出现、规范未消费」清单（标注**透传、不消费**），使字段表与真实数据一致；后续若要消费其中某项（如本地化变体），按显式修订定义。→ **2026-09-10（批 1，R28）本地化变体已收编**：新增 `localized` 通用映射并定义客户端回退链与 `tags_*` 族优先级（§4.1）；`examples_*` 已透传但无展示位、`featured` 维持透传不消费 |
| D8 | **`market/get` 未返回条目安装快照** | 响应结构 `AppServerMarketplaceDetail`（`app_server.rs:592-599`）只有 `summary + entries`；类型 `AppServerMarketplaceEntrySnapshot`（`:583-590`）已定义却未挂载；`to_entry`（`app_server_marketplace.rs:409-419`）不含 `snapshot_id` / 安装态 | §9 声称 `market/get`「含发现条目**与安装快照**」；W13 的「移除市场」影响面因此只能从聚合的 `store/list` 派生（见 `16` 已知偏差 D-W13-1） | ✅ **已实现（2026-09-10，批 1；D11=A 批准协议增量）**：`market/get` 的每条目新增可选 `snapshot`（`AppServerMarketplaceEntrySnapshot` 增 `installed_count`，纯 additive、缺省不上 wire），由 `app_server_marketplace.rs` 的 `get()` 经新增仓储查询 `list_snapshot_provenance_by_marketplace`（一次 JOIN 出 component / installed 计数）投影；TS 侧补 `MarketplaceEntrySnapshot`。验证：`cargo test -p nomifun-db --lib marketplace` **8 passed**（含新用例）、`market_impls_route_through_the_provider_and_gate` 通过 —— **§9 声称的「`market/get` 含发现条目与安装快照」由此成立** |

> D1 影响 webui 的 W13（市场管理）排期，需优先拍板（主计划 Q7）。
