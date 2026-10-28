---
id: n2
title: "内容边界波 grilling——known_names 口径 / R12 收窄 / k03 语义换血 / 先例 / 档位行 / 传统线 / smoke 判据"
type: grilling
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# n2: 内容边界波 grilling（C 线前置裁决）

**What to decide:** #177/#178/#180 移植前的七个裁决点。每点带推荐项；连续两轮
盘问未获回答时按推荐项默认裁决并票面标注可复议（二/三轮先例）。

## 裁决点

### D1. known_names 已教半边口径：本课程图 vs 跨课程全局账

learnhub = **当前课程图**全部非 archived 节点 teaches（`node_ctx.rs:280-288`，
非祖先闭包）∪ 跨空间声明。allo 现成 `taught_ledger`
（learning_graph/mod.rs:972-983 + service/learning_graph.rs:2950-2968）是
**跨课程全局**账（growth 教练块 6 在用，与「跨课程就绪」既有裁决自洽）。

- **推荐：跟 learnhub 严格口径**——本课程图已教 ∪ 本课程声明（m2 锚表天然
  course_id 维度）。构件词边界是课程内部教学叙事的纪律；跨课程概念的合法性
  已由 assumes 前置路径单独承载，不让「他课教过的名词」无声进入本课正文。
  taught_ledger 全局口径留作复议项记录。learnhub 声明半边跨空间 vs allo
  per-course 的差异随裁决一并记档。

### D2. R12 收窄是否拆独立票先行

两处旧措辞（`generation/lesson.rs:801` EN、`lesson_content_loop.rs:65` CN）
是 learnhub 病历同款活性门，且收窄**不依赖** known_names（先收窄即降险）。

- **推荐：不拆票**，但 n3 内部排序 **先收窄后注入**（同票两步、同一冒烟
  验证、同批金样重生成）。拆票的协调成本 > 收益。

### D3. k03 capacity_note 尾句语义换血

`lesson_content_loop.rs:93`「明显超容才写、只留痕、不改安排、不压缩讲解深度」
vs #178「超预算**先砍雄心再砍内容**（减题/并节/降示例密度），teaches 覆盖
不可砍；收敛不住 = 节点意图超载，capacity_note **必写**指名原因（est 复议
信号）；预算内收敛 = 整键省略」。

- **推荐：整体换 #178 语义**（k03 是 #154 前身，learnhub 已把教训走完一轮）。
  形状面不动（normalize_capacity_note / 非空收下）；审计观察条文案
  （lesson_draft.rs:997-1005）与 tracing 透传同步改写；allo 机械活动数上限
  （≥3、AI-graded ≤3）保留不与条款冲突。「est 缺席不虚构预算」照搬
  （allo est 列 NOT NULL 恒注入，此句为版本臂兼容句）。
  capacity_note 出现率→m10 复议读数的自动消费留雾区，本波只留观测读数。

### D4. 意图画像缺角：teaches 档位行补不补

#178 画像判定输入 = 时长预算 × **teaches 档位** × 概念首现 × assumes；allo
图路径提示词**任何地方读不到本节点 teaches 档位**（GraphConceptFaces 不带
tier），而写作纪律 lesson_content_loop「档位决定内容形态」已引用该值——
模型在猜。

- **推荐：补**。图线在节点块（或概念面段）加档位行（数据在
  `learning_nodes.tier`/概念挂接 role，装配点 lesson_user_text :591-597）；
  顺带修「纪律引用模型不可读值」的真缺口。传统线无 tier 概念，画像条款
  传统线降级为三角（预算×首现×assumes）或豁免（见 D6）。

### D5. 先例（precedents）口径：祖先闭包 vs 教师反查

learnhub = 本点 teaches∪assumes 概念 × **祖先闭包**（reached）内教出它的
节点 + 正文节标题；首现概念不入、声明-only 不入、文件缺席 = 空 sections 仍入。
allo `first_appearance_set`（service/lesson.rs:1897-1929）已算祖先闭包但
**丢弃节点归因**；教师反查（graph_node_topology :886-915 变体）现成但口径
跨课程、且只查 assumes 概念。

- **推荐：扩展 first_appearance_set 带节点归因**（返回 ancestor_taught 的
  concept→节点映射），限本课程；跨课程不作先例（前置路径已承载跨课程视角）。
  先例行含先行节标题（复用 taught_sections_for_lessons）；正文全文读取
  （read_prior_content 工具）入雾区不实施。

### D6. 传统课程线处置

传统线（course_outline）无概念挂接/概念面——名单类条款（构件词边界、取材池）
**无锚可挂**（ADR-0009 下线了 per-course 概念契约）。

