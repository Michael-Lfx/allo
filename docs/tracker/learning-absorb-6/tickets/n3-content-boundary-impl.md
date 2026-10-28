---
id: n3
title: "内容边界波实施——#177 known_names 超纲围栏 + #178 前置规划 + #180 先例连续性 + smoke 判据升级"
type: task
status: open
assignee:
blocked_by: [n2]
part_of: ../map.md
---

# n3: 内容边界波实施（C 线）

**What to build:** 图路径课时生成输入面补三块材料（课程已知概念边界行、意图
画像四角、先例摘要行），提示词条款域中立移植（构件词边界/表述工具在场收窄/
意图画像与容量预算/取材池/练习形态按档位/规划即雄心），k03 capacity_note
语义换血，smoke content 判据升级到 learnhub 6 判据档。零迁移（纯提示词 +
计算投影）。**按 n2 裁决执行**；实施顺序先收窄（R12）后注入（名单）。

## allo 现状（落点地图，file:line 为准）

- **图线输入装配**：`lesson_user_text`（nomifun-ai-agent/lesson_content_loop.rs:
  562-669）装配序 = 课程名 → 图节点块（goal/degree :568-572）→ k03 时长预算行
  :575-579（`render_est_budget_line`，开关 NOMI_LEARNING_EST_BUDGET）→ 节点
  i/N :591-597 → 前置路径 :598-605 → 后续节点 :606-613 → 概念面段 :616-618
  （GraphConceptFaces.render_text，lesson_draft.rs:197-234）→ forbidden
  :641-645 → adjacent_context → grounding。
- **已知集构件**：本课程 teaches 投影需新写（图内全节点 teaches ∪ m2 锚表
  concept_ids——072 迁移，feat/learn-absorb-5）；概念 id→canonical 显示名
  JOIN 先例在 graph_node_topology（service/lesson.rs:866）。
- **先例构件**：`first_appearance_set`（service/lesson.rs:1897-1929）祖先闭包
  不动点，**返回值丢弃 ancestor_taught 的节点归因**（n2 D5 裁决扩展点）；
  先行节标题复用 `taught_sections_for_lessons`（:1461-1486）；正文列 =
  learning_lessons.summary（015）+ learning_lesson_sections（050）。
- **R12 旧措辞两处**：`generation/lesson.rs:801`（传统线 EN）+
  `lesson_content_loop.rs:65`（图线 CN，CONTENT_DISCIPLINE_V2_CN）。
- **k03 面**：预算行常量 lesson_draft.rs:51-53；capacity_note 尾句
  lesson_content_loop.rs:93（#154 语义）；normalize lesson_draft.rs:58-62；
  审计观察条 lesson_draft.rs:997-1005；tracing 透传 service/lesson.rs:1498-1505。
- **传统线**：section_outline_system（generation/lesson.rs:831）+
  CONTENT_DISCIPLINE_BLOCK :770 + V2 :798；单节重写两线共用
  build_section_rewrite_prompt :605。
- **smoke content**：commands/learning_smoke.rs `run_content` :2167（布景 =
  两节点图课），现 1 硬判据 + 软读数；机械 grep 通道先例 budget_line_calls
  :2326-2346。
- **est 上游行**：learning_graph/mod.rs:405（生长提示词【节点纪律】）。

## learnhub 参考（Reference 表——实码先读铁律）

| 件 | learnhub 路径 | 取什么 |
| --- | --- | --- |
| #177 known_names | `domain/node_ctx.rs:276-303`（计算）、`engine/contentgen/facade.rs:207-238`（known_line 拼装） | 已知集口径、行格式、空名单省行、BTreeSet 稳定序、声明 ULID 解析失败=Broken |
| #177 构件词边界 | `prompts/contentgen/content-plan.md:34` | 条款全文（名单∪teaches 当工作词；名单外领域名词不立名不预教；日常词替代+三例） |
| #177 R12 收窄 | `prompts/contentgen/content-write.md:48` | 收窄后「表述工具在场」全文（已知集内；「先正式引入再用」仅限记号与读法；领域概念不适用+三例） |
| #178 意图画像 | `content-plan.md:39` | 四档判定全文（短预算×首现×know=轻觘认课 / 长预算×多前提×use=操练整合 / 短预算×重访=巩固变式 / 长预算×teach=完整推导）；先砍雄心再砍内容；capacity_note 必写指名原因；est 缺席不虚构 |
| #178 练习形态 | `content-plan.md:31` | 照示范复制各档合法；引入未教方法论/推理链即越档（know 档尤然）；咖啡域正反例 |
| #178 取材池 | `content-plan.md:36` | 考察概念优先取自已知名单（取材即复习）；情境载体归受众条款两不相扰；多样轮换句 |
| #178 规划即雄心 | `content-write.md:47` | 节数题量形态照规划不加码；练习不引入规划外方法论；考察域照规划 |
| #178 est 双向 | `prompts/node-growth/node-growth.md` 节点尺寸行 | 「est 也是后续内容生成的容量预算锚……」追加句 |
| #178 validator 随批 | f84f242（validator.rs:33 一行） | capacity_note 语义注记改法 |
| #180 先例 | `domain/node_ctx.rs:62-76, 204-243`、`facade.rs:220, 610-622` | Precedent{concept,node_id,node_name,sections}、祖先闭包∩teaches、首现/声明-only 不入、节标题取 `##` 行、缺席=空、行格式与省行 |
| #180 多样轮换 | content-plan.md:36 尾句 | 不在单一概念连出多题 |
| smoke | `src-tauri/src/bin/smoke-content.rs`（8150d92 + 683953b 扩展） | 布景（声明档案+先行节点正文两节+目标节点 know 空 assumes）、六判据形状、软读数 |

