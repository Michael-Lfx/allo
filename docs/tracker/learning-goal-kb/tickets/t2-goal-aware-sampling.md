---
id: t2
title: "目标感知采样：相关度选入 + 结构保真排序 + 连贯预算 + 披露"
type: task
status: closed
assignee: zhangyj
blocked_by: [t1]
part_of: ../map.md
---

# t2: 目标感知采样：相关度选入 + 结构保真排序 + 连贯预算 + 披露

**What to build:** 建课采样从"路径字母序 take(40) + 每文件取前 8KB"改为**目标感知**：
对候选文件用确定性打分（与 `KnowledgeService::score_md` 同尺：路径词条命中 + 首标题命中 +
正文词频，可重放）按目标选入；**采样清单保持路径字典序**（结构信号原样交给大纲 agent）；
带**连贯预算**（选定文件的目录邻居/序号相邻文件按比例补入，防课程式库断链）；每文件摘录
改为按目标相关段落选摘（单文件预算 8KB → 24–32KB）；快照记录完整文件树与截断披露。

**Blocked by:** [t1 目标输入](t1-goal-input.md)。

**Status:** ready-for-agent

- [x] 采样器重写（`generation/sample.rs`）：全库候选枚举 → 目标词条打分排序（确定性，
      不引 LLM）→ 相关集合 + 连贯预算补相邻 → 输出仍按路径字典序的 `(path, excerpt)`。
- [x] 连贯预算常量：占采样额度约 30%，补入规则 = 同目录兄弟优先、其次同父目录下前后
      序号相邻；补入文件即使单文件分低于阈值也保留。
- [x] 摘录策略：单文件上限 8KB → 24–32KB；文件超限时按目标词条命中定位**相关段落选摘**
      （确定性：命中行 ± 上下文窗口），不再固定取文件头。
- [x] 快照披露：`samples_json`（或伴随字段）记完整文件树（目录结构保留）、已采/未采计数、
      因低分未采文件清单（供 UI 与诊断展示）；简报文本如实披露"已按目标采样 x/y 文件"。
- [x] UI 披露：建课进度/课程详情可见采样统计（i18n 双语）；学习图工作区不涉及。
- [x] 保留 `sample_base_files_with_budget` 的显式预算签名供单测复用；旧行为（无目标）
      回退：无 goal 时按现状字母序（t1 的兼容路径）。

**验收（含课程式库专项）：**

- 混杂库（2000 文件：收据/会议记录/剪藏 + 目标相关文档）建课：目标相关文件 100% 入样、
  高噪声类别不入样、快照可读披露。
- **课程式库专项（不倒退）**：库按 `01-`…`09-` 编排、目标与库匹配 → 采样覆盖章节主干
  （连贯预算生效）、清单顺序仍是字典序；目标窄于库覆盖时中间必经章不被断开。
- 大文件（>8KB）相关事实落在文件中段 → 出现在摘录中。
- 无目标路径回归：与现状采样结果一致。
- `cargo test -p nomifun-learning` 全绿。

## Resolution

**结论速览**：采样器重写为「目标词条打分选入 → 连贯预算补章节邻居 → 输出保持路径字典序 →
按词条命中段落 ± 上下文窗口选摘」；词条化 = 确定性切片（无 LLM，纯函数模块，t3 复用）；
披露走新列 `samples_meta_json`（完整文件树/计数/低分未采清单）；`samples_json` 保持
`Vec<(String, String)>` 旧形状，既有消费方零改动。

- 词条化 v1（`generation/terms.rs::term_slices`）：标点/空白分段 → 中文段相邻**二元**切片
  （整句子串命中必然蕴含其全部二元命中，二元不丢召回）/ ASCII 词整词小写 → 去停用词与单字
  → 去重 → 上限 `TERM_MAX_COUNT = 32`。采样打分与（t3 的）检索查询共用同一套词条，口径不
  漂移；模块为纯函数、无词典依赖、可重放。
