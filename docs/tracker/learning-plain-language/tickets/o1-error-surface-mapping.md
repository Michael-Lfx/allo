---
id: o1
title: "错误呈现统一映射——errorMessage 单源改造，25 个直泄点收口（E1 根因）"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# o1: 错误呈现统一映射（最高杠杆，全模块直泄根因）

**What to build:** `pages/learning/utils.ts` 的 `errorMessage(t, error)` 单源
改造：`BackendHttpError` 按 status/body 内容映射学习域词典（409 生成冲突
保留 d7 既有友好文案零改动；404 → 「内容不存在或已删除」；5xx → 「服务暂时
不可用，请稍后再试」；网络/超时 → 「网络不稳定，请稍后再试」；未知 →
通用失败句），**原文一律不进 UI**（console/日志保留）。收编绕过点：

1. 5 个 `Modal.error({ content: errorMessage(t, actionError) })`
   （LearningGraphWorkspace regenerate/mount/repair/declare/grow）——映射后
   语义已友好，但强制弹窗降级为 `Message.error`（与同文件其它动作一致），
   除非施工时发现某处确需阻断式呈现。
2. `error.message` 直用点收编：`LessonStudy.tsx:795,811`、
   `LessonQuestionDialog.tsx:154-156`、`useCourseCreation.ts:222`（回退
   既有 loadBasesFailed 词典）、`:136`（describeError 固定「课程信息读取
   失败」）。
3. `GrowthTaskPill.tsx:67` failed detail 直出 → 通用失败句 + Tooltip 收纳
   detail（诊断不丢）。
4. 系统动作簿 `row.reason` 直出（`:998-1008`）→ reason 分类映射，无匹配
   显示中性「未知原因」。
5. 首屏拼接点：`DeclarationWindow.tsx:43-47`、`index.tsx:312`、
   `LearningGraphWorkspace.tsx:860,978,1096`、`LessonStudy.tsx:222` 的
   `` `${t('learning.loadFailed')}: ${error}` `` → 固定失败句（+可选折叠
   详情），契约门报文不再上屏。

**前置核查（票面硬性第一步）**：读 `scripts/check-error-surface.mjs` 契约
（改错误消费可能撞闸）；确认 `BackendHttpError` 是否已暴露结构化
`status/body`——若无，允许在 `ui/src/common/adapter/httpBridge.ts` 给类加
只读字段（additive，map 非目标允许的唯一 common 改动），除此之外 common
零改动。

## 法证证据（2026-11-11 组件面探查）

- 根因：`httpBridge.ts:252-256` `` `Backend POST {path} failed (409):
  {JSON.stringify(body)}` ``；`utils.ts:123-125` 原样返回。约 25 消费点
  全量清单见探查报告（ReviewSession/QuestionDialogs/QuestionManager/
  useReviewSession/useGrowthTaskPills/useCourseLearning 等）。
- 实锤案例：结构生成契约门报文 `[gd.terrain.keys] 未知顶层键「type」…`
  具备整句上屏通道（DeclarationWindow + 建课失败面板）。
- 好样本：d7 的 409 冲突文案（`learningGraphStructureConflict`/
  `inflightTaskToast`）措辞友好——映射后与其同水位。

## 实施要点

- [ ] 前置核查两件（契约脚本 + BackendHttpError 字段）写进提交说明。
- [ ] errorMessage 映射 + 5 组绕过点收编（清单见上）。
- [ ] 新词典 key 双语对称 + 真实词典 JSON 断言遍历（红线 2）。
- [ ] `bun run check:error-surface` 过（契约不变更）。
- [ ] 测试：errorMessage 映射矩阵（409/404/5xx/网络/未知/普通 Error）+
      绕过点收编后的渲染断言（grep 原文不再上屏的负向断言）。

## 验收

- 全模块 toast/Modal/面板 grep `error.message|: ${error}` 人面直出归零
  （console 日志除外）。
- 真机：人为触发一次 404/一次 5xx/一次 409——全部中文友好句；契约门报文
  不可见。
- 闸门：typecheck / check:i18n / check:error-surface / learning 测试全绿。

## 非目标

- 后端错误 message 构造（消费侧映射兜底，map 非目标）。
- error-surface 契约本身变更。
- v2 流（知识库/描述流）与图课程流的差异化错误文案（统一映射即可）。
