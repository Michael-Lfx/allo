---
id: pg2
title: 出处门 Stage 1c——首批参考面减去已会声明块（不裁未声明候选，不裁结构）
type: task
status: open
assignee:
blocked_by: [pg1]
part_of: map.md
---

## Question

本仓已会声明（m2）只铸概念、不落节点，并把声明概念按满档并进已教账
（`service/learning_graph.rs:3649-3668`）。但首批生长（花名册为空）的
**参考面**没有减去已声明块：`learning_basis_declarations.blocks_json`
存了声明块 slug（`:6335` 附近），消费者只解析 concept_ids，块半边一律
丢弃（`:4810`、`:5889`、`:6008`）。结果：学习者勾选「这块我会了」，
首批仍把它当候选地形展开，只靠提示词一句「标可自检的门口已具备的格子
不重教」（`learning_graph/mod.rs:741-744`）软挡——这是 prose，不是机械
面。

隔壁 Stage 1c（learnhub #290）把同一刀落成机械减法：首批参考面 =
结构参考块 − 本课已声明块（slug 精确匹配），且**不级联裁剪**未声明块
（ADR-0015 分界：过滤输入是学习者自报事实，不是结构起点）。

## 已核验事实（2026-11-26 对码）

- 声明块已落盘：`blocks_json = serde_json::to_string(&block_slugs)`
  （`service/learning_graph.rs:6335` 附近；写入
  `learning_basis_declarations`）；`target_rows`（`basis_declaration.rs:149`）
  已按「块 door_refs 全部命中勾选」算 target blocks。
- 首批地形清单在 `build_coach_context` 装配：
  `service/learning_graph.rs:3299-3338` → `render_terrain_list(structure,
  &coverage.covered, note)`（`structure_view.rs:644-668`）；声明块无节点
  → 不在 `covered` → 永远展开，正是首批会邀请教练重教的面。
- 首批判定只在渲染面用 `roster.is_empty()`（`mod.rs:759-791`）；管线
  没有首批分支。
- 提示词两条软约束：`:744`「标可自检的门口已具备的格子不重教」；
  `:761` 空账 ⇒ assumes 全空（机械）。声明块目前只影响后者（概念并进
  账），不影响参考面。
- 结构参考的不变量：参考不裁剪候选面、不做判定（`mod.rs:560-580`、
  `structure_view.rs` 头注）；本票只减「学习者自报已会」的块，不按
  结构依赖/深度裁任何未声明块。

## 影响面（2026-11-26 对码）

### 生产面（4 个文件）

| 文件 | 触点 | 改动性质 |
| --- | --- | --- |
| `learning_graph/structure_view.rs` | `render_terrain_list`（`:644-668`）、`terrain_rows`（`:582`） | 加 `declared_blocks`/`first_batch` 参数或在调用前过滤；空集逐字节同旧 |
| `service/learning_graph.rs` | `build_coach_context` 地形清单（`:3299-3338`）、`CoachContext` 构造（`:3367`）、声明行读取（`:5837-5897`） | 新增「本课声明块 slug 集」读取；首批传入 |
| `learning_graph/mod.rs` | `CoachContext` 字段（`:643-665`）、九块渲染的块 2/首批纪律（`:733-744`、`:759-791`） | 新增字段（Default 兼容夹具）+ 注记行 |
| `learning_graph/basis_declaration.rs` | `target_rows`（`:149-163`） | 只读复用，不改 |

### 测试面

- `structure_view.rs` 地形清单测试 7 个调用点（`:913-1004`，生产 1 处在
  `service/learning_graph.rs:3326`）——签名变更全部要改；新增「声明块
  剔除 / 未声明保留 / 反级联 / 空集同旧 / 无 slug 不误删」5 个用例。
- `mod.rs` 的 `CoachContext` 字面量约 15 处（`:6104-6383`，多数带
  `..Default::default()`/`..full.clone()`）——新字段用 Default 即可；
  首批/非首批渲染测试各补一条。
- `service/basis_tests.rs`：`basis_declaration_and_block_semantics`
  （`:389`）已有块语义；补一条「声明块 slug 可被读取且只影响首批」。

### 不受影响

- **UI**：地形清单是教练 prompt 块，结构页用的是另一套 `StructureView`
  渲染；零 UI 改动。
- **DB/迁移**：`blocks_json` 已在 `learning_basis_declarations`；只读，
  无迁移。
- **结构参考不变量**：未声明块全量保留，前置块被声明剔除也不级联
  （测试钉住）。
- **门口清单/覆盖读数**：本票不筛 `terrain_doors`/`coverage_gauge`；
  「可自检」提示保持。

### 风险

- **精确字符串测试**：`FULL_TERRAIN` 等断言在声明集为空时必须逐字节
  不变——先加空集旁路，再加过滤。
- **只减首批**：花名册非空后地形清单回到旧形（声明块仍可能展开，靠
  已教账与「不重教」软句）；若要全相位减，需另裁（不在本票）。
- **声明块 slug 空值**：无 slug 的块永不被命中（与 learnhub 同口径）。


## 方案

1. 取本课声明块 slug 集：从 `learning_basis_declarations.blocks_json`
   读本课最新声明行（现有解析函数 `parse_anchor_row` 已解析三元组，
   只是块半边被 `_` 丢弃——改成真消费）。空集 = 零行为。
2. 把 `declared_blocks: &BTreeSet<String>`（slug）传进
   `build_coach_context` → `render_terrain_list`；只在 `roster.is_empty()`
   （首批）时过滤 `TerrainRow.slug ∈ declared_blocks` 的行。
3. 过滤不级联：未声明块全量保留，即使其前置块被声明剔除；结构标注
   （前置块名）原样。无 slug 的块永不被命中。
4. 可读性：在首批地形清单开头加一行模型面注记（不落用户 UI）：
   「已会声明块（本批不装）：A、B——其概念已在已教概念账，不要为它们
   生成学习节点」。注记只在有声明块时出现；空集逐字节同旧。
5. 不做的事：不改 `terrain_doors`（门口「可自检」提示保持）；不改
   coverage gauge；不改声明管线；不加 `subsumed/declared_scope`（Stage 2）。

## Acceptance

- 单测（结构 fixture + 声明 slug 集）：
  - 首批：已声明块行不出现在地形清单；未声明块逐行保留；
  - 未声明块的前置块被声明剔除时，该块仍保留（反级联锁）；
  - 非首批（花名册非空）逐字节同旧；
  - 声明集为空逐字节同旧；
  - 无 slug 块不被误删。
- 断言渲染串：声明块名不出现、未声明块名出现、注记行只在有声明时出现。
- `cargo test -p nomifun-learning` 全绿；`cargo check -p nomifun-learning`。
- 真机/冒烟（可选，按 pg0 后的成本定）：同一从零课「不勾声明」vs
  「勾一块」两臂，勾选块零节点、首批从声明地板之上起步；不勾臂行为
  不变。若不做真机，票面须写明欠账。
- 无 DB 迁移；无 UI 改动；`blocks_json` 旧行解析兼容。

## Out of scope

- Stage 2 的 `declared_scope/subsumed/composed`（声明块内细词的作用域
  承接）——pg0 读数决定是否另开票。
- 结构起点/深度水位/候选面裁剪（ADR-0015：结构只参考）。
- 已会声明撤销后的回滚语义（现状：概念不灭、只删锚行）——不动。
- 门口清单/覆盖读数的声明感知呈现——另议。
