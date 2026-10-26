---
id: m2
title: "已会声明通道——勾选即已会、只铸概念不落节点（ADR-0032 域中立移植）"
type: task
status: open
assignee:
blocked_by: [m1]
part_of: ../map.md
---

# m2: 已会声明通道（ADR-0032 移植）

**What to build:** 学习者对已具备的入门内容勾选声明「已会」：系统按声明铸概念
（走既有铸名纪律），**不落学习节点、不排课、不计 XP**；已知集 = 已教 ∪ 声明，
下游节点的 assumes 就此就绪、教练教新与「当前可学」徽标同源。吸收 learnhub
#164 终态（ADR-0032）+ #161 的投影面 + smoke-basis-flow。

> **正名（2026-10-26 grill 定案，CONTEXT.md 词条已立）**：本机制用户面一律叫
> **「已会声明」**（Declared Known）——「基础声明」已被建课向导的自由文本输入
> （`learner_basis`）占用，两词两物。路由 slug 与 smoke 档名可留 `basis`
> （代码面命名），用户可见措辞（按钮/确认文案/徽标/i18n）一律「已会声明」。

## allo 现状

- **无任何已知概念输入**（grep 零命中）：`learner_basis` 是自由文本
  （`GenerateCourseRequest` :62，进地形提示词 :150-158），地形产物
  `learner_basis_assumed` 是回执字符串不是概念清单。
- 铸名纪律现成：`defnear`（近形/定义归一化/邻居候选集，j04）+ 评审 reuse[]
  裁决 + `register_alias_tx` 登记单点；概念登记表跨课程
  （`learning_concept_registry`，迁移 068:187-203）。
- 已教账：`taught_ledger()`（`service/learning_graph.rs:2946-2950`）——
  concept_id → 最高已教档位，跨课程只计存活课程，**计算投影非表**。
- 教练上下文块 9 = 登记表节选（mod.rs:479/:563-565）；块 3 = 前置地形门口。
- smoke 面：11 档现成（含 k02 maintenance），观测存档先例齐全。
- 迁移下一枚 **071**。

## learnhub 参考（ADR-0032 + #164 + #161 + d8d41d2）

- `docs/adr/0032-basis-declaration-as-known.md`：L18 只铸概念零图写（无节点/
  XP/评审卡/教练队列/挂载）；L19 已知集 = taught ∪ declared；L20 basis.yaml
  幂等锚；L22 生长提示词带基线声明段；补充 1 = 已知集按需查询不整表注入
  （→本仓雾区 #168）；补充 2 = 声明产物按**最小可教单元**铸造、上限 12→60。
- 数据与管线：`domain/basis.rs`（BasisEntry{at,doors,blocks,concepts}，
  load 缺席=空合法、append 读改原子写 L51-57、declared_doors/declared_concept_ids
  L60-80）；`engine/basis_declaration/facade.rs`（run L111：doors ⊆ 地形门口
  校验 L136-149 → 幂等增量 = checked − declared 空则 noop 零 LLM L151-171 →
  命中块 AND 语义 target_rows L342-367 → 一次结构化 LLM 调用
  {concepts:[{name,slug,definition,source}]} L212-227 → 走命名门铸造带
  [ORIGIN_DECLARATION] 标记 L302-339 → 铸造账 Laid L250-266 → append 锚 L269-278）。
- 已知集并网（506d107）：`domain/readside.rs` taught 投影并上声明概念
  （解析失败=Broken），下游节点 assumes 就绪、教练与「当前可学」同源；测试
  `declared_basis_concept_makes_downstream_node_ready_without_any_teaching_node`。
- 提示词：`prompts/basis-declaration/basis-declaration.md:22`「一物一名，按
  最小可教单元…禁止把块名整体铸成一条概念」、:31（concepts 1–60 条）。
- 前端：e06b3cb gate-declare.tsx（按钮「按勾选声明已具备(免学)」+ 确认文案
  「不生成学习节点、不安排学习、不计 XP」）；c2b1376 概念库来源徽标 + 自检
  回显已声明/已覆盖。
- 冒烟：`src-tauri/src/bin/smoke-basis-flow.rs`（d8d41d2，251 行）。

## 实施要点

