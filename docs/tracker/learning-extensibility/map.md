# Map: 学习模块上线前持久化结构扩展性收口（x01–x21）

<!-- wayfinder:map -->
<!-- 状态：开图（2026-11-01）；grilling 裁决关闭（2026-11-02，22 问全按推荐）+ 影响面
      核查完成（无决策翻转，两处范围细化、一个顾虑消除，见裁决记档）。
      来源 = 上线前扩展性评审（三层盘点：DB schema / Rust 格式与枚举集 / API-前端契约）。
      追加裁决（用户，2026-11-02）：未发布迁移链折叠为净形基线（x21）。 -->
<!-- 基线：feat/learn0928 尖端 0c7d86467（本波立票提交）。 -->

## 迁移发布态（2026-11-01 核实；所有迁移票改法的依据）

| 段 | 状态 | 改法 |
| --- | --- | --- |
| 旧学习链 015/029/036–044/048/050–053 | **已发布**（v1.6.8 到 068；v1.7.0 到 069_marketplace）——表里有真实用户数据 | 冻结；一切改动经 **x21 基线文件**承载（重建 = 升级时搬用户数据，须 fixture 测试） |
| 本分支 069_retire…–088 | 未发布 | **x21 折叠为单一净形基线 `0NN_learning_domain_rebuild.sql`**；此后迁移票只编辑基线。号 = 折叠时 main 尖端 +1（今天 070；合并 rebase 时再校准，单文件改号零成本） |

迁移锁：`nomifun-db/src/database.rs:262-318` checksum 逐条校验。还原契约已核实
（`open_database_for_backup` + `validate_current_migration_lineage`）：bundle 必须是
**当前完整谱系**，跨版本还原本就 fail-closed——删表删列不新增还原风险；折叠后 dev 库
谱系非前缀、启动即拒，用 `bun run reset:dev`（package.json:75）重建。

## 评审结论 → 票映射

三个系统性模式（不是单点设计差，是模式反复出现）：

| 模式 | 表现 | 归票 |
| --- | --- | --- |
| A 枚举集「三处同步」 | 同一取值集被 SQLite CHECK + Rust 闭合 enum + 前端 union/`Record<>`/双 i18n 钉死；加一个值 = 整表重建 + 后端 + 前端 N 处 | x01 x05 x12 x13 |
| B JSON 文档列无版本 | 只有 source_plan 与 structure 外层有 version，其余靠 serde default 单向兼容 | x09 x14 x16 x19 x20 |
| C 哈希/水位/幂等键绑易变载体 | structure_hash 原始字节、findings_watermark 自增 id、xp anchor 字符串拼键 | x06 x07 x18 |
| — 真 bug / 死物 | 自建题 kind CHECK 回退（升级可中断 + 新建 500）；孤儿表 + 死列；前端两处显性漏洞 | x01 x02 x03 x04 |
| — 结构债 | 迁移链 20 段未折叠；FSRS 参数无算法版本；WS phase 无单源 + 嗅探双轨；Rust↔TS 双手写契约零校验；可视化三角锁死；structure 中文取值落库 | x21 x10 x11 x15 x17 x08 |

## Notes（评审 + 影响面核查关键事实）

- **可再生 vs 学习者状态**是本波分水岭（词条见 CONTEXT.md）：structure/节正文/活动/挂载
  可再生（格式演进走懒再生）；FSRS 卡、review_log、xp_events、quiz_rounds、findings
  台账、声明、概念登记表、attempts、progress 是学习者状态（只许前向兼容）。
- v3 契约哲学：无物理 FK/触发器，靠 `id_schema_contract.rs` 运行时登记。枚举集校验移出
  DB CHECK 与之一致（ADR-0015）。
- 学习表 SQL 内联在 `nomifun-learning/src/service/*.rs`（无 repository 层），枚举单源
  在 `models.rs`。全仓无 backfill 命令（x18 建第一个）。
- **影响面核查（2026-11-02）**：
  - 折叠清单实码核实：真货 = 069 级联 DELETE×14 + 旧概念表 DROP×6（lesson_concepts 是
    015 DROP → 070 新建，两步都保留）+ custom_questions 重建 + 076/078 回填；可消折腾 =
    端点表 + compass 两列（070 建 → 074 删）、071 补 status、080 aliases 改形 + ref_name、
    085 补 source、074 端点课时 DELETE（用户库无端点表，本就空操作）。
  - 「接管」无持久化标记（`service/lesson.rs:1891` 手动编辑只清 degraded）→ x19 增
    `edited_at` 标记列（勘误）。
  - tier `use` 使用面 = `learning_graph/*.rs` 8 处 + `types.ts:801`，x08 成本极低。
  - 两个现存信封均扁平（`generation/snapshot.rs:29-38`、`learning_graph/lexicon.rs:202`）
    → x09 扁平信封（票面勘误）。
  - ADR-0011：source_kb 单列↔列表双写是**刻意决定**（单列是兼容位、不退役）→ x02/x20
    引用之，不当作「未登记的不变量」。
  - ADR-0012 §4 只钉 FNV-1a 64 锚与级联语义，未钉「原样字节」→ x07 兼容，契约文档记档
    即可，不出后继 ADR。
  - scripts 不依赖迁移文件数/名（`ci-changed-paths.mjs` 仅路径示例；`reset:dev` 在）。
