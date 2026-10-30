---
id: f2
title: "结构生成模型面打磨——door_list 输出示例 / one_name 报全与自检 / 块数下限弹性"
type: task
status: closed
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

## Resolution（2026-10-30 grilling 裁决，影响面核查后；真机结果可复议）

**裁决（用户：按推荐）：**

1. **③ 落定（票内裁决 D1）**：硬下限 40→25，25–39 转 warn（「低于 40 参考档，
   小目标课如实精简，密度抽验兜底」）；smoke structure/repair 档 OK 读数行追加
   `warnings={n}` 计数让 warn 档在验收面可见（k04「读数行在场」先例）；warn 仍
   不进重试反馈（不打回，预算让给真实违约）。
2. **② 校验器已一次报全，「报全」实施要点作废**：打地鼠真因 = 模型全量重生成时
   别处新冒并列名 → 各轮报告互异 → 指纹互异 → 早停正确不触发。② 收窄为：
   交卷自检句（扫全部块名）+ 违约消息出路指引 + 「多块同报」红绿样固化。

**影响面核查勘误（2026-10-30 实码核实）：**

- **gd.* 三条款全在 `learning_graph/structure_contract.rs`，不在 growth_contract.rs**
  （票 L33-34 笔误）：L130 常量（BLOCKS_MIN=40/SOFT=80/MAX=100）、L357-380
  count、L387-416 one_name（已一次报全）、L142-162 判据（七字符+「」豁免+词边
  「和」豁免）、L695-710 domain.doors（真机指纹出处）、L729-811 reconcile_doors。
- warn 通道先例齐备（ContractViolation::warn L40；gd.stages.ordering L575；count
  越档 L376）——无需新机制。但 warn 三处消费面不可见：不进重试反馈
  （structure_gen.rs:266-272）、不进 smoke/repair stdout（learning_smoke.rs:1173
  只印红）——裁决 1 的读数计数即为此补。
- learnhub 行号勘误：one_name 条款实为 L34（33 是 door_refs）；输出契约门条款
  实为 L46。learnhub count warn 文案模板 = validator.rs:458-464；one_name 已
  报全（L568-578）与 allo 同构——② 校验器侧与 learnhub 无差距，自检句是纯增量。
- 断言随批清单（structure_prompts.rs）：L414「40–80」、L439「逐字全收」（① 改
  措辞即动）、L420「与」（② 自检句保留字样则不断）、L440（不动则不断）。
- 围栏：FENCE_DEV_FACE 含 "gd."——提示词正文禁条款码（L47-75）；无重复段落闸
  （L508-523）：>20 字符新行不得与既有行重复。
- 金样仅 `golden/goal-decomposition.md` 一份重生成（LEARNING_UPDATE_GOLDEN=1，
  structure_prompts.rs:574-578）；金样结构体实为 42 块（L1117 注释写「40 块」
  不符，顺带修注释）。
- 测试面：count 下限边界测试现无（现仅 84 warn/104 红）——24/25/31/39 全新写；
  structure_gen.rs 既有 blocks:[] 测试（0<25 仍红）不破。
- smoke 现状：31 块课 EXIT=2（红条），③ 后红条消失且 OK 行 blocks=31 照常；
  repair 档（L3060-3072）同步放行。
- 契约文档：learning-structure-artifacts.md L127（条款行）/L142（警告级汇总）/
  L224（§6 逐字全收行文）/L5（版本 v1.7→v1.8）；learnhub 行文差异记档（40–80
  按其大课设计；allo 25 系 E2E 两轮对照 31/51 密度无差+硬凑产出「底数与指数」
  类边缘块的实证）。

施工图：[blueprints/2026-10-30-f2-structure-prompt-polish.md](../blueprints/2026-10-30-f2-structure-prompt-polish.md)。

## 真机验收（2026-10-30，quality-fixes-verify）与收口

**判据全过（status: closed）。**

- structure smoke EXIT=0：`blocks=46 warnings=0`，OK 读数行含 `warnings=`
  字段（⑥ 收口项落地）。
- **整行抄写指纹零出现**：smoke + 旅程两臂共 4+ 次结构生成，日志零
  「必须是 G<数字> 形」；全程唯一一次条款打回 = `gd.no_schedule`（无关
  条款），重试一轮自愈。
- 如实记档：本轮真机 blocks 抽签 46，25–39 warn 非零路径未踩到——由单测
  24 红/25 过/31 warn/39 warn 覆盖；下次小目标课真机时目验 `warnings≥1`。

评审修复批（`010a7d4e4`）：①蓝图要求的「整行注入行作 door 字符串 → 红」
防回归红样补钉（spec 缺口）；②缺陷回写闭环——structure-repair.md L35
「块数 40–100」未随三档同步（走查没抓到跨文件短语重复），已改三档行文 +
golden/structure-repair.md 重生成（幂等复核）+ repair 断言随批。修复批后
`cargo test -p nomifun-learning` 707 全绿。
