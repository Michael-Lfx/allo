---
id: u10
title: "出题链补洞：变式判底正文/缺席跳过 + 证据逐字核 + 按题型装配契约 + tol 透传 + 指令独立段 + 语言跟随"
type: task
status: closed
lane: D
blocked_by: []
part_of: ../map.md
---

# u10：变式题不再凭空造证据

## 问题与证据（审计⑤，读码证实；变式 24 次真机调用全为无凭据失败，无成功样本）
- 判底缺失：`service/quiz.rs` ~:867「节点正文可能未生成；excerpt 留空」，而系统提示词（`generation/mod.rs` ~:145）声称「You are given the finished lesson document, its cited excerpt…stay grounded」；单题校验（`generation/activities.rs` ~:414）不查 evidence（硬门 ~:264 只走整课链）→ 正文缺席时模型只能造「逐字证据」，随判分下发学习者。
- 单题提示词只写 single_choice/true_false/fill_in_blank/reflection 四型形状（activities.rs ~:157-161），numeric/multi_choice/ordering 也走它，数值题连「answer 是 JSON number、tol」都没写 → 高概率违约，两试不过静默跳过。
- `quiz.rs` ~:880 `tol: None`（ActivityPack.tol 已解析），判分回落 1e-9 → 舍入答案几乎必判错。
- 变式硬性要求被包装成「Focus hint from the learner」（quiz.rs ~:854、activities.rs ~:377）→ 被降权为偏好。
- 出题提示词无「语言跟随正文」条款（判分侧有，open_judge.rs ~:140）。

## 实施要点
- [ ] （机械）`tol: None` → 透传 pack.tol。
- [ ] （开关 on 臂）变式判底：有已生成课时正文时把正文作为 document 装配；**正文缺席则跳过变式**（如实，不造证据），读数记 `variants_skipped_no_document`。
- [ ] （开关 on 臂）单题校验在 document 在场时核 evidence 逐字出现在正文（不在 → 违约走既有重试）。
- [ ] （开关 on 臂）单题提示词按请求题型装配对应形状条款（与整课题库提示词的九题型契约同源，numeric 含 answer 为 JSON number + tol）。
- [ ] （开关 on 臂）变式的「同考点/换皮/可判分」要求放进独立的指令段，不再冒充学习者提示。
- [ ] （机械）出题提示词补「题目语言与课时正文一致」。
- [ ] 开关 `NOMI_LEARNING_VARIANT_V2`（缺省 on 待 A/B），off 臂逐字节回归。

## 三件套
- **主指标**：变式生成成功率（过契约/总请求）、违约重试次数、证据逐字命中率；数值题 tol 生效（单测）。基线：**无真机样本——先以 off 臂补跑 quiz 臂取基线**。
- **护栏**：quiz 臂判据全过；整课题库生成零变化（契约同源但只改单题装配）；跳过变式不影响 quiz 回合进行（题库题照出）。
- **推翻条件**：on 臂变式成功率低于 off 臂 → 默认 off（tol 与语言条款不受影响）。
