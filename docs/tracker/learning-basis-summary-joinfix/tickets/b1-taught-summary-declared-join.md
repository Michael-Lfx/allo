# b1: render_taught_summary 并网 SQL 单参 json_extract——声明概念从未进教练账面

## Status

已关票（2026-11-16 施工收口，feat/learn1116）。

## 已核验事实（2026-11-16 对库对码）

- **声明侧数据完整**：`learning_basis_declarations` 行在（doors
  G1-G5、8 blocks、45 concept_ids）；45 概念全部在登记表
  （source='basis-declaration'）；growth 两批教的 19 个 teaches 里
  9 个（变量声明/初始化/赋值语句/条件分支/布尔条件/作用域/循环结构/
  源码文件/命令行运行命令）与声明**同一 concept_id**——铸名撞名绑定
  正常，登记表无重复行。
- **断点**：`service/learning_graph.rs` `render_taught_summary` 并网段
  `JOIN ... ON reg.concept_id = json_extract(je.value)`——**单参
  json_extract 缺路径参数，SQLite 静默返回 NULL**（`json_extract(X)`
  ≠ `json_extract(X,'$')`），join 永远空 match → 并网段恒空 → 教练
  已教账面（含「视为已掌握，不要再为其安排教学」段注）从未出现声明
  概念。coach 起草与概念评审的上下文都经 `CoachContext::render` 取
  此块 → 两者都把已声明者当零基础。
- **为何掩盖**：另两处消费面（就绪账 3825、供给/语义门 3268）走
  `declared_concept_ids()`（Rust 侧 parse_anchor_row 解析，不经该
  SQL）——UI「覆盖 45 个概念」读数、assumes 就绪投影全部正常；且
  首批 assumes 全空，就绪面无差异可暴露。
- **测试盲区**：`basis_tests.rs` 只有幽灵概念负向断言
  （`!summary.contains(ghost)`——空账面同样通过），无「声明概念必须
  在账面」的正向断言。
- **同款扫描**：全库唯一一处单参 json_extract（本票），无兄弟病灶。

## Terms

- **R1（SQL 修复）**：join 键改 `reg.concept_id = je.value`（json_each
  元素标量本身即文本，无需 json_extract）。
- **R2（正向回归测试）**：`declared_basis_concept_reaches_coach_taught_summary`
  ——声明活概念 → 账面含 canonical + 「已会声明/不要再为其安排教学」
  段注；撤销锚行 → 账面收缩。
- **R3（不做）**：机械门（teaches ∩ 声明）不进本票；存量重复节点不
  回溯；ready/供给面不动（本就正常）。

## Acceptance

- `cargo test -p nomifun-learning` 全绿（含新增正向回归）。
- 真机库验证：修正 SQL 对 01a1e273 现值解出 45 个声明概念。
- 真机复验点：现有课程直接再触发一次 growth（声明行已在库，coach
  上下文即时生效），教练不再教已声明概念。

## Resolution

已施工收口（2026-11-16，feat/learn1116）。

- R1：join 键改 `je.value`，病灶行留注记（病历与机理）。
- R2：正向回归测试落 `basis_tests.rs`（含撤销收缩臂）。
- 验证：`cargo test -p nomifun-learning` 965 绿；修正 SQL 真机库解出
  45/45。
- 真机复验：待用户下次 growth 观察（教练上下文即时读库，无需重建
  课程；已生成的重复节点不回溯）。