- **推荐：传统线只收两样**——R12 收窄措辞（`generation/lesson.rs:801`，
  自然语言兜底，无名单依赖）+ 自足条款（规划即雄心、est 双向轻项句、
  练习形态按档位的「不引入未教方法论」半边——这些不需要概念名单）；
  取材池/构件词边界/意图画像留档豁免（写明「名单类条款传统线无锚」）。
  不为传统线重建概念体系。

### D7. smoke content 判据升级到哪一档

allo content 档现仅 1 硬判据（管线走通）。learnhub smoke-content 6 判据：
①两面 prompt 含「课程已知概念」行且命中布景声明/已教概念 ②条款在场
（plan 四条款 + write 两条款）③契约面全绿 + est 铸值==节点 est ④落位非空
⑤先例摘要 + 先行节标题 ⑥（#179 入册链，本波无）。#177/#178 教训 = 回放对
输入面修复结构性失明；allo 已有机械 grep 通道先例（budget_line_calls
learning_smoke.rs:2326-2346）。

- **推荐：①②③⑤全上硬判据进 EXIT**（stale-binary 必红），超纲专名抽查
  （论域/反例/P(x)/实数 类比 allo 布景）维持软读数 + 对照卡人读清单
  （learnhub 用户定调同款：超纲面无机械门）。

## 无争议直采项（不盘问，随 n3 落地）

- known_names 行格式与空名单省行、BTreeSet 稳定序（facade.rs:207-238 原样）。
- 构件词边界 / 表述工具在场收窄 / 规划即雄心 / 取材池+多样轮换 / 练习形态
  按档位条款**原文域中立移植**（见 n3 Reference）。
- est 双向轻项句挂生长提示词 est 行尾（learning_graph/mod.rs:405）；两仓
  est 基线数字不同（allo 5-30/60 vs learnhub 15-30/60），**不随条款改数字**。
- 布景声明链：allo smoke content 布景改造对齐 learnhub（声明档案 + 先行
  节点正文两节）——布景重写不影响生产面。

## 验收

- 七裁决点全部落 Resolution（推荐项默认裁决须标注「可复议」）。
- 裁决与既有词条冲突时同步 CONTEXT.md（如「已知集」新词条）。
- n3 票面按裁决更新后才能开工。

## Resolution（2026-10-28 关票，grill-with-docs 单轮 18 问全按推荐落定）

- **D1 → (a)**：Known Names = **本课程图全部节点 teaches ∪ 本课程 m2 声明**
  （learnhub 严格口径，课程词汇域、无进度过滤——课程内先后由首现纪律管）。
  行名沿用「课程已知概念」。taught_ledger 跨课程全局口径记为可复议项。
  **术语正名**：Known Names（内容边界，本课程）≠ 已知集（Declared Known
  词条的就绪投影，跨课程）——两词两物，CONTEXT.md 新词条已落。
- **D2**：R12 收窄不拆票；n3 内部**先收窄后注入**（同票两步、同一冒烟、
  同批金样重生成）。
- **D3**：k03 capacity_note 尾句**整体换 #178 语义**（先砍雄心再砍内容/
  teaches 覆盖不可砍/收敛不住必写指名原因/预算内整键省略）；形状面
  （normalize_capacity_note）与 allo 机械题量上限不动；审计观察条文案
  同步。术语「节点意图超载」进 CONTEXT.md。
- **D4**：**补 teaches 档位行**——图线节点块从结构快照 `teaches[].tier`
  （TIERS=know/apply/teach，growth_contract.rs:16）渲染档位；顺手修
  「写作纪律引用模型不可读值」缺口；传统线不适用。
- **D5**：先例 = **扩展 first_appearance_set 带节点归因**（祖先闭包，限本
  课程）；先行节标题复用 taught_sections_for_lessons；跨课程不作先例；
  read_prior_content 工具轮留雾区。术语「先例」进 CONTEXT.md。
- **D6**：传统线只收 R12 收窄措辞 + 自足条款（规划即雄心/est 双向句/练习
  形态「不引入未教方法论」半边）；名单类条款豁免留档，不重建概念体系。
- **D7**：smoke content 硬判据 ①输入面名单行 ②条款在场 ③契约面+est 铸值
  ⑤先例行 全进 EXIT（stale-binary 必红）；超纲专名抽查留软读数+对照卡。

无争议直采项（构件词边界/表述工具在场收窄/意图画像/取材池/练习形态/规划即
雄心条款原文域中立移植；est 双向句数字不动；smoke 布景对齐声明档案）随 n3
执行。