- 票内 file:line 来自 2026-11-01 盘点，实现会话开工先复核（f1 惯例）。

## 裁决记档（2026-11-02 grilling，22 问全按推荐）

1. **Q1 发版门槛**：x01–x09 gate（x09 收窄为「优先 1 的学习者状态五列」，优先 2/3 可滑）；
   x10/x11 尽力、可滑；x12–x20 上线后。
2. **Q2 枚举集**：正名 **Enum Set（枚举集）**（「词表」是 CONTEXT.md 禁用词），已入
   CONTEXT.md；票面全量替换，文件 slug 保留英文 vocab。
3. **Q3 代际/修订**：命名纪律入 CONTEXT.md——`version`/`*_version` 一律指代际，修订计数
   叫 revision；已发布 `version` 列（实为修订计数）不改名，x20 标 legacy。
4. **Q4 词汇**：可再生产物 / 学习者状态 两词入 CONTEXT.md。
5. **Q5 落库码**：Tier 的「DB 存 locale 无关码、展示层 i18n、解析面机械归一」推广为通用
   规则 Stored Code；tier 合一取 **apply**（EDGE_TIERS 的 `use` 是对词汇表的偏离）。
6. **Q6 pipeline**：取值 `sampling | source_plan`（词汇表「来源结构计划 Source Plan」
   对齐，不用 v1/v2）。
7. **Q7 信封扁平**（票面勘误）：与现存两信封同形——version 与载荷键并列，不包 data 层。
8. **Q8 fixture**：跑 ≤068 → 播种 v1.7.0 形状（9 种 kind、双 source、各 status）→ 跑 x21
   基线 → 断言行数守恒、无 CHECK 失败。共享助手，动已发布表的票全复用。
9. **Q9 x02**：course_jobs 直接 DROP（还原契约 fail-closed 已核实，无新风险）。
10. **Q10 x07**：保留 INTEGER hash 列 + 新增 `hash_version`，低版本按旧算法复算、相等即
    静默升级；契约文档记档，不出后继 ADR。
11. **Q11 x15**：零依赖手写守卫；schema 超过 ~10 个再议引库（CONTRIBUTING 许可核查）。
12. **Q12 x12**：不扩自建题菜单（保持 3 种）；只修 9 种 kind 的标签/渲染全集兜底；
    人工创建表单另立产品票（波外候选）。
13. **Q13 ADR**：ADR-0015（枚举集校验移出 DB CHECK，读宽容写严格）+ ADR-0016（持久化
    演进纪律）。x06/x07/x11/x18 不出 ADR。
14. **Q14 编排**：x01–x04 短分支先并回 learn0928 → x21 折叠（纯重构）→ 迁移票串行编辑
    基线 → 其余并行 → 后置组按阻塞解锁。真机臂串行。
15. **Q15 x06**：方案 B——findings 加每课程单调 `seq`，水位锚 seq（业务列随 bundle 不变）。
16. **Q16 x18**：anchor 不加版本前缀；kind 前缀即判别位，单源 parse 拒未知形状。
17. **Q17 x19**：stale_format 提示 + 用户显式触发（告知覆盖手改）；自动排队开关默认关；
    **接管节永不自动再生**——x19 增 `edited_at` 标记列（影响面核查勘误）。
18. **Q18–Q21 迁移折叠**：一个文件；净形（未发布表建终态、已发布表变换逐字保留）；纯重构
    先行（一次性等价校验，结果记 Resolution）；slug 复用 `learning_domain_rebuild`。
19. **Q22 政策**：「发布前折叠未发布段为净形基线；已发布段永不折叠/改动」入 ADR-0016 +
    `data-and-storage.md` 一行（顺手改过时的「45 个迁移」）。

## 验证总纲

1. 迁移票全部经 x21 基线承载：`cargo test -p nomifun-db`（14 个 Migrator 全链测试原样 +
   重指后的 `learning_concept_web_migration.rs` + id_schema_contract 契约）+ Q8 fixture。
2. 学习 crate：`cargo test -p nomifun-learning`；涉 ai-agent 加 `-p nomifun-ai-agent`。
3. 前端票：`bun run typecheck` + `bun run check:i18n`（动词典先 `bun run gen:i18n`）。
4. 新 check 脚本：登记 script-registry（`bun run help --check`），进 `bun run check` 链。
5. 真机：涉落库格式的票跑 `nomicore learning-smoke` 对应档确认写读闭环；真机臂严格串行。

## Tickets

