---
id: h1
title: "分诊批回包完整性：缺条目定向补判 + 单文件兜底修复，批缺条目→uncertain 损失率归零"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# h1: 分诊批回包完整性硬化

**What to build:** 修复「LLM 分诊批回包缺条目 → 拆半 → 单文件兜底仍缺 → 保守 uncertain」链路上每一环的可避免损失。真机复验中成套库 7/19、杂乱库 7/27 文件走完这条链落 uncertain——内容没丢(照常进课),但分诊精度大幅损失,报告里一片「模型未返回该条目的判定」。

**Blocked by:** 无(独立票)。

**Status:** ready-for-agent

## 证据与根因假设(实施时先诊断再修)

- 复验工件:`C:\Users\test\Desktop\work\learning-smoke-course-20261022-verify\reports\verify-{clean,dirty}.json`(triage.files 里 reason 含「单文件分诊仍解析失败」的条目)。
- 链路:`generation/triage.rs` LLM 批(≤40/批,一批常为全库)→ `structured_batch` 部分覆盖只补缺失子集 → 拆半 → 单文件批仍报缺条目。
- **单文件批仍缺条目是关键疑点**:输入只有一个文件,模型仍「未返回该条目的判定」,大概率是提示词/解析形状问题而非模型能力——例如单文件重发时仍要求"数组"形状、或未强调"必须恰好返回 N 条"、或模型改回了对象形状被当作缺条目。先写诊断测试(scripted completer 模拟:数组缺条目/对象按路径键/条目数不符/截断)复现真实回包形态,再对症修。

## 实施要点

- [ ] **诊断**:用 scripted completer 固定复现「批缺条目」与「单文件仍缺」;如能从 observation 归档(`data/observation`)取到真机原始回包更好,对齐真实形态。
- [ ] **批提示词硬化**:明确「必须为输入清单中的每个文件恰好返回一条判定,按输入顺序,rel_path 原样回显」;给文件编号;声明"遗漏任何文件都算失败"。
- [ ] **解析宽容**:除数组外接受「按 rel_path 为键的对象」形状;条目字段名小差异(如 role/kind)按同义词映射;部分覆盖时**只补判缺失子集**并换定向提示(「只判定以下 N 个文件」),不是原样重发全批。
- [ ] **单文件兜底修复**:单文件批的提示词与解析按单条目形状专门化;确保「单文件批合法空回包=显式无判定」与「解析失败」可区分。
- [ ] **保守语义不变**:以上全部失败仍落 uncertain+reason(现有行为),不引入静默丢弃。
- [ ] 计数披露:triage 报告增加「批补判次数/仍缺条目数」类读数(字段名实施时定,信封向后兼容)。

## 非目标

- 不换模型/调温度(不换脑,只修协议)。
- 不改 structured_batch 的 Call/Parse 二分契约与拆半语义。
- 不动 40/批的批量上限。

## 验收

- **单测**:scripted completer 模拟数组缺 k 条 → 定向补判缺失子集并成功;对象形状回包可解析;单文件批缺条目 → 修复后能拿到判定;全部手段用尽仍缺 → uncertain(语义回归测试)。
- **真机复验**:同 verify 目录同库重跑(clean 19 + dirty 27),「单文件分诊仍解析失败」类 uncertain 从 7 降到 0(或 ≤1 且有解释);其余读数不劣化(章节结构/课时数/0 兜底)。
- `cargo test -p nomifun-learning` 全绿;`cargo check --workspace` 干净。

## Resolution（2026-10-22 关票）

**交付**（分支 `learn-v2/h1-triage-batch-completeness`，8 提交 `420d690dc..84a520ab8`，经 `febcdda1e` 并入集成分支）：

- **根因实证**（observation 归档真机回包）：模型把提示词列表标签前缀回显进 `path`（`{"path":"文件 01-前言.md"}`），`map_llm_reply` 精确匹配拒收——回包本身完整、非截断、非缺条目、形状无误。
- 五层修复：① `resolve_requested_path`（trim → 精确 → 剥离标签前缀，前缀不含 `/` 且余部唯一命中才收；条目 path 与 `duplicate_of` 目标同用）；② 解析宽容（`files` 接受按 rel_path 为键的对象形状；`path/rel_path`、`role/kind` 同义映射；未识别形状仍 Parse 错误走拆半，不静默吞）；③ 提示词硬化（编号清单 `1. {path} :: …` 消灭可回显的「- 文件 」标签 +「恰好 N 条、按输入顺序、rel_path 原样回显、遗漏即失败」契约）；④ 缺失子集定向补判（`BatchPass::{Initial,Repair}`，repair 提示「只判定以下 N 个文件」；Call/Parse 二分与拆半语义未动）；⑤ 单文件兜底按单条目形状专门化，「模型显式无判定」与「解析失败+诊断」reason 可区分。
- 计数披露：`batch_repair_count`/`still_missing_count` 上 `KbTriageReport` + 课程计划 DTO 视图（serde default + 零值不序列化；向后兼容有钉死测试）。

**测试**：`cargo test -p nomifun-learning` 442→458（+16，含 3 条真机回包摘录形态：批前缀回显 call#1、单文件重试 call#6、混合前缀 call#22）；全绿零回归。

**真机复验**（`C:\Users\test\Desktop\work\learning-smoke-course-20261022-hardening\`，clean×2 + dirty 全 EXIT=0）：「单文件分诊仍解析失败」clean 7→0（两跑）、dirty 7→0；dirty 余 2 个 uncertain 为新形态「重复判定缺少可核对的 duplicate_of」（模型判 duplicate 未给 dup_of，保守保留有理由，不属本票目标类别）；计数守恒 19/27、fallback=0、degraded=false。

**实施偏差**（不违背票面）：快照版本保持 1（加性可选字段零值不序列化，双向可读）；smoke CLI 读数与 UI 类型未扩展（超出本票 crate 范围，计数经报告信封+DTO 视图披露）；路径归一同样作用于 `duplicate_of` 目标——两轴评审判定保守（唯一命中、无斜杠前缀、无静默丢弃）可接受。

## 备注

- 这是最影响日常观感的一票:分诊报告里近三成文件标 uncertain,会让用户怀疑分诊没用。
