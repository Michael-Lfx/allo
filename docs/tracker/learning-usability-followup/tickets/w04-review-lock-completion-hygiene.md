---
id: w04
title: "评审翻案条款补全 + 卫生批（开关注释同步、app 测试 serial、回合同干题面池）"
type: task
status: open
lane: growth+hygiene
blocked_by: []
part_of: ../map.md
---

# w04: 翻案锁补上另一半，文档不再说谎

## 问题与证据（第二轮审计·教练路 F5 + 出题路/多路，证实）
- 翻案锁只禁「按前轮意见做的修改」再判——「某词 轮1判常识→轮2要求挂号」的改判仍合法（run-1793951137445 病历形态今天可复现）。
- 六开关缺省翻 on 后，`switches.rs` 的 ENV/函数 doc 与多条测试失败消息仍写「缺省 off」（`quiz.rs:46` 同）——文档与行为相反（多路复审同报）。
- `nomifun-app/src/app_server_catalog.rs` `models_list_falls_back_to_db_default_when_config_has_none` 写 env 竞态（本轮合并验证实测红一次、隔离绿）——缺 `#[serial]`。
- 回合同干变式：on 臂 Node0/Node5 变式题干逐字相同——防复用池仅节点内，回合级无门。

## 实施要点
- [ ] （ANTI_LOOP on 臂）评审 user 的翻案锁条款补全：「对同一词源的前轮判定（如已判常识/已判缺失），无新证据不得改判；认为前轮判定有误时写进 findings 说明，不得以改判方式打回」。
- [ ] （卫生）`switches.rs` 六开关 doc/测试消息、`quiz.rs:46` 注释：缺省表述统一为「缺省 on（2026-11-07 真机 A/B 达标）」；测试失败消息同步。
- [ ] （卫生）app `models_list...` 测试加 `#[serial_test::serial]`（nomifun-app dev-deps 已有 serial_test 则直接用）。
- [ ] （变式）回合同干池：变式装配时把**本回合已落盘题面**（题库题 + 已生成变式）以「避免与以下题面重复」清单附给模型；不加硬门（重复仍可过但留读数 `variant_same_stem_hits`）。
- [ ] 新测：翻案锁条款两臂装配；serial 标注（可从测试属性断言或直接人工核）；同干池装配与读数。

## 三件套
- **主指标**：翻案改判次数（真机 grow on 臂档案对照）；回合同干题面命中读数。
- **护栏**：评审正常打回能力不削弱（条款只限「无新证据的改判」）；quiz 判据全过；app 套件全绿。
- **推翻条件**：on 臂出现「真缺陷被 findings 化、该打回未打回」→ 条款措辞回退为仅禁止出处改判。
