# 30 · 市场 zip 单包托管（ModelScope）

> 状态：**已落地（2026-09-17）**。本批是 wire 变更（`AppServerMarketplaceSourceKind` 新增枚举值 + `plugin_marketplaces.source_kind` 的 CHECK 放宽），协议指纹随本批 bump 到 **`fp-7`**。三个官方归档已上传到 ModelScope 并通过远端摘要回验（§9.5），客户端对真实归档的端到端见 §9.6。

## 1. 问题

两件事共用一个根因：官方市场以「站点逐文件托管 + 客户端逐文件镜像」分发。

### 1.1 站点产物超限，部署是红的

EdgeOne Makers 的产物上限是 **20,000 个文件**与**单文件 25 MiB**（`agent-store-site/scripts/copy-market-tree.mjs:37-38` 已写明，无提额入口）。而：

| | 文件数 |
| --- | --- |
| 站点自身 | 94 |
| `market-source/experts` | 14,714 |
| `market-source/skills` | 4,634 |
| `market-source/connectors` | 3,264 |
| **合计** | **22,706** |

`copy-market-tree.mjs:49` 的 `HOSTED_DEFAULT = MARKETS` 是三个市场全托管，所以**默认构建必然超限**——线上部署因此是红的，不是偶发。脚本里已有 `SITE_HOSTED_MARKETS` 做按市场取舍，注释也写明「专家市场必须迁到别的宿主」，**但替代宿主一直空缺**。

### 1.2 客户端为 experts 要发 14,714 次请求

官方源是 `url` 类型 + `_files.txt`，客户端 `mirror_http_tree`（`crates/backend/nomifun-app/src/market_source.rs:132-200`，BATCH=32）**逐个 GET 每个文件**：

| experts 首次获取 | 请求数 | 字节 |
| --- | --- | --- |
| 今天（整树镜像） | 14,714 | 611.3 MiB |
| zip 单包 | **1** | **289.0 MiB** |

「首次获取」= `market/add`（新用户第一次打开商店）与清单变更后的刷新；普通刷新由清单 ETag 短路，不重镜像（实测线上 `agent-store.flowyaipc.cn/source/experts/.codebuddy-plugin/marketplace.json` 同时带 `Etag` 与 `Last-Modified`，`HttpValidators::marker()` 不会退化成常量 `"http"`）。

而 `ensure_default_marketplaces` 给每个默认源套了 **600s** 上限（`crates/backend/nomifun-app-server/src/lib.rs:2153-2159`），其注释自认「整树镜像 1–3 分钟是常见最坏情况」——14,714 次请求 + 611 MiB 已贴近该上限。

## 2. 决策（2026-09-17 用户拍板）

| # | 决策 | 理由 |
| --- | --- | --- |
| D1 | **站点解红与 zip 落地同批**，不先单独降 `HOSTED_DEFAULT` | 避免出现「站点绿了但 experts 无可达宿主」的中间态 |
| D2 | **三个市场一起迁**，不留 `url` 整树托管 | 形态统一、只走一条代码路径；站点产物 22,706 → 约 742 |
| D3 | **老用户迁移只写文档**，不实现地址自动改写 | 未正式发版，成本优先；失败方式温和（见 §7） |
| D4 | 本批取 **`fp-7`** | ⚠️ **决策时的依据已过期**：拍板时指纹是 `fp-3`、`fp-4` 被 doc 27 的阶段 2 预留着，所以原决定是「本批取 `fp-4`、他们顺延 `fp-5`」。落地时另一个会话已经把 `fp-4`（专家开场）/ `fp-5`（专家团开场）/ `fp-6`（模型与思考等级）**全部落地**，取号没有冲突可言——本批直接取下一个空号 `fp-7`，无需任何人顺延 |

## 3. 方案：新增 `zip` 源类型

### 3.1 源类型

`AppServerMarketplaceSourceKind` 增加 `Zip`（wire 串 `"zip"`）。`source` 是一个 **HTTP(S) 归档地址**：

```toml
[default_marketplaces.experts]
source_kind = "zip"
source = "https://www.modelscope.cn/models/me9rez/flowy-marketplace/resolve/master/experts.zip"
```

