# ADR-0014: 脏库/大教材的课程结构发现——清点/分诊/证据结构/分批展开/覆盖披露

Status: Accepted

Date: 2026-10-22

## Context

「知识库 → 传统课程」管线此前以**目标感知采样**（40 文件/24KB/960KB 预算，
`generation/sample.rs`）承担全部结构发现：采样清单即大纲引擎能看到的整个
世界。2026-10-21 真机冒烟（[map 前情](../tracker/learning-kb-course-v2/map.md)、
[assets/dirty-kb-and-large-course-research.md](../tracker/learning-kb-course-v2/assets/dirty-kb-and-large-course-research.md)）
证实两类结构性失败：

1. **脏库**：真实用户库混入无关文件、翻译副本、重复版本、缺标题文件、网页
   推广尾段——采样不区分相关性与角色，噪声与正文同权进清单；文件名/目录序
   被当作阅读顺序真相。
2. **大教材**：105 文件 Rust 书撞 40 文件上限，课程只引用 34 个文件，第 14
   章缺失、15/16 章倒挂——截断静默发生，无任何披露。

用户前提（map 明示）：设计不得假设「库即教材」。

## Decision

用四段新管线取代采样路径的结构发现职责（采样路径保留为回退与无引擎路径）：

1. **Kb Inventory（清点）**：列出库内全部 `.md` 文件（排除 machinery 目录），
   读元数据与内容指纹（content_hash / 语言后缀 / 标题清单）。清点不看目标，
   只回答「库里有什么」。
2. **Kb Triage（分诊）**：每文件判角色——core / prerequisite / practice /
   mixed / noise / duplicate / uncertain。规则预分类零 LLM（元数据/空/纯导航
   → noise；翻译布局与同 hash → duplicate 指回 canonical）；剩余按 ≤40 文件/
   批交 LLM 判定，批失败拆半重试、单文件兜底。**零章节/零 core 才硬失败**；
   noise/duplicate 不进候选集但逐条带 reason（「为什么没进课程」可回答）。
3. **Evidence Structure（证据结构计划，SourcePlan）**：从候选证据（标题 +
   ≤120 字锚点片段）建概念图与章节骨架，拓扑排序校验前置；零证据/草稿连续
   失败降级空章节 → 调用方回退采样路径（不阻断建课）。文件名与目录序只是
   锚点线索之一，不是结构真相。
4. **Batched Expansion（分批展开）+ 覆盖披露（SourceCoverage）**：可用计划
   按章展开产 Blueprint（不再调大纲引擎），topic 对账台账记
   covered/skipped/uncertain 四态与锚点质量；跳过有 reason、锚点不精确有
   anchor_uncertain、H1/文件名兜底课显式 fallback——**静默截断在票面上被
   禁止**。四件套（inventory/triage/plan/coverage）以 v1 信封落
   `learning_courses.course_source_plan_json`（迁移 069），读侧未知版本降级
   `available=false` 不崩溃。

**成本门与 40 文件预算的新定位**：>40 文件或预估 >20 次 LLM 调用时建课前
展示估算与分诊摘要让用户确认（`POST /courses/generate/estimate`，规则分诊
零 LLM）；40 文件只作为单批展开/分诊批的预算，不再承担结构发现。

## Consequences

- **术语分立**：`SourcePlan`/`SourceCoverage` 只指来源结构计划与来源覆盖；
  `Structure`/`Coverage Gauge` 仍只指学习图（CONTEXT.md 词条互不混用）。
- **跳过与不确定可见**：WS 进度帧（triage_*/source_plan_*/coverage_ready）、
  成本门弹窗、课程详情「来源覆盖」卡三处共享同一分诊/覆盖读数；旧客户端
  忽略未知 phase，started/completed/failed 不变。
- **快照语义**：课程建成后 KB 变更不影响既有课程（快照即真相）；描述流/
  回退路径列上 NULL，不伪造快照。
- **展开失败回退**：t2 计划可用但 t3 展开失败（非取消）→ 回退现行大纲引擎
  路径，课程照常建成（degraded 不阻断），此时无快照。
- **legacy one-shot 兜底（无引擎）不跑结构发现**：它无法兑现 plan 的权威
  性，测试/直调路径的 LLM 调用面零变化。
- **已验证**：`cargo test -p nomifun-learning` 436 绿（含脏库守恒、翻译/
  重复 canonical、批拆半、快照四态）；真机三库冒烟见
  `learning-smoke course` 报告（本票 Resolution）。