| id | 票 | 档 | 阻塞 |
| --- | --- | --- | --- |
| x01 | 自建题 kind CHECK 回退修复（升级中断 + 新建 500） | **必修** | — |
| x02 | 孤儿表 learning_course_jobs + 死列清理 | **必修** | x21 |
| x03 | 复习答案渲染 number 类型 | **必修** | — |
| x04 | figureRepair 支持 mermaid 修复入口 | **必修** | — |
| x21 | 未发布学习迁移链折叠为单一净形基线（纯重构） | **必修** | x01 |
| x05 | 增长型枚举集列去 CHECK | 强烈建议 | x01 x21 |
| x06 | findings_watermark 换锚（seq 游标） | 强烈建议 | x21 |
| x07 | structure_hash 规范化投影 + hash_version | 强烈建议 | x21 |
| x08 | structure_json 稳定码统一（tier 合一取 apply） | 强烈建议 | x21 |
| x09 | JSON 文档列扁平 v1 信封（学习者状态五列优先） | 强烈建议 | — |
| x10 | FSRS 参数算法版本包装 | 强烈建议 | x21 |
| x11 | WS phase 单源 + pipeline 显式字段（sampling/source_plan） | 强烈建议 | — |
| x12 | 枚举集三处同步治理（互钉脚本 + 读宽容 Unknown + 前端全集查表） | 后置 | — |
| x13 | 多态 source 扩 quiz | 后置 | x05 |
| x14 | structure_json 扩展键命名空间 ext.* | 后置 | x07 |
| x15 | 热点 payload 零依赖宽容守卫 | 后置 | — |
| x16 | 答案 payload 自带判别 | 后置 | x09 x21 |
| x17 | 可视化 fence 注册表单源 | 后置 | — |
| x18 | anchor 单源 + review_day 重算工具 | 后置 | x21 |
| x19 | format_version 懒再生阀门（接管节排除） | 后置 | x21 |
| x20 | JSON 契约总表进 id_schema_contract | 后置 | x09 |

## 实施编排（裁决后）

- **施工图**：`blueprints/` 六份（2026-11-02，草稿待确认）——图 1 `x21-fold-baseline`（先定稿，定节号）→ 图 2 `x02-x05-baseline-schema`、图 3 `x06-x07-anchor-rework`、图 4 `x08-structure-stable-codes`、图 5 `x09-x10-json-envelopes`（引图 1 节号，可并行）→ 图 6 `x11-phase-single-source`（随 x11 排期）。后置票开工前再画图 7–12（x12+x15 / x13 / x16 / x17 / x18 / x19；x14、x20 分别回写图 4/图 5 阶段二）。
- x01–x04 短分支（基 learn0928 尖端）每票一提交，先并回 learn0928（x01 是升级中断级）。
- **x21 折叠**（一次性等价校验记档）→ x02/x05/x06/x07/x08/x10/x16/x19 **串行**编辑基线
  文件对应节。
- x09/x11/x15/x17 与迁移线并行；后置组按阻塞解锁。
- 上线后纪律（ADR-0016 已载，结波时在 CONTEXT.md 开发注记指过去）：新枚举集列不加
  CHECK；新 JSON 列必带扁平 version 信封；读宽容写严格；可再生产物走懒再生，学习者状态
  才写迁移 + 回填；发布前折叠未发布段。

## 波外候选

- 自建题人工创建表单扩到后端放行的 7 种（Q12 剥离的产品票，表单是新 UI 工作量）。

## Decisions so far

- 2026-11-02：grilling 22 问全按推荐；影响面核查无决策翻转（细化 x09 门槛、x19 增
  edited_at；消除还原顾虑）；x21 入列，枚举集正名，ADR-0015/0016 落档。
- 2026-11-02：x21 关闭——未发布 069_retire…–088 折叠为净形基线
  `070_learning_domain_rebuild.sql`（旧 20 文件删除；一次性等价校验：428 个 schema
  对象 / 28 张学习表 / 差异 0，记图纸 §8）。此后迁移票只编辑该基线对应节。
- 2026-11-02：x05/x02 关闭——基线 §10 保 id 重建七张已发布表去枚举集 CHECK（含
  progress 跨列形态），§11 退役 course_jobs；两处死列（progress.extra_json /
  courses.graph_meta_json）随重建消失；跨列规则移入 quiz/progress/settle 写路径校验
  并有单测；表数 125→124；Q8 fixture 固化为永久测试。
- 2026-11-02：x06/x07 关闭——findings 加每课程单调 `seq`，水位改锚 seq（还原重编号
  不漂移、不重跑）；mounts/repairs 加 `hash_version`，hash 改为内层 structure 规范化
  投影（v2），读侧按存量版本比较不回写（GET 不写库），重序列化不再误判 stale。
- 2026-11-02：x08 关闭——structure 落库码统一（stage/domain_type/source_tier 三组 +
  EDGE_TIERS 的 use 合一为 apply）；模型仍可吐中文，`normalize_structure_vocab` 在
  生成/修补/视图三面机械归一；信封 version bump 2；提示词与金样、前后端夹具全量换码。
- 2026-11-02：x09/x10 关闭——`envelope.rs` 扁平信封 helper 落地；优先 1 四列（quiz
  行级 doc_version / aliases 行级 doc_version / repairs ops 对象信封 / attempts
  response 对象信封）写读版本化，未知版本按列显式降级；FSRS 参数 `{algo,w}` 包装、
  未知算法整体回退、review_log 加 `algo` 列（历史 NULL = fsrs6）。
