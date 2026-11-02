# 施工图 · x21 未发布学习迁移链折叠为净形基线

## 1. 元信息

- **目标**：把 `069_retire_graph_dag_and_concepts`–`088_learning_basis_declarations`（20 文件）折叠为一个净形基线迁移 `070_learning_domain_rebuild.sql`，最终 schema 与旧链逐对象等价，零行为变化
- **版本**：v1
- **状态**：定稿（用户确认 2026-11-02；施工按本图，变更走 §8 记录）
- **日期**：2026-11-02
- **覆盖票**：x21（x01 的修复随链带入；图 2/3/5 的后续编辑引用本图节号）

## 2. 现状摘要

- 迁移锁：`nomifun-db/src/database.rs:25` `DB_MIGRATOR = sqlx::migrate!()`；启动链 `try_init_file` → `run_migrations`（heal EOL checksum → `DB_MIGRATOR.run` → quick_check → 契约校验）。谱系判定 `inspect_supported_migration_lineage`（database.rs:262-318）：`_sqlx_migrations` 行与内嵌链**逐对 zip**（version + success + checksum，checksum 容 LF/CRLF 归一），行数多 → Err，行数少 → `UpgradeRequired`。备份还原要求 `Current`（:201）。
- 版本号来自文件名数字前缀（sqlx 0.8.6 编译期解析）；`Migrator.migrations` 字段 pub 但 `#[doc(hidden)]` semver-exempt，无 `run_to`。既有测试绕开 Migrator 的既定模式 = `migrate_range(pool, from, to)` 逐条 `raw_sql(m.sql)`（`tests/learning_course_enabled_migration.rs:14-26` 等五处）。
- **先例** `217030a7c`（2026-10-20）把旧 068–084 十七段折成 `068_learning_domain_rebuild.sql`：**逐句拼接、语序不变**，「final schema identical across all 421 schema objects」，同步改了 7 个测试文件 + 2 份 ADR + 2 份文档。本图在其上收紧为净形（A2 卡）。
- 链内容事实底稿（20 文件逐句清单、全部 CREATE/DELETE/UPDATE 逐字原文）见本会话 2026-11-02 盘点；关键锚点：069 的 14 条级联 DELETE（:22-90）+ 六张旧表 DROP（:92-100）+ custom_questions 重建（:104-156）；070 建 registry/endpoints/lesson_concepts/growth_batches + compass 两列；071 补 status；074 删 endpoints/compass + 加 structure_json；076/078 两条回填 UPDATE；080 aliases 改形 + ref_name；081 xp_events + review_events 五列；082/083/084/087/088 五张新表；085 补 source；086 加 course_source_plan_json。
- 每文件头部 `v3 contract:` 自述行（仅 069/070/071/074/082/083/084/085/088 有，其余无）。
- 表数契约：`tests/id_schema_contract.rs:146` `assert_eq!(tables.len(), 125)`（折叠不得改变表集合）。
- `memory_snapshot_fingerprint`（database.rs:434-440）对每条迁移 hash——折叠自动触发内存库模板重建，无需人工处理。
- dev 库重置：`bun run reset:dev`（package.json:75 → scripts/reset-dev-data.mjs）**不直接删数据**，写 `.dataset-v3-reset.request.json`，下次启动由 app 消费、旧数据集隔离到 `retired-datasets/`。

## 3. 总体结构

- **依赖关系**：本图产出的基线文件是后续所有迁移票（图 2 §基线节编辑、图 3 §列追加、图 5 §quiz 列变更）的唯一落点；x01 先行落 069，本图折叠时携带。
- **折叠输入 → 基线节的映射**（节号即后续票的引用地址）：