归档根目录**就是市场根**（zip 内第一层即 `.codebuddy-plugin/marketplace.json` 等清单所在目录）。与 WorkBuddy `known_marketplaces.json` 的 `type: "zip"` / `source.source: "zip"` / `source.url` 同构。

`url` / `github` / `git` / `directory` **全部保留**：本批只改官方源的用法，第三方仍可用整树镜像。

### 3.2 获取与晋升

走既有 `fetch_remote`（`market_fetch.rs:50`）的同一套「staging → 校验 → 原子晋升」，新增 `"zip"` 分支：

1. `normalize_source_url("zip", source)`：只接受 `http(s)://`。
2. **新鲜度先探**：对稳定 URL 发 `HEAD`，取 `X-Linked-Etag`（见 §3.3）。与存库 `resolved_revision` 相同 → `Unchanged`，**不下载**。
3. 变了才 GET，**流式落盘**到 staging **之外**的临时文件（下完即删/移动），跟随 302 重定向。
4. **校验**：本地 `sha256(归档)` 必须等于 HEAD 拿到的值（省一次 hash 之外的完整性校验）。
5. 解压到 staging，**归档自身绝不进入 staging**——否则 289 MiB 会被当成市场内容晋升进 live root。
6. `looks_like_market(&staging)` 校验；不过即报错、`StagingGuard` 回收，last-good 不动。
7. `probe_directory` → 条目；`promote` 原子替换 live root。

三个**必须显式给的预算**（现成默认值不够）：

| 项 | 现成默认 | experts 实际 | 结论 |
| --- | --- | --- | --- |
| 解压条目上限 | `ZipExtractionBudget` 默认 20,000 | 14,714 | 抬高 |
| 解压总字节上限 | `ZipExtractionBudget` 默认 256 MiB | **611.3 MiB** | **必须抬高**，否则必定失败 |
| HTTP 超时 | `http_client()` = UA + 15s（`market_source.rs:107-113`） | 289 MiB | **必须另设**（仓内先例 `crates/agent/nomi-config/src/runtime_dep_install/ffmpeg.rs:173` 用 300s + 流式落盘 + 镜像回退） |

解压安全原语复用 `nomifun-common::zip_safe`（`safe_zip_entry_path` / `zip_entry_is_symlink` / `ZipExtractionBudget`），调用方自己定重复条目与大小写碰撞策略。

### 3.3 revision 与新鲜度：`X-Linked-Etag` = 内容 sha256

ModelScope 的 `.zip` **按后缀必走 LFS**，首发 302 到 CDN。实测（2026-09-17）：

- **`X-Linked-Etag` 就是文件内容的 sha256** —— 把 `config.json` 下下来本地算 sha256，与响应头逐字节一致；也等于 files API 的 `Sha256` 字段。
- **`HEAD` 稳定 URL 直接返回它**（200、不重定向、零正文）→ 一次 HEAD 即可判新旧。
- 条件请求不可依赖：modelscope.cn 那层（非 LFS 小文件）**忽略** `If-None-Match` / `If-Modified-Since`（仍 200）；CDN 那层对 LFS 文件**支持** 304，但要走 302→CDN 并依赖跨源转发条件头。**本方案统一用 HEAD + sha256**，跨宿主也更稳。

因此 `resolved_revision` = 该 sha256。归档内容不变 → revision 不变 → 不下载。

### 3.4 三处静默兜底（★ 最容易漏的地方）

新增枚举值后，**只有一处会被编译器抓住**：

- ✅ `AppServerMarketplaceSourceKind::as_str` 的穷尽 `match` → 编译错误。

以下三处**不会被抓住**，必须人工改：

| 位置 | 现状 | 不改的后果 |
| --- | --- | --- |
| `lib.rs:2137-2141` | `_ => AppServerMarketplaceSourceKind::Url` | 默认源里的 `zip` 被**静默当成 `url`**，配置看着对、行为全错 |
| `market_source.rs:77` | `other => Err(...)` | `zip` 被拒为「不支持的远程源」 |
| `market_fetch.rs:136` | `other => Err(...)` | 同上 |

`lib.rs` 那处的 `_ =>` 兜底本身也该收紧为显式列举 + 未知即失败，避免下次再静默错映射。

## 4. ModelScope 托管（实测事实）

