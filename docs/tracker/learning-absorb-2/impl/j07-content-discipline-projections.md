---
id: j07
title: "内容成色纪律与教学上下文投影"
type: task
status: closed
assignee: zhangyj
blocked_by: [j04]
part_of: ../map.md
---

# j07: 内容成色纪律与教学上下文投影

**What to build:** R1–R8 写作纪律进两条管线（零新机械门）+ 首现/引用名两读侧投影 +
content 冒烟子命令。

**实现决议:** 二轮 g2（全部）；R3 词表依赖 j04 别名记录；R9/跨课预告的提示词位随本票
（条款文本已在 j01 落）。

**Blocked by:** [j04]（别名池/记录——R3 词表与引用名落位的数据源）。

**Status:** ready-for-agent

- [x] **通用纪律（R1/R4/R6/R7/R8）进两条管线提示词**：图节点正文（lesson_content_loop
      graph 分支）+ 传统课程（节大纲/节正文）：先推后练 / 档位→内容形态 / 汇合渐隐
      （assumes ≥4 逐级加要素）/ 真对照 / 例题密度（3–6 递进）——域中立、零新机械门、
      「写作取向不是格式模板」收尾
- [x] **概念面条款（R2/R5）只进图节点路径**：引用名开讲（正名首现括注）/ 首现带引入
      （非首现一两句回收）
- [x] **R3 投影词表 + 评审判据**：规划/正文辨认旧叫法（数据源 = j04 别名池）；评审判据
      增观察条（正文用名一致性、首现引入、R9 声部/结构词——观察面非阻断）
- [x] **首现投影（祖先闭包不动点，零新存储）**：起点 = assumes 概念集；循环扫非
      archived 节点，教了任一 assumed 概念即纳入、并入其 assumes、teaches 计入
      ancestor_taught 直至不动点；本节点 teaches 中不在 ancestor_taught = 首现；并行分支
      与 archived 不夺首现；`graph_node_topology` 注入首现行
- [x] **引用名开讲**：迁移 `learning_lesson_concepts` 加 `ref_name` 列（nullable）；
      `apply_batch` 引用解析时记录模型实际用名；上下文 tiered 行渲染 `别名(档位;正名:
      正名)` 形态
- [x] **传统课程落点核对**：通用五条进节大纲/节正文提示词；概念面不进（无挂接）
- [x] **learning-smoke 增 `content` 子命令**：为一个节点生成正文落观测面 + 读数（字数/
      承诺兑现/小节数/首现标注）——读数面非门禁
- [x] 验证：`cargo test -p nomifun-learning`（首现闭包不动点单测：并行分支/archived/
      空 assumes）+ 迁移测试 + `bun run typecheck`

**Resolution:**

**速览**：八条全采落地——通用五条（R1/R4/R6/R7/R8）+ R9 声部进两条管线提示词；概念
面条款（R2/R5）与三投影（首现/引用名/别名池）只进图节点路径；评审判据增观察条（非阻
断）；smoke 增 content 子命令。零新迁移（ref_name 077 已由 j04 落列，本票消费）。

1. **概念面结构化投影**（`GraphConceptFaces`，lesson_draft.rs 导出）：`first_seen/
   returning/ref_names/aliases` 四组事实 + `render_text()`（渲染与审计共用同一事实
   源）。`GraphLessonContext.concept_faces: Option<_>`——本节点无概念挂接时整体缺席。
2. **首现判定** = 祖先闭包不动点：`first_appearance_set` 纯函数（service/lesson.rs）
   ——扫描集起点 = 本节点 assumes，候选节点 teaches 命中即纳入、并入其 assumes、
   teaches 计入祖先已教集合，直到不动点；本节点 teaches − 祖先已教 = 首现。候选集
   SQL 剔除已删墓碑与归档课（archived 不夺首现）；并行分支因闭包只沿 assumes 链生长
   天然不夺。五条单测（直系/间接闭包、并行分支、archived 过滤、空 assumes、引用名
   渲染）。
3. **引用名开讲**（存储 j04 已落，本票消费）：`graph_node_topology` 的 declares 查询
   带出 `lc.ref_name`；前置段 tiered 行渲染 `引用名（正名：正名）` 形态（ref 与正名同
   名不括注）；概念面 `ref_names` 只列与正名不同的条目。
4. **投影词表（R3）**：`active_aliases_for` 查本节点概念（teaches ∪ assumes）的活跃
   别名（j04 alias_records 解析、退役不占池）；概念面渲染「旧叫法（正文不使用，必要时
   括注帮助辨认）」行。
5. **graph_node_topology 重构**：四元组返回值升为 `GraphNodeTopology` 结构体（前置/
   后续/禁止清单/总数/概念面）；agent 生成路径注入 `GraphLessonContext.concept_faces`，
   单节重写路径的「学习图节点上下文」同段渲染——两条图路径同一概念名相口径。
6. **提示词条款**：
   - 通用五条（R1 先推后练/R4 档位→内容形态/R6 汇合渐隐/R7 真对照/R8 例题密度）进
     `GENERATE_LESSON_AGENT_SYSTEM`（agent 两流共享，域中立、无档位声明时按节任务把
     握深浅）+ 传统课程 `CONTENT_DISCIPLINE_BLOCK` 进 `section_outline_system`（规划
     面两句：推导先于练习的节序、例题密度递进）与三风格节正文提示词（standard/
     socratic/feynman）。
   - 概念面条款（R2 引用名开讲/R5 首现引入）+ 用名一致性只落图路径——随概念面段渲染
     进 agent 用户回合（`lesson_user_text`），传统路径无概念挂接不出现（测试钉住）。
   - R9 叙述声部 + 跨课预告废除（契约 v2.6，j01 预留位）：`NARRATIVE_VOICE_BLOCK`
     进三风格节正文提示词；节大纲加「小结收本课以内、不规划预告节」条款；agent 系统
     提示词的成色块同义覆盖。契约文档 §8 对账表两条更新为已落地。
7. **评审判据观察条**（audit_findings，非阻断 warning）：①概念面用名——正文同现引用
   名与正名 = `naming_mixed`、只用正名 = `naming_canonical_only`、首现概念名字全篇
   未出现 = `first_seen_unintroduced`（仅图路径）；②R9 声部——`narrative_self_reference`
   （本课/这一课/本课时）与 `narrative_structure_words`（本节将/下一节将/接下来我们
   将/如前所述），两管线通用、逐类点名命中位置。观察条不阻断发布（danger 面不变）。
8. **smoke `content` 子命令**：立两节点课程（概念挂接 + 引用名 + 别名池真实落库）→
   走 `generate_lesson_content` 真实管线（`wire()` 增挂 LiveLessonContentAgentEngine +
   长任务观测旁路，引擎自行解析 provider——与生产同形）→ 读数打印（小节数/题目数/
   正文字数/首现与用名标注/观测目录）。读数面非门禁：硬判据 = 管线走通并发布；无凭据
   临时库如实失败（structure 冒烟同纪律）。
9. **测试**（+9）：首现闭包五形态、引用名 tiered 行、概念面渲染（图路径有/传统路径
   无）、审计观察条三形（混用/只用正名+首现缺席/传统路径仅 R9 生效）；ai-agent 1094
   全绿。
10. **验证**：`cargo test -p nomifun-learning` 全绿、`cargo test -p nomifun-ai-agent`
    1094 全绿、`cargo check --workspace` 干净、`cargo fmt` 按包过、`bun run typecheck`
    60 错 = 存量基线（零新增）。