| 基线节 | 内容（原 slug） | 处置 |
|---|---|---|
| §1 | 069_retire_graph_dag_and_concepts | 逐字（含 x01 的 kind 9 值 CHECK 修正） |
| §2 | 070_concept_web_growth + 071_growth_batch_status + 080(ref_name) + 085(source) | **净形**：四表 CREATE 终态（growth_batches 带 status、registry 带 source、lesson_concepts 带 ref_name）；**消掉** course_endpoints 表+两索引、compass 两列（070 加 → 074 删） |
| §3 | 072 + 073 + 075 + 076 + 077 + 078 + 079 + 086 | 逐字：对已发布表 courses/lessons 的 ALTER ADD/索引，按原序（净新增 deleted_at/difficulty/structure_json/samples_meta_json/source_kb_ids/enabled/course_source_plan_json） |
| §4 | 076 回填 + 078 回填 | 逐字 UPDATE（作用于已发布数据，必须保留） |
| §5 | 081_xp_events + 081 对 review_events 五列 | 逐字 |
| §6 | 082 + 083 + 087 + 088 | 逐字（四张新表；图 3/5 后续在此节加列） |
| §7 | 084_quiz_rounds | 逐字 |
| §8 | 080 的 aliases_json 改形 UPDATE | **消掉**（080 改形只作用于 §2 自己建的 registry 行，用户库不存在该中间态；净形 CREATE 直接带对象形默认 `'[]'`） |
| §9 | 074 的端点课时 DELETE ×2 | **消掉**（随 endpoints 表消失；用户库无此表，本就空操作） |

- **全局不变量与禁区**：
  - 最终 schema（表/列/索引/CHECK，含 partial 与表达式）与旧 20 段链**逐对象等价**——表数 125 不变（id_schema_contract.rs:146）。
  - 已发布表的数据变换语句**逐字保留、原序排列**，一个字都不改（唯一例外：x01 已把 069 §1 的 kind CHECK 改为 9 值全集，属已裁决行为变更）。
  - 已发布迁移 001–068 一字不动；文件不重命名、不重排号。
  - 不新增依赖；不改 `database.rs` 谱系逻辑。
  - 基线文件头注明：`-- 折叠自 069_retire…–088（先例 217030a7c）；净形：未发布表建终态、已发布表变换逐字保留；各节保留原 v3 contract 自述`。

## 4. 分块图纸

### 4.1 · 基线文件本体

- **接口契约**：单文件 `crates/backend/nomifun-db/migrations/070_learning_domain_rebuild.sql`；号 = 折叠时 main 尖端最大号 + 1（main 现到 069_marketplace；rebase 若 main 长了则改号——单文件改号零成本，重跑一次等价校验即可）。slug 固定 `learning_domain_rebuild`。
- **数据形状**：§2 四表终态列清单 = 2026-11-02 盘点 B 节「新建表最终形态」（registry 含 source；lesson_concepts 含 ref_name，role/tier CHECK 与 UNIQUE(lesson_id,concept_id,role) 保留——x05 才去 CHECK，本图不动）；§7 quiz_rounds 含三条形态 CHECK 与 partial UNIQUE。
- **核心算法（伪代码）**：

```
构造基线：
  for 节 in §1..§9：
    §1/§3/§4/§5/§6/§7 → 原文件语句逐字贴入（§1 带入 x01 修正）
    §2 → 以盘点 B 的终态列清单重写四张 CREATE + 各索引；
         删去 course_endpoints/compass 相关语句
    §8/§9 → 不输出
  每节头部：`-- ==== 原 <slug> ====（v3 contract: <原文或「无」>）`
```

### 4.2 · 一次性等价校验（验收硬门）

- **接口契约**：临时测试 `tests/learning_domain_fold_equivalence.rs`（随本票提交、验收后删除）。
- **核心算法（伪代码）**：

```
seed = Q8 fixture（v1.7.0 形状：050 九种 kind 自建题、双 source 复习行、
        各 status 进度行、图课程全家桶行、course_jobs 行——即 2026-11-02
        盘点「legacy 播种清单」）
world_old = 内存库：raw_sql 依次执行 015..068 + 旧 20 文件（按原号序）
world_new = 内存库：raw_sql 依次执行 015..068 + 070 基线
断言 A：normalize(sqlite_master) 逐对象相等
        （type/name/tbl_name + sql 去空白归一；表+索引+触发器，两库各 125+N 对象）
断言 B：全部 learning_* 表 + users 逐行相等（按业务 id 列排序）
输出：两库对象数、逐表行数、差异=0 → 打印一行结论，粘进本图 §8 变更记录
```

