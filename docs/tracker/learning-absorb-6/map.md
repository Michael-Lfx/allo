# Map: 六轮吸收——内容边界波（优先）+ 题面渲染 + 开放题判分重写（bd31277）

<!-- wayfinder:map -->
<!-- 状态：开图（2026-10-28）。三线深挖（三代理只读对账 + 关键事实主会话复核）
      已完成，结论织入各票「allo 现状」。前沿 = n1（可直开工）/ n2+n3（C 线，
      用户最优先）/ n4+n5（A 线）。两处与 allo 既有条款的真冲突（k03 尾句、
      R12 旧措辞）分别在 n2/n3 票面裁决，无前提性阻塞。 -->
<!-- 前情：五轮 m1–m11 全部落地 feat/learn-absorb-5（尖端 8a424d434，含
      origin/main 同步 merge 6f7edfd9d，未推送）。本波实施基线 = 该分支尖端：
      **n3 依赖 m2 已会声明锚表（迁移 072）与 declare/revoke 端点**。 -->
<!-- 来源：learnhub C:\Users\test\Desktop\my\learnhub，ab908e0..bd31277 共 11 提交
      （10-26 ×1 + 10-27 ×4 + 10-28 ×6）。参考实码逐票引用（Reference 表）。 -->

## Destination

三线域中立移植：

- **C 内容边界波（用户最优先）**：#177 超纲围栏（known_names=已教∪已声明
  边界行 + 构件词边界 + R12 逃生门收窄）、#178 前置规划（意图画像四档 +
  capacity_note 语义升级为节点意图超载/est 复议信号 + 取材池 + 练习形态按
  档位）、#180 内容连续性（先例摘要行 + 取材多样性）+ smoke content 判据
  升级（learnhub 6 判据对齐）。
- **A 开放题判分重写**：#181/#182/#184——判底进评审器（explanation 先行，
  参考答案+要点契约增强档 grilling）、judge 评审执行器（schema/重试环/超时/
  脱敏归档）、降级出口裁决、smoke grade 档。
- **B 题面渲染**：InlineMarkdown 内联通道（KaTeX 一等公民）+ 五渲染面接入 +
  fill_in_blank `___` 转义（allo 特有）+ 复习面解析口径统一。

## Notes

### 两仓对账关键事实（2026-10-28 三代理深挖 + 主会话复核，票面锚点以此为准）

- **分支与迁移基线**：m2 已会声明锚表 = 迁移 `072_learning_basis_declarations.sql`
  （feat/learn-absorb-5；主仓工作树在 feat/learn-absorb-4 看不到它——研究时
  勿误判「m2 未落地」）。本波**迁移从 073 起**（072 已占）；C 波预计零迁移。
  票面文档按惯例落当前分支，实施在集成线（feat/learn-absorb-5 尖端之后）。
- **allo 两处 R12 都是 #177 收窄前旧措辞（活性超纲门）**：
  `generation/lesson.rs:801`（传统线 EN）与
  `nomifun-ai-agent/src/lesson_content_loop.rs:65`（图线 CN）均含无限制逃生门
  「或正文已先行正式引入（再用）」——learnhub 病历（全称量词节点正文合规引入
  论域/反例，已知集退化为恒真）的发病条件 allo 全部具备（know 档首节点
  assumes 空 + 无回放比对面）。**只注入名单不收窄 = learnhub 已证伪的组合**。
- **k03 尾句与 #178 直接矛盾**：`lesson_content_loop.rs:93`「明显超容才写……
  只留痕、不改安排、不压缩讲解深度」vs #178「超预算先砍雄心再砍内容，收敛
  不住 capacity_note 必写指名原因」。k03 是 #154 语义（learnhub 前身），#178
  是语义升级；并存即自相矛盾，n2 裁决换血。形状面不动
  （`normalize_capacity_note` lesson_draft.rs:58、审计观察条文案
  lesson_draft.rs:1001 同步改）。