仓：`me9rez/flowy-marketplace`。**公开、匿名可读、默认分支 `master`、创建时是空的**——必须先上传。

### 4.1 下载

```
https://www.modelscope.cn/models/me9rez/flowy-marketplace/resolve/master/<market>.zip
```

等价 API 形态：`/api/v1/models/me9rez/flowy-marketplace/repo?Revision=master&FilePath=<market>.zip`（两者内容字节一致；SDK 内部用后者）。

要点：

- LFS 文件 GET 首发 **302** 到 `cdn-lfs-cn-1.modelscope.cn/prod/lfs-objects/<sha256>?auth_key=<时间戳签名>`。`auth_key` 是**临时签名**（实测篡改即 403）→ **必须跟随重定向，绝不能把 CDN URL 固化进配置或文档**。
- CDN 支持 Range（实测 `206` + `Accept-Ranges: bytes` + `Content-Range`）。
- 匿名可读、无需 User-Agent；`gated` 对 public 仓无效，不会挡住匿名下载。
- commit sha 可 pin（`resolve/<sha>/<path>`）；`master` 可变。

### 4.2 上传

token 环境变量 **`MODELSCOPE_API_TOKEN`**；**只从运行环境读，绝不入仓**。

```bash
ms upload me9rez/flowy-marketplace ./dist-market/experts.zip --repo-type model \
  --commit-message "Publish experts market"
```

或 `HubApi().upload_file(repo_id=..., repo_type="model", path_or_fileobj=..., path_in_repo=...)`。

**不要用** `/openapi/v1/files/upload`（硬上限 5 MiB，且非仓级）。限制：单文件 ≤100 GB、单次 ≤10 万文件、强制 LFS 阈值 1 MB——289 MiB 毫无压力。

### 4.3 未验证项

- `auth_key` 的具体 TTL（只知篡改→403、数分钟后旧 key 仍可用）。
- 本仓真实 `.zip` 的上传→302 端到端链路（无 token，未实际上传）。链路推理：`.zip ∈ MODEL_LFS_SUFFIX` + 仓内 `.gitattributes` 含 `*.zip filter=lfs` + 实测小 LFS 文件也 302。
- CDN 长期 SLA（ModelScope 未公布）。

Studio **不能**当静态托管（`/studios/.../resolve/...` 返回 SPA HTML）；dataset 与 model 等价。

## 5. 站点侧

### 5.1 产物

`copy-market-tree.mjs` 的 `HOSTED_DEFAULT` 清空（三个市场全 zip 托管），只保留目录页引用的图标：

| | 文件数 |
| --- | --- |
| 站点自身 | 94 |
| 目录页图标（experts 306 + skills 114 + connectors 228） | 648 |
| **合计** | **约 742** |

图标**必须留站点**：`app/lib/market.ts::avatarUrl()` 按同源绝对路径解析，少一张就退化成字母徽标，与市场树托管在哪无关。图标清单由 `snapshotAssets()` 从 `content/market.json` 反查（`check:market` 实测：3 市场 0 发现，快照 381/268/228、avatars=648）。

### 5.2 打包与发布

- `scripts/pack-market-zips.mjs`（新）：**先过 `check:market` 再打包**。复用 `release.mjs` 已有的 `crc32` + `createDeflateRaw` + **显式 mtime** 写入器，扩成多条目版 → **确定性 zip**（摘要可复现）。补大小写碰撞检查（Linux 打得出来、Windows 解不开的两个同形名）。
- `scripts/publish-market-zips.mjs`（新）：上传 + 用 HEAD `X-Linked-Etag` 回验远端 sha256 与本地一致 + 回显大小/URL。
- `content/market-hosts.json`（新，产物）：每个市场的 zip URL + sha256 + 大小 + 文件数，供站点文案与门禁共用，**不手抄**。

`market-source/` 与 `_files.txt` 保留在仓内（它们是打包的输入，也是 `check:market` 的校验对象）；只是不再进部署产物。

## 6. 实测体积

下表是**站点打包脚本的真实产出**（`bun run pack:market`，2026-09-17，耗时 97s）：

