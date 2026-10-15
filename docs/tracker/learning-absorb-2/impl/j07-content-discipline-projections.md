---
id: j07
title: "内容成色纪律与教学上下文投影"
type: task
status: open
assignee:
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

- [ ] **通用纪律（R1/R4/R6/R7/R8）进两条管线提示词**：图节点正文（lesson_content_loop
      graph 分支）+ 传统课程（节大纲/节正文）：先推后练 / 档位→内容形态 / 汇合渐隐
      （assumes ≥4 逐级加要素）/ 真对照 / 例题密度（3–6 递进）——域中立、零新机械门、
      「写作取向不是格式模板」收尾
- [ ] **概念面条款（R2/R5）只进图节点路径**：引用名开讲（正名首现括注）/ 首现带引入
      （非首现一两句回收）
- [ ] **R3 投影词表 + 评审判据**：规划/正文辨认旧叫法（数据源 = j04 别名池）；评审判据
      增观察条（正文用名一致性、首现引入、R9 声部/结构词——观察面非阻断）
- [ ] **首现投影（祖先闭包不动点，零新存储）**：起点 = assumes 概念集；循环扫非
      archived 节点，教了任一 assumed 概念即纳入、并入其 assumes、teaches 计入
      ancestor_taught 直至不动点；本节点 teaches 中不在 ancestor_taught = 首现；并行分支
      与 archived 不夺首现；`graph_node_topology` 注入首现行
- [ ] **引用名开讲**：迁移 `learning_lesson_concepts` 加 `ref_name` 列（nullable）；
      `apply_batch` 引用解析时记录模型实际用名；上下文 tiered 行渲染 `别名(档位;正名:
      正名)` 形态
- [ ] **传统课程落点核对**：通用五条进节大纲/节正文提示词；概念面不进（无挂接）
- [ ] **learning-smoke 增 `content` 子命令**：为一个节点生成正文落观测面 + 读数（字数/
      承诺兑现/小节数/首现标注）——读数面非门禁
- [ ] 验证：`cargo test -p nomifun-learning`（首现闭包不动点单测：并行分支/archived/
      空 assumes）+ 迁移测试 + `bun run typecheck`

**Resolution:**（收口时写）
