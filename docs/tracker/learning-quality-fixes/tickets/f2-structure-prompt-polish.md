---
id: f2
title: "结构生成模型面打磨——door_list 输出示例 / one_name 报全与自检 / 块数下限弹性"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# f2: 结构生成模型面打磨（三小项打包）

**What to build:** 法证发现的三处模型面摩擦：①door_list「逐字全收」被模型
误解为整行照抄（每次结构生成白烧一轮）；②one_name 违约逐轮点名打地鼠；
③blocks 31<40 下限把小目标课逼出凑数块。全部在提示词/校验器面，零迁移。

## 法证证据（e2e2/server.log）

- `06:34:59 [gd.domain.doors] 门口编号「[G1]（硬前置）能在 10 道包含整数…」
  必须是 G<数字> 形`——模型把 door_list 注入行**整行照抄**进 prerequisite_
  doors（「逐字全收——编号与清单顺序照抄」的措辞歧义；成功轮唯一一次打回
  即此项）。
- 首轮三连败指纹各异（早停未触发属正确）：`[gd.blocks.count] 31 条不足下限
  40` → `[gd.blocks.one_name] 「百分数与利率的对应」含并列标记「与」` →
  `[gd.blocks.one_name] 「底数与指数」含并列标记「与」`——第 2/3 轮同为
  one_name 但名字不同：**模型只修被点名那个，别处又冒新的**。

## allo 现状（落点）

- 提示词：`crates/backend/nomifun-learning/prompts/structure/goal-decomposition
  .md`（door_refs 条款/prerequisite_doors「逐字全收」/块数下限行）+
  `structure_prompts.rs`（占位符/条款覆盖断言）+ 金样。
- 校验器：`learning_graph/growth_contract.rs`（gd.blocks.count / gd.blocks.
  one_name / gd.domain.doors）。
- 门口清单注入：`structure_gen.rs` `door_list_markdown`（e2 落的
  `- [G1]（硬前置）判据` 行格式）。

## learnhub 参考

- `prompts/decomposition/goal-decomposition.md:32,33,45`（原条款：40–80/硬
  100、one_name、door_refs）+ `engine/decomposition/validator.rs`（他们 one_
  name 报单条还是报全——实现第一步核实；若报全则 allo 打地鼠的真因在模型
  泛化，走自检句）。

## 实施要点

- [ ] **①door_list 输出形状**：prerequisite_doors 条款补输出示例
      `["G1","G2"]`，措辞改「只写编号本身（如 G1），不带方括号/（硬前置）
      标注/判据文字；顺序照输入清单」；金样重生成 + 条款断言随批。
- [ ] **②one_name 报全 + 自检句**：核实校验器现状——若逐条报，改「同类
      违约一次报全」（全部含并列标记的块名一行一条）；提示词交卷自检句补
      「交卷前扫**全部**块名：不含『与/和/及』等并列标记，官方并列术语以
      「」包裹」。
- [ ] **③块数下限弹性（票内裁决 D1，推荐项已定可复议）**：硬下限 40→**25**，
      40 转 warn 注记（「低于 40：小目标课如实精简，密度抽验兜底」）。理由：
      E2E 两轮对照 31 条与 51 条的课程内容密度无差，硬凑 40 产出「底数与
      指数」类边缘块；learnhub 40–80 按其大课设计。契约文档
      learning-structure-artifacts.md §2 同步 + learnhub 行文差异记档。
- [ ] smoke structure 档回归（读数行不劣化）；真机判据 = 重试轮不再出现
      gd.domain.doors 整行抄写违约。

## 验收

- 红绿样：整行抄写形状被 gd.domain.doors 拒（保留）；one_name 报全断言；
  块数 25-39 过闸带 warn、<25 拒。
- `cargo test -p nomifun-learning` 全绿；金样重生成幂等；契约文档同步。

## 非目标

- 早停语义（e1 已单源化）；near-概念释义同义（雾区⑤）；传统课程线。