| 市场 | 文件数 | 树大小 | zip | 压缩比 | sha256（前 12 位） |
| --- | --- | --- | --- | --- | --- |
| experts | 14,713 | 611.3 MiB | **289.6 MiB** | 2.11x | `5cd0ab947c7e` |
| skills | 4,633 | 41.9 MiB | **17.9 MiB** | 2.34x | `fca2fead19d9` |
| connectors | 3,263 | 37.6 MiB | **16.7 MiB** | 2.25x | `4c35ed10007c` |

> **文件数比目录里少 1**（14,714 → 14,713 等）：打包用的是 `sync-market-tree.mjs` 的 `listFiles`，
> 它是「什么算市场里的文件」的唯一定义，排除了市场根目录的 `_files.txt`——那是 `url` 镜像协议
> 的产物，不该进归档。体积与压缩比是另一套数字：早期用 .NET `ZipFile`（Optimal）探针得到
> 289.0 / 17.7 / 16.5 MiB，与上表差 0.2–0.6 MiB，属不同 deflate 实现之差，**以脚本产出为准**
> （完整 sha256 见 `content/market-hosts.json`，不在此抄写）。

experts 的体积高度集中：≥1 MiB 的 **77 个文件占 50.9%**，≥10 MiB 的 **8 个占 25.3%**（malaysia/indonesia 插件的 CSV/PDF/DuckDB 载荷）。§10 记了「按需拉载荷」的后续空间。

## 7. 迁移（只写文档）

`init.rs:139` 把 `builtin_default_marketplaces()` 写进 `~/.agent-store/config.toml`；而 `ensure_default_marketplaces`（`lib.rs:2122-2131`）**只要用户配置声明了 `[default_marketplaces]` 就不看内置**。所以改内置地址**只对**「全新安装」与「配置里未声明市场源」的机器生效。

跑过 `init` 或照文档手抄过 URL 的机器，配置里冻着旧站点地址。失败方式温和：`fetch_remote` 失败不碰 live root，**条目不会被删，只是停在旧数据**。

文档口径（站点 `upgrade.md` / `configuration.md`）：把配置里那三条 `[default_marketplaces.*]` 删掉（回落到内置默认）或改成新的 zip 地址。**不实现地址自动改写**（D3）。

## 8. 改动清单

**主仓**

| 文件 | 改动 |
| --- | --- |
| `crates/backend/nomifun-api-types/src/app_server.rs` | 枚举加 `Zip` + `as_str` |
| `crates/backend/nomifun-app/src/market_source.rs` | `normalize_source_url` 加 `zip`；HEAD 取 sha256；流式下载 + 解压 |
| `crates/backend/nomifun-app/src/market_fetch.rs` | `fetch_remote` 加 `"zip"` 分支；revision 用 sha256 |
| `crates/backend/nomifun-app/Cargo.toml` | 加 `zip.workspace = true` |
| `crates/backend/nomifun-app-server/src/lib.rs` | `ensure_default_marketplaces` 的 kind 映射加 `zip` 并收紧 `_ =>` 兜底 |
| `crates/backend/nomifun-app-server/src/agent_store.rs` | `builtin_default_marketplaces()` 改指 ModelScope zip |
| `web/packages/protocol/src/protocol.ts` | `MarketplaceSourceKind` 加 `"zip"`；指纹 `fp-7` |
| `web/src/components/catalog/{shared.tsx,MarketSourcesPanel.tsx}` | kind 映射 + option + placeholder |
| `web/src/i18n/{zh-CN,en-US}.ts` | `marketKindZip` / `marketZipPlaceholder` |
| `crates/backend/nomifun-db/migrations/065_marketplace_zip_source_kind.sql`（新） | 重建 `plugin_marketplaces` 放宽 `source_kind` 的 CHECK |
| `crates/backend/nomifun-app/Cargo.toml` | 加 `hex.workspace = true`（`zip` 已在，`sha2` 已有） |
| `crates/backend/nomifun-app-server/Cargo.toml` | 加 `tracing.workspace = true`（该 crate 此前零日志；未知 kind 的告警需要它） |
| `apps/agent-store/src/init.rs` | 配置模板注释改口径（归档、不再提 `_files.txt`） |

**站点**

