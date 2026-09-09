# 市场规范（兼容层）

> 状态：规范 v1（2026-09-09）。
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

---

## 4. 条目模型

条目字段（`market/get` 投影）：

| 字段 | 说明 |
| --- | --- |
| `name` | 条目名（市场内唯一） |
| `source_kind` | 条目来源类型 |
| `source` | **一律相对市场根**的路径（不暴露绝对路径） |
| `version` | 条目声明版本（可缺省） |
| `description` / `keywords` / `category` | 展示元数据 |

**硬约束**：条目 `source` 必须是相对路径，保证 `entries/{entry}/import` 在不暴露宿主文件系统的前提下解析。

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

1. 条件请求清单（`ETag` / `Last-Modified`）；
2. 客户端：User-Agent `allo-agent-store/1.0`，超时 **15s**；
3. `304 Not Modified` → `Unchanged`（保留 last-good）；
4. `Fresh` → 全量校验清单 → 与当前 revision 相同则丢弃；否则晋升；
5. **若源暴露 `_files.txt`**，镜像整棵条目树（见 §6）；否则条目保持 `manifest-only`，其条目来源标记为 **`external`**（不可镜像）。

### 5.3 staging 生命周期

staging 目录由本次获取独占：正常完成时晋升并解除守卫；提前返回或外层超时**必须清理**，避免 `staging-*` 残留堆积。

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
| `marketplace_id` | 稳定 ID；`name` 缺省时由源派生 |
| `name` / `description` | 展示名与描述 |
| `source_kind` / `source_uri` | 源类型与地址 |
| `version` / `content_digest` | 清单版本与内容摘要 |
| `auto_update` | 自动更新开关 |
| `enabled` | 是否参与 `store/list` 聚合 |
| `entry_count` | 条目投影数量 |
| `added_at` | 注册时间 |

- **命名空间**：多市场并存时以 `marketplace_id` 隔离；条目 `name` 的市场内唯一性不跨市场。
- **级联移除**：`market/remove` 默认 `cascade=true`，同时卸载由该市场安装的快照并清空组件安装状态。
- ⚠️ **已知偏差（待修正）**：`02` §8 表述为「官方市场默认开启自动更新、第三方默认关闭」；实现 `market/add` **恒写入 `auto_update: false`**，不存在官方/第三方区分。二者必须择一：要么实现区分，要么修正 `02`。

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
- 不做市场审核后台、签名与信任链（见 `agent-store-v1-roadmap.md` §10 非目标）；
- 原生格式与市场签名体系待生态起量后单独立项。
