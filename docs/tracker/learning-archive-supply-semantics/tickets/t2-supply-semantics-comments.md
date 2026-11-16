# t2: 决策面供给语义注释同步（P2/P3/P4/P5 塌缩后的文档面）

## Status

已关（2026-11-13 协调者内联施工，d1f04219a，fmt 干净）。

## 已核验事实（2026-11-13 对码）

- 用户裁决（ADR-0017 修正案）：知识供给面以「未删」为宽度、包含归档课。
  据此——
  - P4 `render_taught_summary`（learning_graph.rs:5177）零过滤=**期望
    宽度**（软删课无边 g3 掐、硬删课无边 t2 purge 补，course.rs:846 已核）
    ——只需注释钉住「全宽依赖 purge 单源不变量」。
  - P2 `graph_node_topology` teachers/forbidden（lesson.rs:1340/1399）
    零过滤=期望宽度（归档课供给生效）；同函数首现闭包（:1449）排除
    归档=既有仲裁设计（archived 不夺首现）——两类宽度并存是语义一致
    的，各补一行注释消解「同函数分裂」的误读。
  - P3 `course_known_names`（lesson.rs:1578）宽度正确（含归档=支撑语义），
    **注释失真**：自称「与闭包查询同律」实为不同律——修注释并引用
    ADR-0017 修正案。
  - P5 ready_partition 供给面（learning_graph.rs:4056+）宽度正确；门语义
    正确（未掌握的归档前置确实该挡）。徽标措辞留观察（map 不做清单）。
- P6 八处「未删」半宽（gate_batch/taught_ledger/concept_detail/
  concepts_overview 等）=全部正确，不升级、不触碰。

## Terms

- **R1（注释同步，全部一行式）**：上述五处（lesson.rs teachers、forbidden、
  known_names〔修错〕、learning_graph.rs render_taught_summary、
  ready_partition 供给段）各加/改一行注释，引用 `ADR-0017 修正案`，
  写明「知识供给全宽含归档（支撑其它课程）；删除课无边（purge 不变量）」。
  known_names 的失真注释替换为如实描述。
- **R2（不做）**：任何查询的过滤条件零改动；首现闭包宽度不动；
  gate_batch/taught_ledger 等八处不动（已正确）；无测试改动
  （纯注释，`cargo fmt` check 即可）。

## Acceptance

- `cargo fmt -p nomifun-learning --check` 干净；`cargo test -p
  nomifun-learning --lib` 不需重跑（注释不参与编译语义，保险起见跑一次）。
- 逐处对照：注释与 ADR-0017 修正案表述一致，无误导性措辞。

## Resolution

已施工收口（2026-11-13，d1f04219a）。

- R1 五处：lesson.rs teachers（供给全宽含归档 + purge 不变量）、forbidden
  （宽度同 teachers）、known_names（**失真注释修正**——原「与闭包查询同律」
  改为如实：deleted_at 只挡墓碑，归档课自读正名在列，跨课首现闭包才是排除
  归档的另一面，两者不同律）；learning_graph.rs render_taught_summary
  doc（全宽 + purge 不变量依赖）、ready_partition 供给表（deleted_at 只挡
  墓碑，归档已掌握前置照常支撑就绪门）。全部引用 ADR-0017 修正案。
- R2：查询过滤条件零改动（Spec 轴逐行核实仅注释行）；首现闭包/八处半宽
  未动。
- 评审轴一附带的词典碰撞顺手修：service/mod.rs 片段注释「已停用」→
  「已归档」（Archive 词条 Avoid 词，29f5475da 后的 16077f862）。
- 验证：fmt 干净；Spec 轴确认零过滤改动。