## 实施要点

- [ ] **第一步 R12 收窄**（n2 D2 排序）：两处条款替换为收窄版措辞（EN/CN
      各一，域中立），金样重生成；版本臂对照（旧 tip vs 新 tip）确认存量
      case 不劣化。
- [ ] **known_names 计算模块**（n2 D1 口径）：service 层纯函数——本课程图
      非 archived 节点 teaches ∪ 声明锚 concept_ids（072 表），id→canonical
      显示名 JOIN；声明引用不可解析 = 如实跳过该条并 warn（不 Broken 炸——
      allo 撤销语义「声明可撤、概念不灭」下悬空引用是合法态，learnhub
      Broken 语义不适用，票面记差异）；空名单整行省略。
- [ ] **已知行注入**：lesson_user_text 概念面段前新增「课程已知概念」行
      （格式照 learnhub facade.rs:207-238 域中立化）；数据经
      GraphLessonContext 新字段（Option，None = 行缺席）。
- [ ] **teaches 档位行**（n2 D4）：节点块加档位（tier 已在库），装配点
      lesson_user_text；传统线不适用。
- [ ] **先例摘要行**（n2 D5）：扩展 first_appearance_set 返回 concept→祖先
      节点归因；先例行 = 概念 ← 节点名 + 先行节标题（taught_sections_
      for_lessons）；空先例省行；GraphLessonContext 新字段注入。
- [ ] **条款移植**：图线 = GENERATE_LESSON_AGENT_SYSTEM（lesson_content_loop.rs:75）
      + 节大纲/manifest 规划条款（构件词边界/意图画像/练习形态/取材池进规划
      面；表述工具在场收窄/规划即雄心进写作面）；传统线按 n2 D6 只收 R12 +
      自足条款（section_outline_system）。金样重生成 + 条款覆盖短语断言
      （三闸先例：金样/围栏/短语断言）。
- [ ] **k03 语义换血**（n2 D3）：lesson_content_loop.rs:93 尾句替换 +
      lesson_draft.rs:997-1005 审计文案 + service/lesson.rs:1498-1505 措辞
      对齐 #178（先砍雄心再砍内容；收敛不住必写指名原因；预算内省略）。
      normalize/形状不动；开关 NOMI_LEARNING_EST_BUDGET 语义面覆盖。
- [ ] **est 双向轻项句**：learning_graph/mod.rs:405 行尾追加（数字不动）。
- [ ] **smoke content 升级**（n2 D7）：布景改造（声明档案锚行 + 先行节点
      带两节正文）；硬判据①输入面（两面 prompt 含「课程已知概念」且命中
      布景概念——观测存档 grep）②条款在场 ③契约面 + est 铸值==节点 est
      ⑤先例行 + 先行节标题，全进 EXIT；超纲专名抽查软读数 + 对照卡人读。
- [ ] **双臂验证**：开关臂（NOMI_LEARNING_EST_BUDGET=off 同二进制）+
      版本臂（纯条款面，旧 tip vs 新 tip）；既有审计计数器 on 臂不劣化
      （naming_*/narrative_*/over_capacity）。

## 验收

- `cargo test -p nomifun-learning` + `-p nomifun-ai-agent` 全绿（新单测：
  known_names 口径边界/空名单/悬空声明跳过、先例归因/首现不入/文件缺席、
  capacity_note 换血后条款断言）。
- smoke content 硬判据全绿 EXIT=0（有凭据真机）；无凭据如实非零。
- 对照卡两臂并排（读数：节数/capacity_note 出现与内容/已知行命中数/先例行
  命中数/超纲专名抽查）；人读清单落 Resolution。**只跑一臂不算关票**。
- typecheck / i18n / workspace check 净。

## 非目标

- read_prior_content 工具轮（雾区 #168）。
- #179 表述工具入册（雾区，等记号密集域需求）。
- 传统线概念体系重建（D6 豁免留档）。
- capacity_note 出现率自动消费（雾区，m10 接线后置）。
- 迁移（本波零迁移——纯提示词与计算投影；若实施发现必须动库，回 map 重新
  对账）。
