---
id: sb1
title: 评审退出结构 + 教练改读地形清单——i06 交换的部分回退（ADR-0018 决定 1–2）
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

评审输入的最大单块（35.5%，63 行结构块清单）按设计不参与任何判定，却被
i06 加的第 4 条判据里的结构对照拉进了门；该对照运行一个多月零产出（18 条
findings 里 1 条，且自述「本批不改档位」）。教练侧全量摊开把与已教概念同名
的块名同时摆在模型面前，是 11-16「照块名起概念名、整批打回」病历的根源。
ADR-0018 决定评审侧彻底退出、教练侧按覆盖折叠。

## 已核验事实（2026-11-24，读码；行号对应尖端 5c7be0425）

完整笔记在仓库外 `C:/Users/test/Desktop/work/allo-sb-notes/sb-facts.md`。
要点：

- **评审与教练共用同一个 `CoachContext`**：`context.render()` 生产调用点恰
  三处，全在 `learning_graph/mod.rs`——起草 :839（`coach_draft`）、重裁
  :888（`coach_redraft`）、评审 :1776（`concept_review`，在重试 loop 内）。
  一次生长只装配一次上下文（service:2232），三者复用。
- **结构措辞在 `render()` 里有两处**：块 2 块头（:684-690，字段
  `structure_reference` 为空则整块省略）与【首批纪律】首行（:714，「入口从
  结构参考的入门档起步」——花名册为空时渲染，**教练与评审共用**）。
- **第 4 条判据是一行三事**（:1048）：①难度如实申报；②结构档位对照；
  ③内容过载可附拆分建议（absorb-8 z02，专项测试在钉）。评审侧唯一的
  「如实申报」句就在①里。
- **回放臂从不给评审塞结构**：`learning_smoke.rs:4950` 构造评审上下文时
  `structure_reference` 取 `Default`（空串）。冒烟臂里给评审塞结构的夹具
  没有。
- `render_reference_lines`（`structure_view.rs:476-532`）现状**零单测**；
  行格式 `- {name}({slug})|{note}|{stage}|{spine}|{tail}`，排序键
  `(stage_rank, layer, spine_order, 原下标)`。
- 读挂载行没有现成 helper（三处内联 SQL）；`mounting::parse_mount_entries`
  与 `mounts_stale` 现成。维护侧口径：**无挂载行 = stale**。
- **UI 词典里没有「结构参考材料」**这个术语；它只出现在后端两句任务文案
  （service:2014 `set_step`、:2034 WS 帧 `text`），前端按 step 机器名查词典、
  不显示这两句。
- 评审提示词头写死「逐节点核对四条」（:948）；:1058 引用「难度诚实条」。

## R（ADR-0018 决定 1–2；用户裁决 2026-11-24，含 Q30）

- **R-a 评审侧退出**：`CoachContext` 增 `render_for_review()`——与 `render()`
  同源，跳过结构块；`concept_review`（:1776）改用它，`coach_draft` /
  `coach_redraft` 不动。一个上下文、两个渲染函数。
- **R-b 判据 4 只退役结构对照那一句**（Q30 b）：删除「节点内容与【结构参考
  材料】里同名块的档位明显背离时点名」；标题「难度诚实与阶段/档位相符」改
  为只说难度（如「难度诚实」）；①难度如实申报与③拆分建议原样保留。
  `difficulty` 仍是 `REVIEW_FIELDS` 的合法 field（不动）。findings 例子
  （:1064）去掉「结构材料可能过时」。:948「四条」保留（判据条数不变）。
- **R-c 地形清单**：`structure_view` 增 `render_terrain_list(structure,
  covered: &HashMap<String, usize>) -> String`——展开 `covered` 之外的块，
  行格式与呈现序同 `render_reference_lines`；已覆盖块折叠成一行「已覆盖
  {k} 块（{m} 个节点）：名称、名称…」。覆盖集 = 挂载条目 `blocks` 里出现
  的 slug → 挂载该块的节点数；**无挂载行 = 空覆盖集 + 清单前加一行说明**
  （「尚无节点挂载记录，以下为完整地形」）。只统计 slug 仍在当前结构里的
  条目。不做任何块名↔概念名匹配。
- **R-d 术语更名**：`render()` 块 2 块头「结构参考材料（全量块摊开…）」→
  「地形清单（…）」；:714 首批纪律「结构参考的入门档」→「地形清单的入门
  档」；:553 教练自查条款**保留**、改为「地形清单里的块名不是登记表名」；
  service:2014 / :2034 两句任务文案改「教练正在阅读地形清单与花名册…」。
  字段名 `structure_reference` 与 `CoachContext` 字段注释一并更名/改注释。
  UI 词典无需改（已核实不含该词）。
- **R-e 回放臂**：`replay_concept_review` 自身不调 `render()`、经
  `concept_review` 渲染——**零改动自然跟随** review 渲染。回放臂夹具不动
  （本就无结构）。

## Acceptance

- 单测：
  ① `render_for_review()` 与 `render()` 在**同一上下文**下对比：评审输出不
  含块头 `【地形清单` / `【结构参考材料`；去掉结构块后，其余各块逐字节相同
  （负断言钉**块头**，不钉裸子串——首批纪律改名后含「地形清单」字样）；
  ② `covered` 为空 → `render_terrain_list` 的清单主体 == 旧
  `render_reference_lines` 输出（逐字节，另加说明行）；
  ③ `covered` 含 k 块 → 展开行数 = 总块数 − k、呈现序不变、折叠行名称与
  节点数正确、顺序确定（同呈现序）；
  ④ 评审提示词（`concept_review_system()` 全开关臂）不含「结构参考材料」
  「同名块」「结构材料可能过时」（负断言）；仍含「难度如实」语义句与 z02
  拆分建议句（正断言）；
  ⑤ 既有测试同步而非删除：`mod.rs:3566`（判据④标题）、`:5553`（z02——应
  **不受影响**）、`:5836`（11-16 病历夹具，更名后断言新措辞）、`:6015`
  （块头更名）。
- `cargo test -p nomifun-learning` 全量绿（基线 1048/0 只增不减）+ fmt +
  `cargo check --workspace`。
- **真机**（冻结二进制 + dev 库，手动触发生长避开预算）：
  - 评审调用的输入字符数（live 档案 `system_chars + user_chars`）对照基线
    （20,946 / 18,864 / 17,202）下降约三分之一；教练调用输入不增；
  - 一轮生长落库，返工次数不高于基线。
- 回放臂重跑一次：reject 锚 ≥ 2/3 打回（验证判据 4 改写未伤及打回方向；
  **它测不到 R-a**）。
- 读数进 Resolution。

## 回退条件（负优化判定）

- **R-a（评审去结构块）的回退信号 = 真机**：评审打回质量出现可归因的下降
  （本应打回的挂号缺口被放行，对照基线轮的 findings/problems）→ 回退
  `render_for_review`，评审恢复带结构块。回放臂对此**无信号**（它本就不带
  结构）。
- **R-b 的回退信号 = 回放臂**：reject 锚在判据 4 改写后翻转为放行 → 回退
  判据 4 的措辞改动。
- 真机出现「教练重复提名已覆盖块」且花名册未拦住 → 收紧 R-c（折叠行展开
  名称+档位）。
- 提示词更名引起教练输出漂移（批形状/字段合规率变化）→ 回退 R-d 的提示
  词面，保留代码面术语。
