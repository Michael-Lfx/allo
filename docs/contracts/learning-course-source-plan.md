---
contract: learning-course-source-plan
version: "1.0"
date: 2026-10-22
status: 定稿（learning-kb-course-v2 t1–t5 裁决落地；ADR-0014 与 CONTEXT.md Kb Inventory/Kb Triage/Source Plan/SourceCoverage/Mixed File/Anchor Uncertain/Uncertain 词条为最高约束）
sources: [docs/tracker/learning-kb-course-v2/map.md, docs/tracker/learning-kb-course-v2/assets/dirty-kb-and-large-course-research.md, docs/adr/0014-dirty-kb-course-structure-discovery.md]
---

# 课程来源计划契约 v1.0（SourcePlan / SourceCoverage）

> 传统建课「知识库 → 课程」新管线的落盘与读侧契约：t1 清点/分诊、t2 证据
> 结构计划、t3 展开覆盖以**一个版本化 JSON 信封**落在
> `learning_courses.course_source_plan_json`（迁移 069）。本契约约束信封的
> JSON 形状、列约束、旧行兼容、锚点不变量与关键语义；类型单源住
> `nomifun-learning` 的 `generation/snapshot.rs`（信封）与 `triage.rs` /
> `structure.rs` / `expand.rs`（三段产物），DTO 视图住 `models.rs`。

## 0 最高约束

- **术语铁律**：`SourcePlan`（来源结构计划）/`SourceCoverage`（来源覆盖）
  只指本契约的传统建课管线；学习图的 `Structure`（宏观参考地图）与
  `Coverage Gauge`（学习存量徽章）是另外两个词，任何界面/文档不得混用。
- **快照即真相**：课程建成后 KB 变更不影响既有课程；读侧只读快照，永不
  实时重算 KB（与 blueprint/samples 快照语义一致）。
- **NULL 不伪造**：列上 NULL = 旧课程/描述流/未走「计划 → 展开」管线
  （含展开失败回退大纲引擎路径）的课程。这些课程**没有**来源计划，读侧
  视图整体不出现（UI 不显示空卡），绝不落空对象冒充。
- **静默截断禁止**：一切跳过/降级/兜底必须带 reason 可见——快照 warnings、
  分诊 excluded/duplicates、覆盖 skipped/anchor_uncertain/fallback 三层承载。

## 1 存储形状与 CHECK 约束

```sql
-- 069_learning_course_source_plan.sql
ALTER TABLE learning_courses ADD COLUMN course_source_plan_json TEXT CHECK (
    course_source_plan_json IS NULL OR (
        json_valid(course_source_plan_json)
        AND json_type(course_source_plan_json) = 'object'
    )
);
```

- Nullable、无 default、**旧行不回填**：NULL 与 `{}` 必须可区分。
- CHECK 只放行 JSON 对象（信封恒为对象；数组/标量/非法 JSON 拒绝），仿
  068 `samples_meta_json` 先例。
- 写侧在课程导入的**同一事务**内写列（与 modules/lessons 同生共死）；回滚
  方案 = 读侧忽略本列（只加列，不删数据）。

## 2 信封 JSON 形状（v1）

```jsonc
{
  "version": 1,                       // 信封版本号（§6 版本策略）
  "inventory": {                      // t1 清点（体积控制后）
    "files": [ { "rel_path": "...", "kb_id": "...", "bytes": 0,
                 "content_hash": "...", "headings": ["..."],   // ≤16 条
                 "preview": "..." } ]                           // ≤200 字
  },
  "triage": {                         // t1 分诊
    "files": [ { "rel_path": "...", "kb_id": "...", "role": "core|prerequisite|practice|mixed|noise|duplicate|uncertain",
                 "confidence": 0.0, "reason": "...", "duplicate_of": null } ],
    "counts": { "core": 0, "prerequisite": 0, "practice": 0, "mixed": 0,
                "noise": 0, "duplicate": 0, "uncertain": 0 },   // 七角色之和 = 文件总数
    "excluded":  [ { "path": "...", "role": "noise|duplicate", "reason": "..." } ],
    "duplicates": [ { "path": "...", "duplicate_of": "...", "canonical_reason": "..." } ],
    "degraded": false,                // 规则分诊（LLM 判定缺席/失败）
    "warnings": []
  },
  "plan": {                           // t2 证据结构计划（SourcePlan）
    "goal": "...",
    "evidence": { "counts": [], "gaps": ["..."] },
    "concept_graph": { "nodes": [], "edges": [] },
    "chapters": [ { "title": "...", "summary": "...", "learning_objectives": [],
                    "covers": [], "prerequisites": [],
                    "source_anchors": [ /* SourceAnchor */ ],
                    "content_type": "concept|practice|reference" } ],
    "coverage": { "topics": [], "files": [], "excluded": [] },  // t2 台账（计划侧）
    "warnings": [], "degraded": false, "plan_hash": "..."
  },
  "coverage": {                       // t3 展开覆盖（SourceCoverage）
    "goal": "...",
    "chapters": [],                   // 逐章展开报告
    "lessons": [ { "fallback": false } ],
    "files":   [ { "status": "covered|partial|skipped|uncertain" } ],
    "topics":  [ { "status": "covered|skipped|uncertain" } ],
    "warnings": []
  },
  "warnings": []                      // 信封级披露（快照体积截断事件等）
}
```

