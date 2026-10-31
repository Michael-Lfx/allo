---
id: w1
title: "verify_math 断言核验接口 v0——例题可算量经确定性工具实算"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# w1: verify_math 断言核验接口 v0

**What to build:** 课时内容引擎新增 `verify_math` 工具 op：模型把正文数学翻译成
结构化 claim（eval / identity），确定性核验器执行——eval = 表达式求值，identity =
等式两边随机采样 5–10 点数值比较（相对容差）。R14 写作条款随批（凡可算量必须经
工具实算并展示可复核中间步骤）。smoke content 种子预埋已知收益序列做 oracle 对账。

## 病例（2026-10-30 真机，quality-fixes-verify/content-review/a3-nodeB.md）

- 例题对照列偏离平方和写 ≈0.7（实算 0.16+0.16+0.09+0.09 = **0.50**），后续
  0.7/3≈0.23、√0.23≈0.5% **与错根内部一致**——单步验算全对，只有从原始四数
  重算求和才翻得出；精确列（9+1+4+16=30）全对。
- 根因 = 模型跳过偏差行（从均值直接蹦到和，恰好跳过唯一能暴露错的一步）+ 小数
  平方和是 token 级算术最不稳形态 + 「≈0.7 远小于 30」的粗合理性检查通过即无人
  报警。教学伤害 = 课文自许「可核查」，学习者复核失败会归因自己。

## 设计原则（用户 2026-10-31 拍板，全文见记忆 learning-verification-roadmap）

- **断言接口优先于解算器**：模型负责把正文数学翻译成 claim JSON，核验器是纯
  确定性代码——接口今天定，解算器明天换（v3 若采样真机证明不够，再议 sympy
  sidecar；带 python 运行时成本，决策门非默认）。
- **采样优先于 CAS**：identity = 两边采样 5–10 点数值比较；模型生成的数学错误
  非对抗构造，采样必中（0.7 vs 0.5 类）。
- **失败语义沿既有形状**：核验失败 → 死因+正确值建议回灌打回自修改（条款反馈环
  现成）；预算尽 → audit findings（math_verify warning）留痕，不拦课——挂号表/
  viz_block_lint 同款，零新门语义。

## allo 现状（落点提示，实现会话复核）

- 工具 op 白名单：`nomifun-ai-agent/src/lesson_content_loop.rs`（ls_* 系列，
  `lesson_loop_exposes_exactly_the_whitelist` 测试钉住白名单——加 op 要随批）。
- 修复对照行：REPAIR_LESSON_AGENT_SYSTEM（153–161 行，新 kind 补一行对照）。
- 审计 findings：`nomifun-learning/src/lesson_draft.rs` audit_findings（kind 裸
  String 零注册成本）；smoke content 读数行在 `learning_smoke.rs` run_content。
- 提示词资产：内容引擎 system 常量（GENERATE_LESSON_AGENT_SYSTEM 等）+ R 系
  写作纪律（R10–R13 在案，R14 顺位）；改 system 常量无金样牵动（内容引擎无金样，
  与结构提示词资产不同——实现会话复核）。
- 求值器落点：`nomifun-learning`（内容管线侧，与 lesson_draft 同 crate）或
  ai-agent——**定死：放 nomifun-learning**（R14 条款与 smoke 在该 crate，w2 的
  quiz 线 settle 也在该 crate，未来复用最近）。

## 实施要点

- [ ] 求值器：手写 Pratt 解析器（+ − * / ^ 括号、sqrt/sin/cos/tan/ln/log/exp/
      abs、pi/e），纯 Rust 零外部依赖；相对容差比较；红绿样含 a3 病例表达式
      （`(1/(4-1))*((3.0-0.1)^2+...)` 与平方和 0.5）。
- [ ] claim schema：`{ kind: "eval"|"identity", expr|lhs+rhs, variables?,
      tolerance? }`；identity = 采样 5–10 点（奇点剔除），相对容差缺省 1e-9；
      坏 JSON/未支持 kind 如实报错不静默。
- [ ] 工具 op `verify_math` 进白名单；核验失败返回死因+对照值（模型自修改输入）；
      调用计数进 publish 帧（smoke 读数数据源，viz_lint_events 同款接线）。
- [ ] R14 条款：凡正文可算量（例题/对照列/单位换算/计数/日期差）必须经
      verify_math 实算并在文中展示可复核中间步骤；对照列与精确列**同等展开**
      （偏差行逐个写，禁从均值跳到和）；「≈」只许用在最后开方/极限步，可直算的
      中间值一律写实算值。
- [ ] smoke oracle：种子课程布景预埋已知收益序列（期望 σ 事先算死），判据 =
      生成正文例题数字与预埋值吻合 + calls.jsonl 观测 verify_math 调用在场；
      读数行 `math_verify read-out: calls=p passed=q failed=r`（非门禁）。
- [ ] 红绿样：求值器全运算符表/优先级/一元负号；claim 解析边界；identity 采样
      命中合成差错（如 x² vs (x+1)²）；R14 条款覆盖断言随批；工具失败→打回→
      自修改→过闸的循环测试。

## 验收

- `cargo test -p nomifun-learning` + `-p nomifun-ai-agent` 全绿；fmt；零迁移。
- 真机判据：smoke content 种子 oracle 对账吻合、math_verify 读数在场；含数值
  例题的生成至少一次真实打回自修改成功（或如实记录零打回——种子难度低时可能
  一次过，如实报）。

## 非目标

- 符号引擎/CAS（v3 决策门）；equation/inequality/导数/积分/ODE kind（w3）；
- 练习题型接核验器（w2）；网络质证与 KB 溯源（map 雾区已裁决）；
- jsxgraph/几何图形核验。
