# ADR-0007: 节内容 AI 建议重写与用户手动编辑

Status: Accepted

Date: 2026-09-14

## Context

ADR-0003 落地了单节重写，但产品面上有两条缺口：

- **重写入口只覆盖失败节**：前端仅在 `status='failed'` 的节上出 Alert 重写按钮。
  学习者对正常（ready）节的表述有异议时，既不能让 AI 换个写法，也不能自己动手改。
- **重写不接受用户意图**：`GenerateLessonRequest` 只有 `provider_id`/`model`，
  重写提示词没有"用户想要什么"的输入位，重生成结果与原文同分布，异议无法被表达。

连带问题：正文变更后，绑定在该节上的题目（旧规范内容节直挂题；新规范练习轮按
生成时前置内容出题）可能失配。

## Decision

**AI 建议重写（对 ready 节开放）**：

- `GenerateLessonRequest` 增加可选 `feedback` 字段（自由文本建议），穿入
  `rewrite_section_body` → `build_section_rewrite_prompt`；为空时与既有失败节
  重写行为完全一致（同分布重生成）。
- 前端对 ready 内容节提供常驻 mini 操作按钮，打开对话框：可选输入建议 → 确认
  → 复用既有 `POST /lessons/{id}/sections/{key}/rewrite`，成功直接替换正文；
  失败节 Alert 按钮保留原样。不暴露模型选择（用默认）。
- 走既有确定性单节管线（visual 承诺质检 + 降级兜底不变），不加新端点。

**既有缺陷修复（单节重写落库 SQL）**：

- 实现手动编辑复用 `persist_rewritten_section` 时发现其 summary 同步语句
  `UPDATE learning_lessons SET summary = ?, updated_at = ?` 引用了
  `learning_lessons` 上不存在的 `updated_at` 列（迁移 015/040 均未引入）——
  ADR-0003 的单节重写在真实库上必然在事务内失败回滚。修正为只写
  `summary`；失败节重写路径因此同一处修复（行为从"必败"变为"可用"）。

**手动编辑（用户自改正文）**：

- 新路由 `PUT /api/learning/lessons/{id}/sections/{key}/body`：仅更新
  `body_md`，`version+1`，status 保持 'ready'，不记录编辑来源（重写提示词已带
  当前正文作为上下文，来源标记暂无消费方）。
- 前端对话框复用 CodeMirror MarkdownEditor；仅对有正文的节开放（failed 节走
  既有 AI 重写路径，不手写空白节）。

**边界**：

- 练习节（practice）两个功能都不开放——练习轮的题目是一等实体，编辑其指令性
  正文意义有限；与现状（练习节无重写入口）一致。
- 正文变更后题目失配：本期不处理。题目有作答记录（attempts 绑 activity_id），
  自动替换会破坏作答历史与步进门禁；为节级重出题补 `section_key` 绑定是独立
  增量，留待后续按需升级（检测+提示 → 一键重出题）。

## Consequences

- 学习者对单节内容的三条修复路径闭环：失败节重写（既有，随本次修复恢复可用）、
  正常节带建议重写、正常节手动编辑；三者都只动目标节，其余节与题目不动。
- `feedback` 为空与既有行为逐字节一致（提示词层）。
- 题目失配风险已知且被接受：旧规范内容节直挂题在正文大改后可能不再对应正文；
  用户可经 QuestionManager 自行补题。