| 文件 | 改动 |
| --- | --- |
| `scripts/lib/zip-lite.mjs`（新） | 流式、确定性 zip 写入器（条目 >65,535 / 偏移 >4 GiB 直接报错，不写半个 ZIP64） |
| `scripts/pack-market-zips.mjs`（新） | 先过 `check:market` 再打包；`listFiles` 为唯一文件集定义；大小写碰撞拒绝；`--check` 自检确定性（打临时目录，**不碰** `dist-market/`） |
| `scripts/pack-market-zips.test.mjs`（新） | 8 条：`crc32` 已知常量、确定性、固定 mtime 生效、`node:zlib` **独立** inflate 回读、反斜杠拒绝、大小写碰撞。已接进 `check:release`（`test:market-zips`） |
| `scripts/publish-market-zips.mjs`（新） | 走官方 `ms` CLI 上传（token 只读环境）+ `HEAD` 回验远端 sha256 |
| `content/market-hosts.json`（新，产物） | host + 每市场 zip URL / sha256 / bytes / files；**不写时间戳**（幂等） |
| `scripts/copy-market-tree.mjs` | `HOSTED_DEFAULT` 清空（三个市场全迁，站内只留目录页图标） |
| `package.json` / `.gitignore` | `pack:market` / `publish:market` / `test:market-zips`；忽略 `/dist-market/` |
| `content/docs/{zh-CN,en-US}/{configuration,plugins-market,upgrade,changelog}.md` | 双语同步（`compatibility.md` 复查后无需改） |

**指纹落点**（`bun run check:fingerprint` 管）：`lib.rs`、`protocol.ts`、`http-transport.ts`、`mock-server.ts`、`smoke.ts`×2、`readiness.test.ts`×2、`scripts/probe-agent-store-runtime.mjs`，站点 `typescript-sdk.md`×2 —— 实测 **10 处 / 7 文件 + 站点 2 处**，全绿。

## 9. 落地记录（2026-09-17）

### 9.1 实现期发现的两个**真缺陷**（都不是本批引入的）

**① `looks_like_market` 漏了 `.codebuddy-plugin/marketplace.json`** —— 这条几乎让整个功能无声地失败。

`probe_directory` 的发现顺序里有它（doc 18 §3 第 3 条），但「远程获取后的校验」`looks_like_market` 里**没有**。而官方 `experts` 市场的根目录**只有这一个文件**（`.codebuddy-plugin/marketplace.json`，实测确认）。后果：

- `zip` 分支会把自己的官方归档判成「不像市场」而拒绝——**功能对 experts 直接不可用**；
- 更早就在错的：`github` / `git` 源克隆这种布局同样被拒，而 `probe_directory` 自己的注释举的例子正是 `marketplaces/experts/.codebuddy-plugin/marketplace.json`。

已修（`MARKET_MANIFEST_PLUGIN_MARKET` + doc 18 §3 订正），回归钉 `looks_like_market_accepts_a_plugin_market_root`。

**② `plugin_marketplaces.source_kind` 的 CHECK 不含新值** —— 由 e2e 暴露，单测抓不到。

`055` 建表时写了 `CHECK (source_kind IN ('directory','github','git','url'))`，SQLite 改不了 CHECK，于是新增迁移 `059` 重建表。重建时**必须带上 056/057 新增的四列**（`resolved_revision` / `staging_root` / `source_etag` / `source_last_modified`），否则静默丢列。已核对：该表**没有真实外键**（`plugin_snapshots.marketplace_id` 只是 `id_schema_contract` 里的**逻辑**引用），所以 `DROP TABLE` 不会级联 `SET NULL`。

### 9.2 三处静默兜底（编译器只抓得住一处）

新增枚举值后，**只有 `as_str` 的穷尽 `match` 会编译报错**。以下三处不会被抓住，逐一人工核对并改掉：

| 位置 | 原本 | 现在 |
| --- | --- | --- |
| `lib.rs::ensure_default_marketplaces` | `_ => AppServerMarketplaceSourceKind::Url`（把 `zip` **静默当成 `url`**） | 新增 `AppServerMarketplaceSourceKind::parse()`（与 `as_str` 同处一地），未知 kind **跳过 + 告警 + 标记不完整** |
| `market_source::normalize_source_url` | `other => Err(不支持的远程源)` | 显式 `zip` 分支 |
| `market_fetch::fetch_remote` | 同上 | 显式 `zip` 分支 |