- 打分（`goal_relevance_score`）与 `KnowledgeService::score_md` 同尺：词条命中路径 +4 /
  命中首标题 +3 / 正文词频每词条封顶 5；0 分 = 低于相关度阈值（不进相关集合、进低分清单）。
- 选择：相关集合分数降序（同分路径序）占满额度；连贯补入只用余量且不超过采样额度的 30%
  （`GOAL_SAMPLE_COHERENCE_PERCENT`）——从已选文件出发反复补入**同目录字典序紧邻**的前后
  文件（课程式库按 `01-`…`09-` 编排时序号相邻即路径相邻），补入文件即使 0 分也保留。输出
  按路径字典序排序（与旧 `rels.sort()` 同尺）。
- 摘录：单文件上限 8KB → 24KB（`GOAL_SAMPLE_MAX_PER_FILE`）；超限文件按命中段落 ±
  上下文窗口选摘（命中段权重 = 命中数、前后相邻段 +1；只取有相关性的段落，不拿无关填充瓜分
  预算），无命中段则回退文件头；总量 = 配额 × 单文件上限（相关文件绝不因总量被挤掉）；单文件
  扫描上限 1MB（确定性——不引入机器相关的遍历预算，同一库同一目标处处同一份采样）。
- 披露：迁移 072 新列 `samples_meta_json`（nullable + JSON object CHECK；NULL = 无目标路径
  或旧课程）承载 `SampleDisclosure`（目标文本、全库计数、已采计数、连贯补入数、完整文件树、
  低分未采清单）；`brief_line()` 如实进大纲简报；DTO `CourseSamplingView`（低分示例 ≤20）
  经 `CourseSummary.sampling` 暴露，UI 在建课完成视图与课程详情展示（i18n 双语对称 + 双语言
  断言测试）。
- **与票面的偏差（新增一条防线）**：目标与库无交集（一个词条都不命中）时**不再失败**——
  退回默认字母序采样（课仍能建出来，不比现状差），披露以 `goal_fallback` 标注，口径改为
  「已按默认顺序采样 x/y 个文件（目标措辞与库内容可能不一致）」，简报与 UI 均不冒充
  "按目标采样"。理由：目标措辞与库内容不一致是常见输入错配，直接报"库里没有文档"会
  把可用的库判死。
- 兼容：`samples_json` 形状未动（`service/lesson.rs` 三处消费方零改动）；`sample_base_files_with_budget`
  显式预算签名保留（旧路径与单测复用）；无目标 → `disclosure = None` 且采样与旧实现逐字一致。

**验收对照（测试）：**
- 混杂库 2000 文件（收据/会议记录/剪藏 1960 + 目标相关 40）：相关文件 100% 入样、噪声类别零
  入样、清单字典序、披露可读（`goal_sampling_on_a_mixed_two_thousand_file_library`）。
- 课程式库专项：九章 `01-`…`09-`、目标只命中第 05 章 → 九章主干全员入样（其中 8 章靠连贯
  预算）、顺序仍字典序（`goal_sampling_keeps_the_course_trunk_intact`）。
- 大文件中段事实：>24KB 文件的中段 K-means 事实进入摘录、无关文件头段落被剔除；8KB–24KB
  文件整篇交付（`goal_sampling_keeps_relevant_facts_from_the_middle_of_a_large_file`）。
- 无目标路径回归：空/空白/纯标点目标 → 与 `sample_base_files_with_budget` 逐字同结果、无披露
  （`goal_sampling_without_a_goal_falls_back_to_the_legacy_result`）。
- 服务级与迁移：brief 采样 + 标题/目标列（`kb_goal_flows_into_brief_title_and_course_row`）、
  无交集回退（`kb_goal_without_any_match_falls_back_and_still_generates`）、迁移契约
  （`cargo test -p nomifun-db`：旧行 NULL 可读 + 新行必须 JSON object）。
- 门：`cargo test -p nomifun-learning` 184 passed；`-p nomifun-ai-agent` 1115 passed；
  `-p nomifun-db` 全绿；`cargo check --workspace` 干净。

