# Map: 学习课程生成——目标中心化与知识库接回（2026-10-13）

<!-- wayfinder:map -->

## Destination

把"从知识库生成课程"从**库即课程的一次性采样快照**改造成**以用户目标为中心、可实时读库、
支持多库**的生成面：目标文本成为采样排序、大纲范围、课时检索的主索引；课时正文写作接回
知识库（检索 + 全文读取）而非冻结的 8KB 文件头；多知识库选择作为参数扩展落地；课程式
知识库（本身按课编排）不因目标筛选而倒退——这四个方向全部裁决完毕并可按票实施。

## Notes

- **前情**：本轮由 2026-10-13 的风险评估会话发起（用户提出多库选择诉求 → 追出三条更深
  缺陷：课时写作不接知识库、目标中心化缺位、课程式库可能被负优化）。一轮/二轮吸收
  （`learning-absorb/`、`learning-absorb-2/`）的实施与裁决**不在本图范围**，但共享生成
  管线文件（`course_outline/`、`generation/`、`service/generate.rs`）——**排期建议：本图
  t1/t2 先落，二轮吸收实施随后 rebase**，避免同文件反复改。
- **现状锚点**（评估会话核实）：
  - 知识服务在 learning crate 只有两处调用：`tutorial.rs:52`（教程播种）、
    `service/generate.rs:119`（建课采样）——**建课后知识库再无读取**；课时正文 grounding
    = 单文件 8KB 前缀（`sample.rs` 的 `read_prefix_lossy`）+ 邻居 1000 字（`lesson.rs:12`）。
  - 采样 = `rels.sort()` 字母序 `take(40)`（`sample.rs:52-56`），不看目标。
  - 传统建课请求**没有目标输入**；`proficiency_statement`/`learner_basis` 是学习图专属。
  - `KnowledgeService::search_bases(kb_ids, query, limit)`（`service.rs:3802`）已是跨库
    词频检索：路径/标题加权打分、文件级命中（`{kb_id, kb_name, rel_path, heading,
    snippet, score}`）、64MB 缓存、每库 20s 遍历预算（超时该库返空）、结果按分归并；
    agent 侧 `knowledge_retrieval.rs` 的 search→`encode_doc_handle`→read 全文已闭环。
  - **query_terms 只按空白切词**（`service.rs:5967`）——CJK 整句会当一个词条做子串匹配，
    长句检索必然空，此行为需在检索侧修正（t3）。
  - 教程课程走 `import_course` 资产包（`tutorial.rs:85`），不经 `GenerateCourseRequest`
    校验——本次输入面变更不触碰它。
- **铁律沿用**：课程边界（结构课程内隔离）、概念表跨课程；学习图课程不读库（t5 议题）。
- **常设偏好沿用**：每条生成管线配 `nomicore learning-smoke` 无头冒烟入口；LLM 观测走
  `ArchivingCompleter`。
- **工作方式**：t1 起为 task 票（规格已裁决，直接实施）；t5 为 grilling（边界裁决）。
  决议逐票写入 Resolution 并追加到下方索引。

## Decisions so far（评估会话输入，待各票 Resolution 追认）

- **目标输入形态（用户裁决 → t1）**：知识库流加**多行文本框**——简单时即课程名称（课程名
  本身是高度抽象的目标），复杂时是目标描述/长说明；不做形态判断，整体文本即目标；课程名
  默认即该文本，不引入 LLM 派名；UI 必填、后端 `Option` 宽容（缺省回退库名，兼容旧客户端）。
- **检索形式（评估会话定案 → t3）**：**复用 `search_bases` 的多词条关键词查询**，不新建
  检索系统；查询词必须**词条化**（中文空切分不成立，CJK 退化行为在 t3 修正）；建课采样排序
  用确定性打分（不引 LLM 随机性），课时写作查询 = 课时标题/purpose 词条；embedding 语义
  检索为 v2 增强，不立项。
- **不倒退纪律（课程式库，用户追问；防护 → t2/t3）**：相关度决定**读什么**，结构决定
  **按什么顺序读**；冲突时结构优先——采样清单保持路径字典序、连贯预算补章节内相邻文件、
  检索不替换课时主 source、采样快照保留完整目录树并披露低分未采文件。
- **多知识库形态（→ t4）**：遍历 `search_bases` 已天然多库；落库采用数组列（仿
  `cs_agents.knowledge_base_ids` 先例），预算按库分摊，去重键排序后拼接，引用带库归属。
- **排期**：本图 t1/t2 先行，二轮吸收实施（g1–g4/t1）随后 rebase。

## Frontier

- t1（目标输入）→ t2（采样）/ t3（写作接库，含"目标词条化 LLM vs 确定性切片"拍板）；
  t4（多库扩展）等 t2+t3；t5（边界裁决）。全部 open、无人认领。

## 操作约定

- 认领：开工前把 `assignee` 写成自己，会话结束未解决则清空。
- 解决：写 `## Resolution`、`status: closed`，并把一行摘要追加进 Decisions so far。
- 阻塞：只在 frontmatter `blocked_by` 里表达（create-then-wire）。
- 雾毕业：下面 Not yet specified 说清问题了就建票并从雾里删掉该条。

## Not yet specified

- 目标词条化采用 **LLM 单次调用（建课时一次，推荐）还是确定性切片**——t2/t3 实施前拍板。
- 快照刷新机制（建课后库更新，是否提供"重采样"入口）——t3 落地后按使用情况再议。
- 课时检索的 token 预算常量与 top-N 默认值——t3 实施时以学习图教练批的预算纪律为参照定。
- 学习图课程接库若选做，其"连贯预算"条款与结构铁律的豁免口径——t5 裁决。
- 本图落地与二轮吸收实施的合并/推送时机（`feat/learn0928` 已领先 origin）。

## Out of scope

- embedding/语义检索基建（v2 增强，单独立票）。
- 学习图课程的生长/结构语义（t5 只裁决"接不接库"，不重开结构铁律）。
- 课程更名为独立工单——默认课程名即目标文本，足够。
- 描述流（无库建课）的改造。