**还有第四处，`fp-7` 当时漏了**：`AgentStoreMarketplace::resolved()` 里那份手抄的 kind 白名单（§9.8）。
教训不是「补上第四行」，而是**新增 kind 的正确做法是全仓搜字面量**（`"directory"` / `"url"` 之类），
而不是照着上表核对——上表本身就是「我以为的完整清单」。

### 9.3 已知风险：解压落点可能越过 Windows MAX_PATH（**未修，登记**）

experts 树里最长的条目相对路径 **174 字符**，而解压落点是
`{work_dir}/agent-store-markets/experts/staging-<uuid v7，36 字符>/…` → 目标路径约 **292 字符**。

- 实测本机（`LongPathsEnabled = 1`）：306 字符的目录与文件都能建，`cmd` 也能写 → 本机没问题；
- 但**默认装机 `LongPathsEnabled = 0`** 时 260 就上限了，届时 experts 会解压失败。

**不是本批引入的**：`url` 的整树镜像本来就写同样深的同一棵树。zip 没让它更糟（同样的相对路径、同样的 staging 前缀），但也没修。可行的收口方向（未做）：把 staging 目录名从 `staging-<uuid36>` 缩短（省 ~28 字符，仍偏紧）、或解压到更短的根、或在宿主侧声明长路径支持。

### 9.4 验证读数（2026-09-17）

**Rust**

| 检查 | 读数 |
| --- | --- |
| `cargo check -p nomifun-app` | exit 0 |
| `cargo test -p nomifun-app --lib market_source` | **16 passed**（含 4 条 zip 单测 + `looks_like_market` 的回归钉） |
| `… --lib market_source -- --ignored` | **1 passed**（9.71s，真实 ModelScope 归档的端到端，见 §9.6） |
| `cargo test -p nomifun-app-server --lib agent_store` | **36 passed** |
| `cargo test -p nomifun-db --lib marketplace` | **13 passed** |
| `cargo test -p nomifun-db migration` | 0 failed |
| `cargo test -p nomifun-app --test importer_e2e` | 15 passed / **1 failed（与本批无关）** |

那条失败是 `importer_mention_resolves_installed_preset_and_agents_run_gate`。它断言 `agent/run` 会以 `agent_not_installed` / `runtime_unavailable` / `invalid_request` 之一失败，而实测响应是**成功创建的 run**（`{"run_id":…,"status":"planning"}`，没有 `code`）。归属清楚：`runtime_unavailable` 门禁最后一次改动是提交 `042ce226b`（2026-09-17，「连接器工具参数、会话绑定与模型/思考等级（fp-1 → fp-6）」——**本批之外的另一个会话已提交的工作**），测试未随之更新。本批的 Rust 改动只落在市场源 + 一个指纹常量 + 一个新迁移，`git status` 里除本批外没有其它未提交源码改动。

**迁移 `059` 的行拷贝分支**（表重建最易出错处）用一个一次性 `bun:sqlite` 脚本按 055→057 的旧形态建表、写入一行带全部 19 列的数据、再逐句执行 059 原文：

```
列保留：19/19          索引重建：是
插入 zip 行：成功      非法 kind 仍被拒：是      (source_kind, source_uri) 唯一性仍生效：是
```

**真实归档**（`dist-market/experts.zip`，由打包脚本产出）用 .NET 读中央目录核对：

```
条目数 14,713   未压缩 610.1 MiB
默认预算 256 MiB / 20,000 条目 → 超出（**证明抬高预算不是臆测**）
本批预算 4 GiB / 200,000 条目 → 容纳
条目名含 ':' 0 个，含反斜杠 0 个（Windows 上不会被 RejectDrivePrefix 拒）
```

**前端与门禁**

| 检查 | 读数 |
| --- | --- |
| `cd web && bun run typecheck` | exit 0 |
| `cd web && bun run test` | **513 passed / 1 skipped** |
| `bun run check`（主仓仓级） | **exit 0**——市场门禁 14 个非法样本全被拒；指纹 `fp-7` 10 处 / 7 文件 + 站点 2 处；release-sync `48 / 71` |
| 站点 `bun run check:release` | **exit 0**——docs-sync 10 页 0 drift、test:docs-sync 16、check:market 3 市场 0 发现、test:market 26、**test:market-zips 8**、typecheck 0 |
| `node scripts/pack-market-zips.mjs --only connectors --check` | exit 0——两次打包 sha256 相同（确定性成立），且未触碰 `dist-market/` |
| 站点 `bun run build` | **产物 742 个文件**（原 22,706）；`source/` 只剩 648 张头像、`_files.txt` 已消失 |

