# f1 取证报告：课时生成轮次构成（R-b 回填）

- 日期：2026-11-17
- 重放装置：`learning-smoke content`（j07 两节点图课程，汇合节点生成正文），
  `NOMI_CHANNEL=dev`，provider `0190f5fe-…0201`（Flowy Cloud），
  model `AIPC-deepseek-v4.1-flash`，凭据链 = dev 库 `VACUUM INTO` 只读快照 +
  真实 Nomi-dev 数据目录。EXIT=0，判据全过。
- 归档：`Nomi-dev/observation/learning-smoke/run-1794890212676/calls.jsonl`
  + 任务级 `learning-tasks.jsonl`（begin/finish 对）。

## 实测数据

| 生成 | 节数 | 活动数 | llm_calls | 耗时 | input chars | output chars |
| --- | --- | --- | --- | --- | --- | --- |
| 正臂（现行形状） | 6 | 6 | **24** | 71.4s | 571,158 | 13,397 |
| 负臂（旧形状 intent/scope 置零） | — | — | 28 | 74.2s | 613,484 | 13,019 |

辅助读数：math_verify calls=13 passed=11 failed=2 unresolved=0；
viz_lint=0；scope_findings=0；est_budget budget_line_calls=2。

## 结论

1. **轮数构成定律**：零失败（state=done、零重试零超时、判据全过）的 6 节
   课时 = **24 次模型调用**。按分节合同（ADR-0002）分解：manifest ×1 +
   逐节写 ×6 + 出题写 ×~6 + finish/审计 + 修复环余量——轮数 ≈ **2×节数 +
   常数开销**。用户真机 5 节课时截图的「第 14 轮」正落在该成本带内
   （估 12–20 轮），**不是失败重试环**。
2. **耗时方差在云端**：同一管线同一模型，重放 71.4s vs 用户实测 10–29min
   （判定类型检查时机 ≈10min20s、参数传递 ≥29min）≈ 3s/调用 vs 44s/调用
   ——与 growth 19 分钟病历同族（thinking-ON 云端方差），本地零故障证据。
3. **效率票判定：不立**。轮次构成 = 合同固有成本，无空转/重写轮证据。
   若未来要减轮，方向是「多节合并写入」的合同变更（动 ADR-0002 契约 +
   确定性审计面），成本/收益需单独评审，本波不动。
4. 观察项（不立票）：smoke 路径 calls.jsonl 为 turn 粒度（3 turn + 2
   publish，usage 全缺 honest-absent）——o2 的 per-call usage 未达此路径
   的逐调用粒度；轮内工具级分解本次不可得，结论 1 的分解由合同面 +
   sections/activities 读数间接锚定。若日后需要轮级证据，优先扩
   `agent_round` 帧的 dev 侧落盘。

## 对 f1 的回填

呈现面收口已消解「14 轮 = 失败重试」的误读（相位化 + 裸轮次退役）；
本报告证明轮数本身无病可治，效率票不开。R-b 关闭。