- **allo 概念面缺三样输入**（learnhub #177/#178/#180 的输入面）：
  ① 无「课程已知概念」行（grep 零命中）；② 本节点 teaches 档位在任何图路径
  提示词不可读（GraphConceptFaces 不带 tier；而写作纪律 lesson_content_loop
  「档位决定内容形态」已在引用模型读不到的值——顺手修的真缺口）；③ 先例摘要
  整条缺席（`first_appearance_set` service/lesson.rs:1897-1929 算祖先闭包但
  **丢弃节点归因**，要扩展返回；先行节标题可复用 `taught_sections_for_lessons`
  :1461-1486）。
- **known_names 半边现成构件**：已教半边 = 图内 teaches 投影（注意 allo
  `taught_ledger` learning_graph/mod.rs:972 是**跨课程全局**账，与 learnhub
  「本课程图」口径不同——n2 裁决）；声明半边 = m2 锚表
  `learning_basis_declarations.concept_ids_json`（072，撤销语义「声明可撤、
  概念不灭」已定）。
- **allo 开放题判分已是 LLM 评审制，但三重脆弱 + 判底缺席**：
  `grade_open_answer`（service/progress.rs:206）单次裸调 `completer.complete`
  （no-thinking、无瞬态重试、非 json 通道、无超时——provider 挂起 = 请求永久
  悬挂）；判底只吃课时标题+摘要（build_open_grading_prompt :296），题目自己的
  `explanation`（**非空必填**，models.rs:928 + LESSON_SYSTEM 规则）从未进评审
  prompt。`config.answer` 对开放题**契约性为 null**（validate_shape
  models.rs:1054 强制 null，非 null 即拒）——learnhub 式「参考答案+要点进判底」
  需契约反转+模板重写+存量双轨，v1 用 explanation 判底零契约变更。
- **A 线三个结构性豁免（显式裁决避免半吊子移植）**：① quiz/复习线只出客观题
  （quiz.rs:204 OBJECTIVE_KIND_SQL_LIST）——§9 分母剔除裁决 allo 结构性免疫；
  ② HeldSession verdict 缓存对 allo 伪需求（submit_attempt 判分即落账即终态，
  attempts 表本身就是缓存；learnhub 缓存服务其「结算重判分」架构）；③
  EvidenceTier 证据档 allo 结构性不需要（题型白名单已实现自动性证据纯度）。
- **现存不对称（A 波顺带裁决）**：node XP 结算 `total_questions` 只数客观题
  （xp.rs:62-69）但 `answered/passed` 子查询**不过滤题型**（xp.rs:70-95）——
  开放题 passed 抬分子不进分母。
- **B 线渲染面全集**：五修面 = LessonStudy.tsx:75 题干 / ActivityInput 选项
  （arco Radio/Checkbox → `<label>` 短语内容位，必须 span 形态）/ 排序条目
  ActivityInput.tsx:41 / 复习题干 ReviewSession.tsx:191 / 复习答错揭示
  :293-298（解析走纯文本 `<Text>`，与课时面 `<Markdown>` 不一致）；
  三保持面 = 题库列表行 line-clamp、ReviewSession.tsx:439 标记待编辑、匹配题
  Select.Option。**allo `<Markdown>` 组件不可当内联通道**（ShadowView 每实例
  钉死排版 + 块级文档流）——照 learnhub 抄轻量 light-DOM InlineMarkdown，但
  必须复用 allo `convertLatexDelimiters`（`\(...\)` 容错）、排除 remarkBreaks、
  不开 allowHtml（学习面从未开，XSS 底线）。KaTeX CSS 已全局 import。
- **fill_in_blank `___` 与 markdown 冲突（allo 特有，learnhub 无此题型）**：
  `___` 独占一行 GFM 渲染成 `<hr>`、行内成对吃成粗斜体——内联组件需占位符
  预处理，n1 最易漏点。
