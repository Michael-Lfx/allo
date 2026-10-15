---
id: j03
title: "生长/结构循环行为加固：重试机制 + 真取消"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# j03: 生长/结构循环行为加固（重试机制 + 真取消）

**What to build:** 两类循环行为修复一次到位（同文件合并实施）——①二轮 t1 重试机制
（同错指纹早停 + guard 豁免 + 容量守卫 + 原地重试）；②三轮 t2 长任务注册表与真取消
（修复假取消缺陷）。

**实现决议:** 二轮 t1（全部）+ 三轮 t2（全部）。参考实现：learnhub
`engine/maintenance/mod.rs` RoundRetry、`app/generation.rs`（全套含 mid-call 取消测试）。

**Blocked by:** None（与 j01 并行安全；建议早于 j05/j07——同文件循环改造一次到位）。

**Status:** ready-for-agent

- [ ] **同指纹早停**：指纹 = 失败报告规范化 JSON；`growth_body` 与 `structure_gen` 两
      循环同款——`attempt > 1 && fingerprint == prev && !guard` → 立即 exhausted
      （连败两次即停，调用数 2 非 3）
- [ ] **「未点名被改」机械守卫**：重裁输出 vs 上一稿 diff——未点名节点逐字段相等，违者
      `guard=true` 固定文案打回；guard 失败豁免早停 + **原地重试恰一次（不占 3 轮预算）**
      ，二次越权计轮
- [ ] **容量守卫**：重裁出口 `nodes.len() > 7` → 专用码 `ng.batch.redraft_capacity` +
      定向指令（「按拆分后的形态重新装箱……不要放弃已正确的拆分」）；`ng.batch.size`
      起草轮执法单源不变
- [ ] **长任务注册表**：新模块 `service/generation_tasks.rs`（键 `{kind}:{course_id}:
      {node_id|-}`；状态机 running→done|failed|cancelled 终态幂等、同键并发 begin 拒绝、
      AtomicU64 注册序排序）；快照命令 `GET /generation-tasks` + 统一取消端点（存量
      `/generate/cancel`、grow cancel 改造转发）
- [ ] **CancellationToken 全调用点双检查**：`growth_body` 每轮起草/重裁/评审前后、
      `structure_gen` 两调用前后 + 循环头、快照写前（写前取消 = 零快照）、lesson/outline
      管线各 LLM 调用前后；completer 调用 `select!` 真中断（中断 = 断 HTTP）；批事务
      原子落库 = 中断零落库
- [ ] **中性终态 envelope**：`generate`/`grow` 响应升 `{state, ...}`；取消 = cancelled
      不报错误；生成开始后才取消 = 竞速落败如实 done；WS 对齐（补 structure_cancelled）
- [ ] **进度叙事**：step 机器面/detail 人面单源在后端；前端共享订阅 hook（事件缺席不碍
      流程）；三消费方接线（建课对话框/结构图 Tab/学习页）
- [ ] **观测旁路**：begin/finish JSONL 落观测面（写失败降级）
- [ ] **测试口径**（learnhub 移植）：同错连败两次即停（调用数 2）/ 异错烧满预算 /
      超容定向打回 / 未点名被改原地重试落位、二次越权计轮 / mid-call 取消内层恰被调
      一次且零落库 / 终态幂等 / 同键拒绝 / 注册序排序 / 每类长操作一个取消用例

**Resolution:**（收口时写）