精确字段以 `generation/snapshot.rs` + `triage.rs`/`structure.rs`/`expand.rs`
的类型为准（serde snake_case）；`counts` 各角色之和恒等于库内 md 文件总数
（守恒律，测试钉死）。

## 3 读侧视图（DTO 面）

- **列表摘要**（`CourseSummary.source_plan: Option<CourseSourcePlanView>`，
  NULL 时字段不出现）：`snapshot {version, available}` +
  `triage`（七计数 + degraded + warnings 限长）+ `plan`（章/概念/边计数 +
  gaps + 章节映射摘要）+ `coverage`（topic 四态计数 + 前置/练习去向 +
  anchor_uncertain + uncertain 标签 ≤20 + skipped 示例 ≤20 + fallback 数）+
  `duplicates`（全量计数 + 示例 ≤20）。
- **详情明细**（`CourseDetail.source_plan_detail`，仅详情端点）：全量
  `excluded`（path+role+reason）与 `duplicates`（path+duplicate_of+
  canonical_reason），限长 200。列表端点只给计数/示例。
- **四态**：旧课程/描述流 → `source_plan` 不出现；新课 v1 → 完整视图；
  未知 `version` → `snapshot.available=false`（计数不展示，不崩溃）；
  v1 形状损坏 → 同 `available=false`。
- **成本门估算**（`POST /api/learning/courses/generate/estimate`，写侧面）：
  规则分诊（零 LLM、`triage.degraded=true`）+ 文件/批/调用/耗时估算；
  `needs_confirm` = >40 文件或 >20 次预估调用（阈值后端单源
  `COURSE_ESTIMATE_CONFIRM_FILES/CALLS`）。

## 4 关键语义

- **mixed**：文件内相关与无关内容并存。处置 = 部分内容进入课程（相关段落
  作证据锚点）；**不是**整文件跳过，也**不是**整文件当干净正文。展示必须
  明说「部分内容进入课程」。
- **duplicate_of**：翻译副本（语言后缀布局）与同 content_hash 副本指回
  canonical（Q12：非翻译后缀原文优先 → README → 目标语言；同 hash：非翻译
  命名优先 → 最新 mtime）。`canonical_reason` 记录入选理由；无法判定翻译
  方向的孤儿副本落 **uncertain**，不排除。
- **anchor_uncertain**：主题归属文件可确认但锚点无法精确定位到标题/段落级
  （标题对不上原文 / 只剩 path 级兜底）。课时照常建，读数显式计数——披露
  引用精度打折，不是失败。
- **fallback 课时**：H1/文件名兜底生成的课时（无正文级证据时的确定性兜
  底）。不伪装成正常设计，读数醒目披露。
- **Uncertain**：规则与 LLM 都判不了时的显式保留态——带 reason、计数可见、
  不静默丢弃、不强行归 core。uncertain 过多（≥10 或超过 core）触发
  「建议先整理知识库」提示，不是硬失败。
- **失败策略**：只有零章节/零 core 硬失败；部分覆盖缺口建课 + 醒目
  warning；degraded/fallback/anchor_uncertain 必须有可见文案。

## 5 来源锚点不变量

1. 计划/课时引用的每个锚点 `rel_path` 必须来自清点清单（**防幻觉门**）；
   课时正文生成的 grounding 仍按课时 `sources` 走既有 24KB/40KB 预算。
2. 文件级角色（role）只是诊断面；覆盖以 **topic** 为主单位，文件状态由
   topic 聚合（covered/partial/skipped/uncertain）。
3. 七角色计数守恒：`core+prerequisite+practice+mixed+noise+duplicate+
   uncertain = 文件总数`；每个 excluded 条目必须落在 noise/duplicate 角色。
4. 快照体积控制只在序列化入口截断（preview ≤200 字、headings ≤16 条），
   截断事件聚进信封 warnings；**不改内存类型语义、不截断 plan/coverage
   结构骨架**（审计需全文按 rel_path 回 KB 读）。

## 6 版本号策略

- 信封 `version` 单调递增；v1 = t1–t3 冻结形状。
- 读侧见未知版本 → `snapshot.available=false`（降级不崩溃不伪造）；
  写侧永远写当前 `COURSE_SOURCE_PLAN_SNAPSHOT_VERSION`。
- 加字段（读侧可忽略）→ 递增 version；改语义/删字段 → 递增 version 且
  读侧按能力降级。**不为演进拆列**——一列一信封，演进靠 version。
- WS 进度帧（`triage_started/triage_completed/source_plan_started/
  source_plan_ready/coverage_ready`）与 DTO 一样按「旧客户端忽略未知
  phase」前向兼容；`started/publishing/completed/failed` 顺序与语义不变。

## 7 旧行与回退兼容

- 069 之前的课程行：列 NULL，读侧视图不出现，UI 不显示空卡。
- 描述流课程（无知识库）：永远 NULL。
- 计划可用但展开失败（非取消）回退大纲引擎的课程：NULL（课程不是按 plan
  建成的，快照不得冒充）。
- legacy one-shot 兜底（无引擎注入）：不跑结构发现，NULL。
- 回滚：读侧忽略本列即可（列可留库不动）。
