---
id: m9
title: "定义用语表与只读审计落地——base 语料移植 + 定义审计纯函数 + 审计端点与概念 Tab 标记"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# m9: 定义用语表与只读审计落地

**What to build:** 移植 learnhub #171/#173 定义接地一阶（m4 定案范围 (a)+(b)）：
定义审计纯函数模块（CJK n-gram 候选 + 最长优先掩码 + 自指/互指检查）、定义
用语表三层装配（登记表活跃名 ∪ 仓库级 base 语料 ∪ 课程豁免/增词）、GET 只读
审计端点 + 概念 Tab 行级标记；碎片裁决面（FragmentVerdict）在 m3 落地后接。
纯本地零 LLM——无双臂需求，验证 = 夹具红绿（m4 Resolution）。

## allo 现状

- 概念定义：`learning_concept_registry.definition`（068:187-203）+ j04 别名
  携带定义（AliasRecord.definition）；零定义质量检查。
- m3 将给评审输出加 term_provenance 表（挂号程序化）——本票碎片裁决面是其
  旁列扩展（独立清单，不并表）。
- findings 通道（j09 warn + k02 水位消费）接碎片 report-only 输出。
- 概念 Tab：`LearningGraphWorkspace.tsx:513`（GraphConceptRowView 表）。

## learnhub 参考（实码先读）

- `src-tauri/src/domain/concepts/definition_audit.rs`：`candidate_terms`
  （CJK 1-4 gram 枚举，两过滤：已知词/纯虚字组合；宁滥勿缺）、
  `grounding_runs`（最长优先已知词掩码后未覆盖极大段 ≤40/定义；单字只作
  边界 token——单字吞复合缺陷纪律）、`self_references`（ISO 704）、
  `mutual_cycles`（A↔B）。
- `src-tauri/src/domain/concepts/lexicon.rs`：三层已知 = 登记表活跃名 ∪
  `BASE_YAML = include_str!("../../../lexicon/base.yaml")`（127 词 +
  function_single；`BaseLexicon{version,function_single,words}`）∪ 课程
  `lexicon-course.yaml`（`CourseLexicon{door,words,ignore}`）；function_single
  与审计掩码**互钉测试**；铁律 = 课程只许豁免与增词，不反向污染仓库级。
- `src-tauri/src/app/commands.rs:469-476` audit_definitions →
  `course_domain.rs:596-660` definition_audit_report（全登记表 × 三层 →
  标记条目，零写入）。
- 夹具：`docs/research/fixtures/philosophy-intro/`（预埋自指「怀疑论」/互指
  实体↔属性，跨域零失败实证）+ Rust 域夹具（518c902）。

## 实施要点

- [ ] **base 语料移植**：`nomifun-learning/lexicon/base.yaml` 原样移植
      （m4 定案；不是提示词资产，金样三闸不适用）+ function_single 与掩码
      逻辑互钉测试。
- [ ] **定义审计纯函数模块**（`learning_graph/definition_audit.rs` 或
      domain 层）：candidate_terms / grounding_runs / self_references /
      mutual_cycles 全套移植，单字只作边界 token。
- [ ] **三层装配**：登记表活跃名（DB）∪ base ∪ 课程豁免/增词（v1 存储从简：
      structure_json 旁键或小表，实施定；只许豁免与增词）。
- [ ] **审计端点 + 前端**：`GET /courses/{id}/concepts/audit`（全登记表 ×
      三层 → 标记条目：缺口碎片/自指/互指，零写入）+ 概念 Tab 行级标记 +
      i18n 双语。
- [ ] **碎片裁决面（m3 后接）**：评审输出增 `FragmentVerdict[]`
      （term + noise|名词|note），机械核全覆盖；未处理碎片 v1 report-only 进
      findings（k02 水位消费）；enforce 阻断留开关位
      （`NOMI_LEARNING_LEXICON_ENFORCE`，v1 缺省 off）后置。

## 验收

- **夹具红绿（核心判据）**：移植/构造双域夹具（哲学入门 + Rust 域）——预埋
  自指定义、互指对、缺口碎片定义全部被审计抓住；干净定义零误报。
- **单测**：candidate_terms 两过滤；最长优先掩码（内角不被 内+角 吃掉）；
  三层装配优先级与课程豁免语义（只许豁免增词）；审计报告形状；旧快照兼容。
- **端点读数**：审计端点对空表/全标记/部分标记三态如实；零 LLM。
- `cargo test -p nomifun-learning` 全绿；typecheck/i18n/check 门过。

## 非目标

- provenance 表（m3 领地）；根类别集（雾区，m4 Resolution）；词汇裁决代理
  提案者（雾区）；enforce 阻断默认开（v1 off）。

## 备注

审计哲学（learnhub 同源）：「读侧先看见」——审计是缺口标记不是拦截门；
概念表结构评估结论遵守：不给 ConceptRecord 加属字段，改定义契约 + 根资产 +
只读投影。

## Resolution（2026-10-27 关票，implement-spec 波收口）

- **交付**（learn-v5/m9-lexicon-audit，5d406c2aa 等）：base.yaml 逐字节移植（148 行）+ `definition_audit` 纯函数全套（CJK 1-4gram 两过滤、最长优先掩码、**单字只作边界 token**、自指/互指）+ 三层定义用语表（登记表活跃名 ∪ base ∪ structure_json 旁键课程豁免——零迁移）+ `GET /concepts/audit` 只读端点 + 概念 Tab 行级标记 + 双域夹具（philosophy-intro + rust-ownership：预埋自指/互指/缺口全抓住，干净定义零误报）。
- **验收形态**：纯本地零 LLM——夹具红绿矩阵即判据（14 模块测 + 3 服务测），票面无双臂要求。
- **碎片面补接**（m3 落地后，learn-v5/m9f-fragment-verdict，dcaa04919..4e17f8572）：`FragmentVerdict` 独立清单（不并 provenance 表）+ 机械核全覆盖 + **report-only v1** 进 findings（k02 水位消费）+ `NOMI_LEARNING_REVIEW_FRAGMENTS`（面开关，缺省 on）+ `NOMI_LEARNING_LEXICON_ENFORCE`（块路径，**首个缺省 off 开关**，反转约定已注记）。真机：27/27 碎片全裁噪声（3 行抽检属实）、enforce 臂演示完整 degraded 阶梯（rows=0 invalid=2 retried=1 degraded=true）——assets/m9f-fragment-verdict-comparison-card.md；enforce 阻断支未自然到达（unhandled=0）如实注记。