**观察到的偶发**：站点 `test:market` 的 `this repository's markets` 用例在打包 + 两个 cargo 编译同时压满机器时失败过一次（耗时 5004ms；空闲重跑 1834–2416ms 通过）。该用例无内部时限，纯遍历 22,612 个文件，判定为负载偶发，非回归。

### 9.5 上传记录（2026-09-17，**已完成**）

```
$env:MODELSCOPE_API_TOKEN=<令牌>  bun run publish:market      # 328s
[publish-market] experts:    ✓ 远端摘要与本地一致（第 1 次探测）
[publish-market] skills:     ✓ 远端摘要与本地一致（第 1 次探测）
[publish-market] connectors: ✓ 远端摘要与本地一致（第 1 次探测）
```

**独立复核**（`bun run publish:market -- --verify-only`，不需要令牌也不需要 `ms`）：三个市场全绿。
远端文件清单（`/api/v1/models/me9rez/flowy-marketplace/repo/files?Revision=master`）：

| 文件 | 大小 | IsLFS | sha256（与本地逐字节一致） |
| --- | --- | --- | --- |
| `experts.zip` | 289.6 MiB | **True** | `5cd0ab947c7ed497…` |
| `skills.zip` | 17.9 MiB | **True** | `fca2fead19d91480…` |
| `connectors.zip` | 16.7 MiB | **True** | `4c35ed10007c60715…` |

`IsLFS=True` 与 §4.1 的预测一致（`.zip` 按后缀必走 LFS → 302 到 CDN）。

### 9.6 客户端路径对**真实归档**的端到端

新增联网测试（`market_source::tests::live_official_zip_market_probe_download_and_extract_agree`，
`#[ignore]`，默认不跑）：

```
cargo test -p nomifun-app --lib market_source -- --ignored
test result: ok. 1 passed ... finished in 9.71s
```

它走**客户端自己的**代码路径验三件事：三个官方源都是 `zip` 且 `HEAD` 都返回 64 位十六进制摘要
（reqwest 实际读到 `X-Linked-Etag`）；最小的 `connectors.zip` 经宿主重定向流式落盘后
**本地 sha256 === HEAD 摘要**（GET 响应的摘要也一致）；解压出来的东西 `looks_like_market` 认。
它**刻意不钉摘要值**——市场是移动目标，钉值会在下次发布后腐烂；钉的是契约。

> 最后那条断言正是能抓住 §9.1 ① 的那个：官方归档解出来后必须被认成市场。

### 9.7 仍未验证 / 已知风险

- ~~**只完整下载了 `connectors.zip`**（16.7 MiB）~~ → **已补测（2026-09-17 晚，改用户 config 时顺带跑通）**：
  宿主以 `zip` 注册三个默认市场，三个归档**全部**走完 `HEAD` → 下载 → sha256 校验 → 解压 → 晋升，
  `experts.zip`（289.6 MiB，解压后 610.1 MiB / 14,713 文件）在内；三个市场按序在 ~90s 内注册完成，
  **600s 外层上限余量充足**；解压后无 `download-*.zip` / `staging-*` 残留。读数见 §9.8。
- §9.3 的 Windows 长路径风险（默认装机 `LongPathsEnabled=0` 时 experts 解压落点约 292 字符）。
- `ensure_default_marketplaces` 的 **600s** 外层上限没跟着归档改大：单包比整树镜像小得多（1 请求 vs
  14,714 请求），但 289 MiB 仍然是带宽受限的。若真机首次开店超时，这里是第一个要调的数。

### 9.8 第三个真缺陷：`resolved()` 自带第二份 kind 白名单（2026-09-17 晚，改用户 config 时暴露）

`AgentStoreMarketplace::resolved()` 里有一份**手抄的 kind 白名单**：
`matches!(kind, "url" | "github" | "git" | "directory")`——**不认识 `zip`**。于是形成一条完整的静默链：

