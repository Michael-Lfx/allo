# Map: 学习模块上线前持久化结构扩展性收口（x01–x20）

<!-- wayfinder:map -->
<!-- 状态：开图（2026-11-01）。来源 = 上线前扩展性评审（三层盘点：DB schema /
      Rust 格式与词表 / API-前端契约；关键结论已逐条实码核实）。
      用户指示：评审里的所有条目全部立票。 -->
<!-- 基线：feat/learn0928 尖端 289119ae1（w1 verify_math 已并回）。 -->

## 迁移发布态（2026-11-01 核实，本波所有迁移票的改法依据）

| 段 | 状态 | 改法 |
| --- | --- | --- |
| 旧学习链 `015/029/036–044/048/050–053` | **已发布**（v1.6.8 到 068；v1.7.0 到 `069_marketplace_index_source_kind`）——表里有真实用户数据 | 冻结，只能追加迁移；重建 = 升级时搬用户数据，必须有 v1.7.0 形状的 fixture 测试 |
| 本分支 `069_retire_graph_dag…`–`088_learning_basis_declarations` | 未发布 | 原地改创建迁移；**合并 main 时整体重编号**（本分支 069 与 main 的 069_marketplace 撞号）——票内一律用迁移 **slug** 引用，不用号 |

迁移锁：`nomifun-db/src/database.rs:262-318` 逐条校验 `_sqlx_migrations` checksum，
不匹配即启动失败——已发布段任何原地改动都会让用户升级后起不来。

## 评审结论 → 票映射

三个系统性模式（不是单点设计差，是模式反复出现）：

| 模式 | 表现 | 归票 |
| --- | --- | --- |
| A 封闭词表「三处同步」 | 同一词表被 SQLite `CHECK IN(...)` + Rust 闭合 enum + 前端 union/`Record<>`/双 i18n 钉死；加一个值 = 整表重建 + 后端 + 前端 N 处 | x01 x05 x12 x13 |
| B JSON 文档列无版本 | 十几个 JSON 列只有 `course_source_plan_json` 与 `structure_json` 外层有 version，其余靠 `#[serde(default)]` 单向兼容 | x09 x14 x16 x19 x20 |
| C 哈希/水位/幂等键绑易变载体 | `structure_hash` 原始字节、`findings_watermark` 自增 id、`xp_events.anchor` 字符串拼键 | x06 x07 x18 |
| — 真 bug / 死物 | 自建题 kind CHECK 回退（升级可中断 + 新建 500）；孤儿表 + 死列；前端两处显性漏洞 | x01 x02 x03 x04 |
| — 其它 | FSRS 参数无算法版本；WS phase 无单源 + 嗅探双轨；Rust↔TS 双手写契约零校验；可视化格式三角锁死；structure 中文词表落库 | x10 x11 x15 x17 x08 |

## Notes（评审关键事实）

- **可再生 vs 不可再生**是本波的分水岭：structure/节正文/活动/挂载都有 regenerate
  路径（纯 LLM 产物，格式演进可以「再生即迁移」）；不可再生 = 用户状态
  （FSRS 卡、review_log、xp_events、quiz_rounds、findings 台账、concept_registry、
  basis_declarations、attempts、progress）——只有这些必须做前向兼容。
- v3 契约哲学：无物理 FK/触发器，靠 `id_schema_contract.rs` 运行时登记。词表校验
  从 DB CHECK 移到代码层与之一致。
- 学习表的 SQL 全部内联在 `nomifun-learning/src/service/*.rs`（无 repository 层），
  枚举单源在 `models.rs`。
- 全仓没有 backfill/rebuild 类命令；格式演进目前只靠 SQL migration + serde default。
- 票内 file:line 来自 2026-11-01 盘点，实现会话开工先复核（f1 惯例）。

## 待裁决（实现开工前 grilling）