- **失败路径**：任何差异 → 修基线（回 §4.1），不允许「差不多」；等价校验不过禁止进入施工步 4。

### 4.3 · 测试重指

| 测试文件 | 现状 | 改法 |
|---|---|---|
| `learning_concept_web_migration.rs` | 21 处 include_str!（15 legacy + 6 个 069–074 目标常量），4 个测试走中间态 | legacy 15 保留；6 个目标常量并为 1 个基线常量。四个测试改断言**终态**：图课程数据清除与旧表消失（不变）、四张新表 CHECK/UNIQUE 直接对终表插入验证；「071 给存量行补 status='applied'」用例作废（净形下无存量行，DEFAULT 随 CREATE 走）——先例同款处置（217030a7c 对 alias 测试的先例） |
| `learning_course_enabled_migration.rs:55,57` | `migrate_range(0,78)/(78,79)` 钉版本号 | 改 `(0,69)` + `MIGRATOR.run`（或 `(0,69)/(69,MAX)`，与同款测试统一） |
| `learning_lesson_source_paths_migration.rs:50,89` | `(0,77)/(77,MAX)` | 同上改边界 |
| `learning_course_source_kb_ids_migration.rs:49,76` | `(0,75)/(75,MAX)` | 同上 |
| `learning_concept_alias_records_migration.rs:33,42` | `(0,79)/(79,80)` + 回填断言 | 边界同上；080 回填中间态消失 → 改钉**记录形状契约**（先例同款） |
| `learning_course_jobs_migration.rs` | include 037，测表行为 | 本票**不动**（表仍在）；图 2 才退役 |
| `provider_models_migration.rs:37` | 版本唯一性断言 | 自动通过，跑一遍确认 |
| `channel_owner_*` / `knowledge_writeback_*` | migrate_to(20/21/30/31) | 低于 069，不动 |

- 其余 14 个 Migrator 全链测试：理论零改动，全量跑一遍确认。

### 4.4 · 周边同步

- `id_schema_contract.rs` 注释里的旧迁移号（:304/:311/:932-949/:1086/:1147）→ 改指基线节号；表/列集合零变化。
- 文档：`docs/architecture/learning.zh.md` 迁移族节（:181-196）改为「`070_learning_domain_rebuild`（学习域合并基线 v2，折叠自 069_retire…–088）」；`docs/contracts/learning-course-source-plan.md:34` 的 069 引用改基线；`data-and-storage.md` 迁移计数行已由裁决提交改掉，复核。ADR 历史文本不改。
- dev 库注记：提交信息与 `CONTEXT.md` 开发注记写明——折叠后旧 dev 库谱系非前缀、启动即拒，跑 `bun run reset:dev` 后重启一次（请求文件机制，旧数据集进 `retired-datasets/`）；smoke 临时库不受影响。

## 5. 决策卡片

### A1 · 折叠单位：一个文件、一个迁移

- **类型**：架构选型
- **决策**：20 段折为单个 `0NN_learning_domain_rebuild.sql`，单 checksum、单事务（sqlx 默认每迁移一事务），升级全成或全不成。
- **理由**：sqlx 无 `run_to`（现状摘要），多文件折叠省不了事务边界；单文件使「合并 main 改号」零成本；先例 217030a7c 同构。
- **代价**：约 700 行单文件；用户升级在一个事务里重建多表，失败无部分进度（但失败=不写任何东西，回滚干净）。
- **推翻条件**：单文件超过 ~1500 行或含必须分事务的超大表重建（本链无）。

### A2 · 净形 vs 逐句拼接