1. `ensure_default_marketplaces` 先 `resolved()` 过滤（`filter_map`）、后 `parse()`。`resolved()` 返回
   `None` → 条目在第一步就被丢掉，**`parse()` 那段「未知 kind → 告警 + `complete = false`」永远走不到**
   （§9.2 修好的分支，自 `fp-7` 起对 config 源就是死代码）；
2. `default_marketplaces` **非空**，所以 builtin 兜底不触发（兜底条件是 `is_empty()`）；
3. 三个源全丢 → `sources` 为空 → 循环空转 → **`complete = true`**：宿主报告「注册完成」，store 却是空的。

**触发面比「用户手写 zip」大得多**：`agent-store init` 生成的模板，正是从 `builtin_default_marketplaces()`
逐字写出的 `source_kind = "zip"` —— **向导产物必然得到空 store**。

**为什么三处测试都没抓住**：`builtin_marketplaces_are_complete_and_resolvable` 名字里有 resolvable，
却只断言 URL 字符串、**从没调用 `resolved()`**；`init` 两条模板测试，一条只做字符串包含，另一条虽然
`from_source()` 解析成功却只看 `[tools]` 策略。缺陷正好从三者之间穿过。

**修法**（不是「往白名单里加一行 `zip`」）：删掉 `resolved()` 的白名单，只保留「两字段都非空」，kind 判定权
收归唯一权威 `AppServerMarketplaceSourceKind::parse`——它的文档注释恰好警告过这类第二份清单的漂移。副作用是
`parse()` 的未知 kind 分支由死代码变活路径：现在写错（`zipp`）会告警 + 标记不完整 + 下轮重试，而不是静默空店。

**回归钉**：`builtin_marketplaces_survive_the_config_loader`（把 builtin 列表拼成 TOML 喂回加载器——钉的是
「释放默认值 ≡ 向导产物 ≡ 加载器接受」三者一致）、`unknown_source_kind_is_passed_through_not_dropped`、
`init::template_includes_builtin_markets` 的往返断言（原测试的字符串包含断言全部保留）。

**真机验证**（改完 `~/.agent-store/config.toml` + 重启宿主）：三个市场全部以 `source_kind = "zip"` 注册，
`enabled = 1` / `removed_at = NULL`，且 `resolved_revision` 与发布侧摘要**逐字节相等**：

| 市场 | 主机 `resolved_revision`（`X-Linked-Etag`） | 发布侧 | 条目 | 解压后 |
| --- | --- | --- | --- | --- |
| `experts` | `5cd0ab947c7ed497…` | `5cd0ab947c7ed497…` ✓ | 381 | 14,713 文件 / 610.1 MiB |
| `workbuddy-skills` | `fca2fead19d91480…` | `fca2fead19d91480…` ✓ | 262 | 4,633 文件 / 41.6 MiB |
| `connectors` | `4c35ed10007c60715…` | `4c35ed10007c60715…` ✓ | 228 | 3,263 文件 / 37.4 MiB |

**残留（登记，不修）**：`connectors` / `workbuddy-skills` 两行的 `source_etag` 仍是 `url` 时代的旧值，`zip`
分支不会覆盖它。对 `zip` 是惰性的——`fetch_remote` 的 zip 分支只读 `current_revision`、不读
`current_validators`，且返回 `validators: None`，所以 `record_source_validators` 也不会被调到。若哪天把该行
改回 `url`，这个陈旧 validator 会被当作条件请求凭证重新送出。

## 10. 明确不做

- **不删** `url` / `github` / `git` / `directory`：第三方源继续可用整树镜像。
- **不做**「索引包 + 按需拉载荷」：experts 的 611 MiB 里 8 个文件占 25.3%，理想形态是清单/元数据一个小包、载荷装的时候再拉。那是新协议面（`store/install-entry` 需要「载荷缺失」状态与按需获取），本批只把「全量镜像」变成「一次请求」，不改变安装语义。
- **不做** 老用户地址自动改写（D3）。
- **不修** §9.3 的长路径风险（登记为独立项，不是本批的必要条件）。
- **不动** `tools_updated_at` 等 `26` §10 已登记项。