1. **已发布表重建的 fixture 基线**：x05 对 050/042/052/036/048 的重建，迁移测试必须
   从 v1.7.0 形状种子（含 9 种 kind 行、course|custom 两源行）跑到尖端全通。
2. **x02 孤儿表**：直接 DROP vs 先 RENAME `_retired_learning_course_jobs` 观察一版再删。
3. **x07 hash 列形态**：INTEGER + `hash_version` 列（推荐）vs 改 TEXT 前缀串。
4. **x15 校验库**：先查 `ui/package.json` 现有依赖；新增依赖走 CONTRIBUTING 许可核查。
5. **x12 自建题菜单**：开放到后端放行的 7 种 vs 产品面保留 3 种。

## 验证总纲

1. 每个迁移票：`cargo test -p nomifun-db`（新增 `tests/learning_*_migration.rs`）+
   `tests/id_schema_contract.rs` 全表契约同步；涉已发布表必须含 fixture 升级测试。
2. 学习 crate：`cargo test -p nomifun-learning`；涉 ai-agent 加 `-p nomifun-ai-agent`。
3. 前端票：`bun run typecheck` + `bun run check:i18n`（动词典先 `bun run gen:i18n`）。
4. 新 check 脚本：登记 script-registry（`bun run help --check`），进 `bun run check` 链。
5. 真机：涉落库格式的票跑 `nomicore learning-smoke` 对应档确认写读闭环；真机臂严格串行。

## Tickets

| id | 票 | 档 | 阻塞 |
| --- | --- | --- | --- |
| x01 | 自建题 kind CHECK 回退修复（升级中断 + 新建 500） | **必修** | — |
| x02 | 孤儿表 learning_course_jobs + 死列清理 | **必修** | — |
| x03 | 复习答案渲染 number 类型（numeric 正确答案空显示） | **必修** | — |
| x04 | figureRepair 支持 mermaid 修复入口 | **必修** | — |
| x05 | 增长型词表列去 CHECK（随新版升级一次重建，胜过以后每次） | 强烈建议 | x01 |
| x06 | findings_watermark 换锚（自增 id → 稳定游标） | 强烈建议 | — |
| x07 | structure_hash 规范化投影 + hash 版本 | 强烈建议 | — |
| x08 | structure_json 词表统一（中文→稳定码、双 tier 合一） | 强烈建议 | — |
| x09 | JSON 文档列 v1 信封（不可再生优先） | 强烈建议 | — |
| x10 | FSRS 参数算法版本包装 | 强烈建议 | — |
| x11 | WS phase 单源 + pipeline 显式字段 | 强烈建议 | — |
| x12 | 词表三处同步治理（互钉脚本 + 读宽容 Unknown + 前端全集查表） | 后置 | — |
| x13 | 多态 source 扩 quiz（复习日志/标签/打卡接入） | 后置 | x05 |
| x14 | structure_json 扩展键命名空间 ext.* | 后置 | x07 |
| x15 | 热点 payload 运行时宽容校验 | 后置 | — |
| x16 | 答案 payload 自带判别（kind/v） | 后置 | x09 |
| x17 | 可视化 fence 注册表单源 | 后置 | — |
| x18 | anchor 单源 + review_day 重算工具 | 后置 | — |
| x19 | format_version 懒再生阀门 | 后置 | — |
| x20 | JSON 契约总表进 id_schema_contract | 后置 | x09 |

## 实施编排（建议）

- 单分支 `feat/learning-extensibility`（基 feat/learn0928 尖端），每票一提交。
- 顺序：x01–x04（必修，可并行） → 裁决点 1 → x05/x02/x06 迁移组（**串行**，同链取号） →
  x07–x11（可并行） → 后置组按阻塞解锁。
- 上线后纪律（结波时写进 `CONTEXT.md`，不单独立票）：新词表列不加 CHECK；新 JSON 列
  必带 version 信封；读宽容写严格；可再生产物走 x19 懒升级，不可再生才写迁移+回填。

## Decisions so far

（结案时追加）
