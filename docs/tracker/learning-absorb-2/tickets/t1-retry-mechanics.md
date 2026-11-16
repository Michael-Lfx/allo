---
id: t1
title: "重试循环机制定案：早停、guard 豁免、容量守卫、原地重试"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: map.md
---

## Question

learnhub 两件 P0 机制（#60 同指纹早停 + #85 容量守卫/点修原地重试）平迁到 allo 的设计
事实定案——决策面已清楚（就是吸收），本票把落点/口径/边界定成施工事实：

1. **同指纹早停**（learnhub 口径：`prev_fingerprint = fail.reason` 逐字；同指纹且
   `!guard` → 立即 Exhausted 不烧预算；口径「反馈回灌已无法改变结果」）。落点两处：
   - `structure_gen.rs` 重试循环（MAX_ATTEMPTS=3，任一门红两份一起重生成）——指纹取什么
     （逐条款报告全文？条款编号集合？learnhub 用 reason 逐字，allo 报告是结构化清单，
     指纹 = 规范化后的报告字符串？）；
   - `learning_graph.rs::growth_body` 3 轮循环——指纹源（形状报告 / 门报告 / 评审报告）。
   allo 现在失败原因是否已带稳定文本（决定指纹可用性，实施时核）。
2. **guard 豁免判别**：allo 现有失败类型里哪些是「机械固定文案」（同指纹≠同输出前提不
   成立，豁免早停）、哪些是「携带模型输出的原因」（早停适用）。对照 allo：红raft/评审
   打回都携带上一稿细节——allo 现循环没有「未点名节点被改动」类机械比对守卫（无 guard
   面），**该守卫本身是否顺带引入**（learnhub 116e805 的一部分：点修输出与上一稿机械
   diff，未点名节点被改 = guard 失败）——推荐引入（它与「点修不是重写」提示词纪律配对，
   allo 已有后者缺前者）。
3. **容量守卫**：allo 批上限 `MAX_BATCH_NODES`=7（learnhub 5）；拆步/点修后超容从「整批
   违规」改判「定向重装指令」（反馈携带拆分后形态 + 明示「超出留后续批，不要放弃已正确的
   拆分」）。落点：growth_body 重裁出口 / 结构门 `ng.batch.size` 前置检查（在门报告里带
   定向措辞，还是循环层独立守卫？）。
4. **点修越权原地重试恰一次**：guard 失败（未点名被改）时带守卫原因再裁一次、保留评审
   上下文；第二次再越权才计轮。allo 循环语义（attempt 计数）如何融入——重试恰一次不占
   3 轮预算之一，还是占？
5. **提示词配对**：批容量红线（自检类；allo 无独立自检轮——红线进 COACH_SYSTEM 还是
   重裁反馈模板——与 g4 文本面交叉，本票只定机制侧落点）；「点修不是重装」负例已有一轮
   redraft 纪律，核对是否需补「修法需要动未点名节点才能成立时放弃该修法」句。
6. **验收**：以 learnhub 测试口径为底——同错连败两次即停（调用数 2 非 3）+ 错不同烧满
   预算 + 超容定向打回 + 越权原地重试落位；冒烟面（learning-smoke grow/loop 已有批管线
   判据）是否需要补早停观测读数。

产出：以上六项的定案（每个落点的文件/函数/口径 + 测试口径清单），供实施票照单施工；
实施随二轮整体切票进行。参考实现：learnhub `src-tauri/src/node_growth/facade.rs`
（`assert_round_capacity`、`pass_naming_gate` 循环、`fail.guard` 字段）与其测试。

## Resolution

**速览**：六项定案——指纹 = 失败报告的规范化 JSON；早停两处循环同款（连败两次即停）；
**顺带引入「未点名被改」机械守卫**（guard 豁免与其配对）；容量守卫在重裁出口、专用条款码；
原地重试恰一次不占轮预算；smoke 增读数。

1. **指纹与早停**：`fingerprint = serde_json::to_string(&failure_reports)`（报告结构体
   序列化，字段序稳定即确定性；learnhub 用 reason 逐字，allo 报告是结构化清单，规范化
   JSON 同义）。两处循环同款：
   - `service/learning_graph.rs::growth_body`（:504-648）——指纹源 = 该轮失败面（形状
     报告 / `validate_batch` 错误 / 评审 problems）之一，序列化后比对；
   - `learning_graph/structure_gen.rs`（MAX_ATTEMPTS=3 循环）——指纹源 = gd.* 双产物
     合并条款报告。
   条件：`attempt > 1 && fingerprint == prev && !guard` → 立即 `Err(exhausted)`，不烧剩余
   轮次（learnhub 口径「反馈回灌已无法改变结果」；连败两次即停，调用数 2 非 3）。
2. **guard 豁免 + 顺带引入「未点名被改」守卫**：allo 现有失败面全为生成面（携带输出细节，
   早停适用）；learnhub 的 guard 类失败（机械固定文案）在 allo 本不存在，**与「点修不是
   重写」提示词纪律配对的守卫一并引入**：重裁输出与上一稿机械 diff——未出现在门报告/
   评审 problems 点名清单中的节点必须逐字段相等（title/purpose/est/difficulty/teaches/
   assumes），违者 = `AttemptFail{guard: true, reason: 固定文案}`。guard 失败豁免早停
   （同指纹≠同输出前提不成立）。
3. **容量守卫**：重裁出口前置检查 `nodes.len() > MAX_BATCH_NODES(7)` → 专用条款码
   `ng.batch.redraft_capacity`，消息携带定向指令：「按拆分后的形态重新装箱，每批至多
   7 条——超出容量的条目留待后续批次，不要放弃已正确的拆分」（learnhub
   `assert_round_capacity` 同构；guard=false，可早停）。`ng.batch.size` 形状条款保持
   起草轮执法单源不变。提示词配对红线（「修订不得超批容量，装不下宁可少拆」）随 g4
   波次进 COACH_SYSTEM 自查条。
4. **点修越权原地重试恰一次**：guard=true 失败时，带守卫原因在同一 attempt 内再裁一次
   （**不占 3 轮预算**，保留评审上下文）；第二次越权计为该 attempt 失败，走正常轮推进。
   learnhub 取舍照抄：原地重试保上下文 vs 回炉重装丢上下文。
5. **早停适用面**：解析失败 / 形状条款 / 语义门 / 评审 problems / LLM 错误 = 生成面，
   早停适用；guard 类（本票新引入的 diff 守卫）豁免。无第三类。
6. **smoke 读数与测试口径**：learning-smoke grow/loop 输出增 `attempts_used` +
   `stop_reason`（success | early_stop | exhausted）——读数面非门禁。测试（learnhub
   口径为底）：同错连败两次即停（LLM 调用数 2 非 3）；异错烧满预算；重裁超容定向打回；
   未点名被改 → 原地重试一次落位、再次越权计轮；structure_gen 至少一个早停用例。
   落点文件：`service/learning_graph.rs`、`learning_graph/structure_gen.rs`、
   共享小助手（指纹 + diff 守卫，放 `learning_graph/mod.rs` 或新 `growth_retry.rs`）、
   `service/tests.rs` + structure 测试。