- **类型**：算法
- **决策**：未发布表的建-改-删折腾写终态（§2/§8/§9），已发布表变换逐字（§1/§3/§4/§5/§6/§7）。
- **理由**：拼接会把「建 endpoints 再删、加 compass 再删、aliases 先字符串后对象」的死折腾永久冻进发布链，每个读 schema 的人都要解释一遍；净形由 4.2 等价校验兜住转录风险。先例选拼接是求快，本链是发布前最后一次折叠，值得多花一小时换净形。
- **代价**：§2 需手工合并四个文件的列定义（转录错误风险）——由等价校验断言 A 全量兜底；中间态行为（如「071 给存量批行补默认」）随净形消失，两个测试用例作废。
- **推翻条件**：等价校验两次以上因转录出错返工时，退回逐句拼接（先例路径，机械安全）。

### A3 · 基线号取「合并时 main 尖端 + 1」

- **类型**：接口契约
- **决策**：号不在本票定死为 070，而是「落地 rebase 时 main 最大迁移号 + 1」；slug 固定。
- **理由**：main 的 069_marketplace 已占 69，且 main 还在走版；文件名数字前缀即 sqlx 版本号（现状摘要），撞号=启动失败。
- **代价**：rebase 时可能要改一次文件名并重跑等价校验（分钟级）。
- **推翻条件**：main 冻结发版分支后定稿号，不再动。

### A4 · 等价校验一次性、不留永久测试

- **类型**：失败模式
- **决策**：临时测试随票提交、结果记本图 §8、验收后删除；不留「新旧链双跑」的永久测试。
- **理由**：旧 20 文件删除后无 include 来源，留档等于在 tests/ 里养一份迁移链拷贝；等价性在折叠合入后即成既定事实，永久测试无再触发场景。
- **代价**：事后无人能重跑校验——靠 §8 记录的输出存证。
- **推翻条件**：同波后续票（图 2/3/5）再动基线时若需要回归比对，届时以「基线 vs 上一版基线」建轻量 schema-diff 测试。

## 6. 施工顺序

| # | 施工步 | 图纸位置 | 验收 |
|---|---|---|---|
| 1 | x01 已并回（前置）；从 069 现状拷出 20 文件内容 | 4.1 | `git status` 干净，分支基于 learn0928 尖端 |
| 2 | 写 `070_learning_domain_rebuild.sql`（§1–§9） | 4.1 | `sqlx::migrate!` 编译通过；文件内无 course_endpoints/compass/080-UPDATE 字样（grep 零命中） |
| 3 | 删旧 20 文件；写临时等价测试并跑 | 4.2 | 差异=0 输出粘进 §8；`cargo test -p nomifun-db` 全绿（含表数 125 契约、版本唯一性） |
| 4 | 重指 7 个测试文件 | 4.3 | 各测试单跑通过；`grep -rn "migrate_range\|include_str" tests/` 无 069–088 旧文件名残留 |
| 5 | 契约注释、文档、dev 注记 | 4.4 | `rg "069_retire|070_concept_web|071_growth|08[0-8]_" docs/ crates/ --glob '!migrations/**'` 仅剩历史 ADR 与 tracker |
| 6 | 删临时等价测试；提交 | A4 | 提交内不含临时测试文件；`cargo check --workspace` |

## 7. 未决事项

- **基线最终号**：影响 4.1。推荐默认 070（main 现 tip 069）。触发重议：rebase 时 main 迁移尾号变化。
- **`learning_course_enabled` 等四个测试的边界风格**：`(0,69)+MIGRATOR.run` vs `(0,69)/(69,MAX)`。影响 4.3。推荐统一 `(0,69)/(69, i64::MAX)`（与 lesson_source_paths 现有风格一致、不引入第二模式）。触发重议：无（纯风格，实现时定死一种）。

## 8. 变更记录

| 日期 | 类型 | 位置 | 变更 | 根因/触发 |
|---|---|---|---|---|
| 2026-11-02 | 修订 | 全图 | v1 成图：22 问 grilling 裁决 + 2026-11-02 六路读码底稿 | — |
| 2026-11-02 | 修订 | 4.3 | 测试重指清单从「1 文件 6 处」扩为「7 文件」：course_enabled/lesson_source_paths/source_kb_ids/alias_records 四个测试把版本号 75/77/78/79/80 钉进了 migrate_range 边界 | 读码底稿 D 节；成图走查补上了这层耦合 |
