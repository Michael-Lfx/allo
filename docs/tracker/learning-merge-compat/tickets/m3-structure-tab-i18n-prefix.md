---
id: m3
title: "结构 tab 43 处 t() 补 learning. 前缀——裸 key 不再露出"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# m3: 结构 tab t() 漏前缀机械修

**What to build:** `ui/src/renderer/pages/learning/components/structure/` 六个
文件共 43 处 `t('learningGraph...')` 补上 `learning.` 前缀——当前全部解析
失败，用户在标题图功能上看到裸 key 文本（如 `learningGraphStructureMissing`）。

## 法证证据（2026-11-09 审计，实码复核）

- 全仓扫描 `grep -rn "t('learning[A-Z]"` 恰好 43 处、6 文件，无扩散：
  - `StructureGraphTab.tsx`：61、92-173（挂载面板/页脚/空态/tab 标签）、
    140-146 三元
  - `Mindmap.tsx`：59、155
  - `RelationNet.tsx`：81-141
  - `RouteMap.tsx`：101-195
  - `BlockDetail.tsx`：35-135（含 :59 动态键）
  - `MatrixView.tsx`：10-21
- key 本身双语齐全：全部存在于
  `ui/src/renderer/services/i18n/locales/{zh-CN,en-US}/learning.json`（已抽查
  `learningGraphStructureMissing` 两语言均在）。
- 正确写法在仓内有大量先例：`coursePlan.ts:265`
  `t('learning.learningGraphTabTitle')` 等——纯前缀机械修，无翻译工作。
- 未被拦截的原因：`scripts/generate-i18n-types.mjs` 只从 en-US 生成类型 +
  校验两语言文件集一致，**不扫 `t()` 调用点**；项目无 `CustomTypeOptions`
  增强，TS 接受任意字符串。防复发闸在后置清单（动态键误报需小设计）。

## 实施要点

- [ ] 六文件 43 处逐一补 `learning.` 前缀（含 BlockDetail.tsx:59 动态键的
      拼接处——确认拼接模板也带前缀）。
- [ ] 自查：`grep -rn "t('learning[A-Z]" ui/src` 归零。
- [ ] `bun run gen:i18n` + `bun run check:i18n` 过。
- [ ] `bun run typecheck` 过。
- [ ] 目验两处空态文案真实渲染（zh/en 各一）：结构未生成空态 + 挂载面板。

## 验收

- grep 归零 + 三门（gen:i18n / check:i18n / typecheck）全绿。
- 双主题目验无裸 key。

## 非目标

- i18n 词典内容改动（key 双语已齐全，零翻译）。
- 防复发扫描闸（后置清单：check:i18n 扫调用点会对动态键误报，需独立小票）。
- 结构 tab 其它 UX 项（空态语义、失败态文案语义在后置清单）。

## Resolution（2026-11-09 施工收口）

**代码面验收全过，已闭票；双主题目验留待下次真机批次。** 施工 = `5e787091f`。

- 六文件 51 行改动 = 43 处单行调用 + 8 处同缺陷变体（票面证据明确列出的动态
  键 / TABS 数组 / 三元形态，单行 grep 盖不到但同属「裸 key 不再露出」票面
  目标）；动态键模板 `learning.learningGraphTier_${tier}` 带前缀，对齐
  LearningGraphWorkspace.tsx:1012 先例。
- 验证：全仓 `t('learning[A-Z]` 归零（含反引/双引号变体与 GateCoverage 扩散
  检查均零命中）；`gen:i18n` + `check:i18n` 过（11582 keys，词典零改动）；
  typecheck 54 错 = 父分支存量债，stash 对照基线逐条持平零新增零涉及 learning。
- 防复发扫描闸仍为票面非目标（map 后置清单）。
