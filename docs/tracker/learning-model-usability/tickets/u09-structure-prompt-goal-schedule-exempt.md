---
id: u09
title: "结构提示词「和」自检句对齐门 + goal 字段豁免排期扫描"
type: task
status: closed
lane: C
blocked_by: [u08]
part_of: ../map.md
---

# u09：结构生成的两处提示词/门不一致

## 问题与证据（审计④#3#4，证实）
- `goal-decomposition.md` ~:32 自检句写「词边『和』…豁免」，契约却是「『和』不设词首/词尾豁免，是否捆绑交 L2」（`structure_contract.rs` ~:264-268），L2 判 bundle=true 即打回——模型自认合规仍被拒，原样重交会同指纹早停。
- `gd.no_schedule` 全文扫描只豁免 quality/disclaimer（`structure_contract.rs` ~:843-844），而 goal 必须逐字回执目标原句——目标含「计划/进度/小时」类词（如「掌握项目进度管理」）时，合规回执必然红，唯一修法（改写目标）又被禁止：死锁。

## 实施要点（机械，不挂开关）
- [ ] 自检句改为与门一致：「名字里含『和/与』时一律交裁决；拿不准就改用不含并列字的规范名」；如该提示词资产有金样，按既有流程（`LEARNING_UPDATE_GOLDEN=1`）重生成并更新条款覆盖断言。
- [ ] `gd.no_schedule` 扫描豁免 `goal` 字段（目标原句回执），其余字段照扫。
- [ ] 新测：目标含「进度」的结构产物 goal 回执不再触发 no_schedule；其它字段含排期词仍报。

## 三件套
- **主指标**：两处不一致消除（单测钉住）。
- **护栏**：structure 臂真机 EXIT=0；no_schedule 对 goal 以外字段的检出不变。
- **推翻条件**：无（机械）。