- [ ] **迁移 071**：`learning_basis_declarations(id, course_id→learning_courses
      Cascade, doors_json, concept_ids_json, declared_at, note)`——一行一次声明
      （幂等增量语义 = 累计并集差）。**锚表即铸造出处**（allo 无 mint-log，
      map 对账事实①）：每行记录本次声明铸出/复用的 concept_id，概念库可反查
      「来自哪次声明」。v3 契约四处注册（PRODUCT_TABLES/UUIDV7/引用/索引）。
- [ ] 声明管线 `run_basis_declaration`（service 层，guest 学习域编排先例）：
      doors ⊆ 地形门口校验 → 幂等增量（空 = noop 零 LLM 零写）→ 命中块 =
      door_refs 全落勾选集的块（m1 AND 语义）→ 一次结构化 LLM 调用产概念
      清单（最小可教单元条款 + 1..60）→ **走既有命名门**（defnear 邻居候选 +
      定义归一化直定宿主；歧义多候选走评审 reuse 通道或如实失败——实施时按
      j04 语义定，不得绕过登记单点）→ 写锚行。铸造概念带来源标记
      （`learning_concept_registry.source` 列已存在，068:758——用
      'basis-declaration' 值）。
- [ ] 已知集并网：`taught_ledger()` 投影并上声明概念（声明无档位——就绪语义
      按「已会」处理，教练块 9 / 下游 assumes 解析 / 概念 Tab other_courses
      读数同源）；**声明解析失败（概念已删）= Broken 如实降级不炸**。
- [ ] 端点 + WS：`POST /courses/{id}/basis/declare` + `GET .../basis`（读数：
      已声明 doors/concepts/命中块）+ 撤销（删锚行；**概念留池**——登记表
      全局有效，撤销只收缩本课已知投影、来源标记保留不抹；2026-10-26 grill
      定案「声明可撤、概念不灭」）。
      开关 `NOMI_LEARNING_BASIS_DECLARATION=off`（off = 端点 409 如实拒绝）。
- [ ] 前端：结构 Tab 或概念 Tab 加声明入口（勾选门口 → 确认文案三后果如实：
      不生成节点/不排课/不计 XP）+ 概念行来源徽标（basis-declaration 来源）
      + 已声明回显与撤销。i18n 双语。
- [ ] **smoke 新档 `basis`**（对照 learnhub smoke-basis-flow）：temp SQLite 建
      图课程 + 结构（含地形门口与 door_refs）→ 声明 → 硬判据：概念登记表
      增行且 **零 learning_nodes/learning_lessons/growth_batches 写入**（计数
      守恒）、下游节点就绪投影翻转、幂等重跑 noop 零 LLM、撤销后投影复原、
      off 臂端点拒绝。无凭据时声明调用如实非零。
- [ ] 双臂对照（开关臂）：同课程 fixture，off/on 各跑——对照卡：登记表行数 /
      节点数 / 下游就绪节点数 / 声明概念数并排；人读抽判声明概念粒度（是否
      最小可教单元、有无块名整体铸成一条）。

## 验收

- **单测（红绿）**：doors 越界打回；幂等增量（重复声明同一门口 noop）；AND
  块命中（部分 door_refs 命中不算）；命名门集成（撞已教概念 → 复用入锚不重
  铸）；已知集并网（声明概念使下游 assumes 就绪——learnhub 同名测试移植）；
  Broken 降级；撤销语义。
- **db 测试**：迁移 071 + v3 契约全绿。
- **smoke basis 档**：上述硬判据 EXIT=0；观测存档含声明调用档案。
- **对照卡**：两臂并排读数落 Resolution（只跑一臂不算关票）。
- `cargo test -p nomifun-learning` + `-p nomifun-db` 全绿；typecheck/i18n/check。

## 非目标

- 门口物化/定向生成批次（ADR-0031 learnhub 已退役，不吸收）。
- #168 按需查询工具轮（雾区；本票已知集仍走既有投影注入）。
- mint-log 全量铸造账（雾区；锚表承载声明出处即够）。
- 节点级「自报已学/跳过」UI（物化时代产物，声明时代无此面）。

## 备注

- 粒度证据（learnhub 真机）：11 块被 12 条上限压成主题级 → 后续批次概念平均
  40 字且多主题——上限 60 与最小可教单元条款是同一条教训的两半。
- 声明是**课后动作**（结构生成之后勾门口），不改建课向导输入面；结构重生成
  后 door_refs 变化的声明一致性 = 幂等增量天然消化（doors 不在勾选集即失效）。