- **quiz 前端 UI 未建面**（q2 只落后端载体；全 ui/src grep quiz 零命中）——
  n1 票面留注「后续建面必须复用渲染管线」，不入本波。
- **观测面**：allo 判分调用生产零观测档（ArchivingCompleter/prompt_observer
  均冒烟专用且**全文落盘零脱敏**）——判底（参考答案）进评审 prompt 后需
  learnhub 式占位替换归档器（n5 随票）；smoke grade 四判据 + 脱敏回读移植。
- **smoke content 落差**：allo content 档仅 1 硬判据（管线走通），learnhub
  smoke-content 6 判据（输入面名单行/条款在场/契约面+est 铸值/落位/先例/入册）。
  #177/#178 的教训恰是「回放对输入面修复结构性失明」——n3 把输入面+条款在场
  判据升级为硬判据（机械 grep 通道现成：budget_line_calls 先例
  learning_smoke.rs:2326-2346）。

### 验证总纲（沿用四/五轮纪律）

1. 开关臂：新行为面配 `NOMI_LEARNING_<FACE>=off`（switches.rs 先例），同构建
   同凭据同 fixture 双跑；纯提示词资产面用版本臂（旧 tip vs 新 tip，独立构建
   目录，金样重生成 `LEARNING_UPDATE_GOLDEN=1`）。
2. 硬判据进 smoke EXIT；效果判据产出**对照卡**（两臂读数并排 + 人读清单）——
   两臂必须都跑，只跑一臂不算关票。
3. 既有审计计数器（naming_* / narrative_* / over_capacity）与既有 smoke 判据
   on 臂不得劣化；观测存档 `observation/learning-smoke/run-*/calls.jsonl`。
4. learnhub 参考实码先读铁律（每票 Reference 表）；迁移从 **073** 起（072 已
   占，069-072 在 feat/learn-absorb-5）；DB 迁移高危区纪律照旧。
5. 真机纪律：NOMI_CHANNEL=dev 显式带、跑前核 nomicore.exe mtime、--data-dir
   放子命令前。
6. 前端门：typecheck / i18n 双语（`bun run gen:i18n` + `check:i18n`）/ theme /
   dead-css；Shadow 内新 token 同步变量表。

### 雾区（Not yet specified）

- **read_prior_content 工具轮**（#168 雾区延续）：allo completer 无工具缝；
  #180 摘要行先行（learnhub 真机实证摘要已够、工具按需纪律生效）。
- **同题重复提交幂等/折叠**：allo 现状每次提交全新评审；缓存对 attempts 架构
  伪需求，折叠语义后置。
- **quiz 回合 UI**：建面时自带渲染管线（复用 n1 InlineMarkdown）+ evidence
  揭示位，不在本波。
- **capacity_note 出现率 → m10 补批复议读数接线**（#178 重项候选）：观测先行
  （smoke 读数），自动消费后置。
- **#179 表述工具入册**：可挂 allo 别名登记单点，依赖记号密集域需求，缓。

## Tickets

| id | 票 | 类型 | 依赖 | 线 |
| --- | --- | --- | --- | --- |
| n1 | 题面渲染管线接入（InlineMarkdown + 五面 + `___` 转义） | task | — | B |
| n2 | 内容边界波 grilling（known_names 口径 / R12 / k03 换血 / 先例 / 档位行 / 传统线 / smoke 判据） | grilling | — | C |
| n3 | 内容边界波实施（#177/#178/#180 + smoke 判据升级） | task | n2 | C |
| n4 | 开放题判分重写 grilling（判底来源 / 失败出口 / 分数形态 / 模型档 / 范围） | grilling | — | A |
| n5 | 开放题判分重写实施（judge 执行器 + 判底 + 降级出口 + smoke grade） | task | n4 | A |

优先序（用户 2026-10-28 指示）：**C 最优先**（n2→n3），B 独立可并行先修
（n1 无阻塞），A 随后（n4→n5）。
