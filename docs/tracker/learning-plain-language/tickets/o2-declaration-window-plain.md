---
id: o2
title: "已会声明区口语化——门口/铸出/幂等/读侧/通道五连清（E2 重灾区）"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# o2: 已会声明区口语化（词典 J1/J2/J3 重灾区）

**What to build:** 已会声明（Declared Known）功能区的用户文案全部口语化，
以下 key 逐条改写（改写建议来自词典面探查，施工时允许微调措辞、不允许
保留被点名的实现词）：

| key | 现状（病） | 改写方向 |
| --- | --- | --- |
| `learningGraphBasisDeclareDone` | 「铸出新概念 N 个、复用既有概念 M 个（零节点、零排课、零 XP）」 | 「已记录这些内容你已会：不再安排学习，也不计 XP」+ 明细行 |
| `learningGraphBasisConfirmContent` | 「按声明铸出概念并入登记表…」 | 「确认后勾选内容视为已掌握，不再生成或安排，也不计 XP」 |
| `learningGraphBasisDeclareNoop` | 「本次零改动（幂等增量）」 | 「这些内容之前已声明过，本次没有变化」 |
| `learningGraphGateHint` | 「门口判据与其覆盖的知识块（读侧参考）：满足判据 = 覆盖块已会」 | 「这些自测题对应的知识组：能答上来 = 这部分你已会」 |
| `learningGraphGateCounts` | 「N 条门口判据」 | 「N 条入门自测」 |
| `learningGraphWindowBody` | 三术语一句（生长首批/入门判据/声明门口） | 「开始前可先勾选你已会的内容并确认，这部分不再安排学习；不勾也行，直接开始」 |
| `learningGraphBasisDoorAria` | 「勾选已具备的门口」 | 「勾选你已经掌握的内容」 |
| `learningGraphBasisOff` | 「已会声明通道未开启」 | 「此课程暂不支持声明已会」 |
| `learningGraphDoorAria` 族其余门口系 key | 「门口/门」悬空 | 统一改「入门检查/自测」（同域 key 一并核对） |

**关联组件呈现**（与 GateCoverage 卡相关，但布局重构归 o6）：行内
door.id Tag、basis.doors id 串直出的口语化归 o4；本票只管词典措辞。

## 法证证据（2026-11-11 词典面探查 J1 #3/#16、J2 #2/#3/#4/#5/#6/#7、J3 #1/#2）

- 声明窗口是建课新用户落地首屏（零节点声明窗口）——用户第一个撞见的
  功能面，「铸出/登记表/读侧/门口」四词齐发。
- en-US 与 zh 同病（minted/registry/idempotent increment/gate）——双语
  同步改。

## 实施要点

- [ ] 上表 key 逐一改写（双语对称）+ 同域「门口」系 key 全量核对
      （`rg "门口" zh-CN/learning.json` 清零或只剩隐喻保留清单内的用法）。
- [ ] 断言：涉及 key 的既有测试更新 + 真实词典 JSON 断言（红线 2）。
- [ ] `bun run gen:i18n` + `check:i18n` + typecheck。

## 验收

- zh/en 两语「门口|铸出|登记表|幂等|读侧」在已会声明区 key 中归零。
- 真机目验：声明窗口 + GateCoverage 卡全流程读一遍（两语）。

## 非目标

- 结构区其余术语（o4）、布局重构（o6）、后端 WS text 帧（c3 裁决保留）。
- 「已会声明」功能名本身（产品名保留）。

## Resolution（2026-11-11 施工收口）

**代码面验收全过，已闭票；真机目验（声明窗口 + GateCoverage 两语读一遍）与欠账合并执行，FAIL 按票重开。**

- 施工 = `9ab13389a`：票面 9 key 双语改写 + **同域新发现 5 处**（`learningGraphGateTitle` 过门自检→入门检查、`learningGraphBasisFrozenHint` 仅 en 侧 gates 清零、`learningGraphBasisDeclaredDoors` 标签、`learningGraphStructureReadyToast` 声明邀请句、`learningGraphGateDoorCoversNone`→「暂无对应知识块」）。
- **防回潮栅栏**：新测试 `declarationPlainLanguage.test.ts` 做声明区 key 家族负向扫描——zh 门口/铸出/登记表/幂等/读侧/过门、en mint/registry/idempotent/gate/criteria/read-only 归零 + 家族 key 双语对称断言（后续塞回实现词直接红灯）。
- 与 o4 的合并裁定（已落在集成树）：DeclareDone/DeclaredDoors **措辞归 o2、占位契约归 o4**（`{{doorsCount}}/{{blocksCount}}` 计数占位替代 id 串；o2 原期望同步到合并值，minted/reused 退出用户面、组件死参数删除）；`learningGraphAuditUnavailable` 归 o2 口径。
- zh 剩余 2 处「门口」（StructureSummary/StructDoorRefsLabel）归 o4（已在 o4 落地）；`learningGraphStructureReadyToast` 与 o4 的潜在同行冲突以 o2 改写为准合并。257 绿（施工时）→ 316（全波终态）绿。
