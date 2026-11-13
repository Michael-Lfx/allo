---
id: g2
title: "结构生成契约与存储落点"
type: grilling
status: closed
assignee: zhangyj
blocked_by: [g1]
part_of: map.md
---

## Question

learnhub 的结构产物（8 标量键 + domain + spines 6–12 + blocks 40–80 + edges 三型 +
gaps + quality）与前置地形（门口 G1..Gn）引入 allo 时，契约与存储怎么落？

需要裁决：

1. **产物取舍**：structure 全套引入还是裁剪（例如 allo 已有 `learning_goal`/`learning_scope`
   与入口域盘点，与 goal_type/proficiency_statement/learner_basis、前置地形功能重叠）？
   前置地形要不要独立产物，还是并入 structure 一次生成？g1 已裁决罗盘退役、人读概览交给
   结构读侧投影——learnhub 同位产物是 knowledge-system 文档，要不要一并产出由本票定。
2. **SQLite 落点**：结构文档快照存哪个表哪个字段（单字段 JSON 快照语义；ADR-0002 铁律：
   投影永不落盘、不建关系表存块间边、运行时重算）；快照与课程行的关系（一课一快照？
   重跑 = 新快照版本？learnhub 是重跑同目标 = 新课并列，多用户服务端是否照搬）。
3. **生成轮次**：learnhub 每轮 2 次串行调用 + 门口对账 + 整批重试 ≤3 带反馈，照搬还是
   合并为单次调用（allo 的 completer 已有 thinking/非 thinking 双通道与 480s 预算先例）？
4. **契约文档**：是否照 learnhub 建 `docs/contracts/` 式条款化契约（gd.* 编号即稳定机器
   标识）放进 allo 的 `docs/architecture/learning` 域文档。

约束：建课交互形态依赖「终点锚体系命运」的裁决（g1）；契约一旦定案是后续投影 API（t1）与
结构化输出基建（g4）的前提。

## Resolution

**速览**：六项全采推荐——

1. **契约全量 + 输入最小化**：structure 全套（8 标量 + domain + spines 6–12 + blocks
   40–80 五键纪律 + edges 三型 + gaps≥3 + quality≥5）照搬；向导单步只收三项——学习目标
   （必填）、程度声明（可选，原终点 goal_note 的单数继承，缺省 LLM 从目标扩写）、基础声明
   （可选，缺省纯零基础）；goal_type（capability|coverage）与 domain_type 由 LLM 自判落
   产物。teaching_style 等现有控件不动。
2. **前置地形独立产物**：第二次调用生成门口 G1..Gn 判据；structure 的 prerequisite_doors
   只准引用地形门口，生成后机械对账堵引用悬空；**入口域盘点退役**（`survey_entry_domains`
   / `ENTRY_SURVEY_SYSTEM` / `EntryDomain` 随实施移除，地形门口是其结构化替代）。
3. **生成轮次**：① structure ② 地形 串行双调用；任一产物违反契约，两份一起重生成、
   逐条款违规报告（gd.* 编号）注入重试装配，≤3 轮，全成才落库（落库 = 快照原子更新）。
4. **存储**：`learning_courses.structure_json` 单字段快照列 + 重跑覆盖（重生成 = 换参考
   地图不换课；节点/概念网与结构无连接，覆盖无副作用）。071 迁移与 g1 同支：删
   `learning_course_endpoints` + 终点课时行 + `compass_md` 列，加 `structure_json`。
   投影运行时重算、永不落盘（t1 施工准绳）。
5. **人读概览**：不产独立文档（learnhub 的 knowledge-system 是其单机文件形态伴生物）；
   概览语义全部落在读侧投影（p1 可视化 + 路线图）；将来窄宽/无画布场景可从快照运行时
   渲染文字大纲，零契约成本。
6. **契约文档**：新建 `docs/contracts/learning-structure-artifacts.md`，条款化编号
   （gd.*）= 校验器报错、整批重试反馈、提示词自查三方的公共语言；域文档
   `docs/architecture/learning.zh.md` 链接过去。

解锁影响：g4（结构化输出基建）、t1（图投影 API）解锁；建课向导单步形态定案，雾区仅剩
「生成进度与可视化的衔接」等 p1。

