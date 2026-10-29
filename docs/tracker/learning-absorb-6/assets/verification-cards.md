# 六轮吸收 n1–n5 双臂真机对照卡（2026-10-29）

> 验证对象 = **feat/learn0928 合并后尖端**（波次经 e08e65d60 并入 + cherry-pick
> tracker + merge follow-ups；真机四臂跑在 c67c991dd..8335d719c 区间的二进制上，
> mtime 2026-10-29 14:28 起逐次重建已核）。凭据 = Flowy Cloud flash（10-29 重登
> 后新库搬 provider：providers 1 + models 17，密文与 Nomi-dev encryption_key
> 成对）。运行根 = `C:/Users/test/Desktop/work/absorb6-verify/`（各臂独立
> data/ + 共享 `db/absorb6-base.db` 副本）；观测存档各臂
> `data/observation/learning-smoke/run-*/calls.jsonl`。

## 真机前的冒烟修复（真机阶段暴露，四处，全在 learning_smoke.rs）

三处「臂盲判据」+ 一处布景 bug——实现者只跑了单测（评审执行器/判据逻辑），
smoke 布景与判据的开关臂语义直到真机首跑才被 exercise：

1. `1ae4d4fb7` 判据②预算条款按臂取词（off 臂 = 旧 #154 条款「明显超容才」）。
2. `cf57de510` 判据③节点 est 断言仅 on 臂（off 臂回落 10、模型复议，改健全
   区间 1-240；实测两轮 12/15 印证模型复议行为）。
3. `02e502309` grade 布景两条 activity 同插 position 0 撞 UNIQUE
   (lesson_id, position)——n5 布景 bug，潜伏至首次真机。
4. `8335d719c` grade 判据②缺失要点断言仅 judge 臂（off 臂旧 {score,feedback}
   契约结构性无此字段，改断判错+低分）。

## n3 内容边界——content 双臂（开关臂 NOMI_LEARNING_EST_BUDGET）

| 读数 | on（新语义） | off（旧 #154 条款） |
| --- | --- | --- |
| EXIT | **0（判据全过）** | **0（判据全过）** |
| 课时读数 | sections=4 activities=5 body_chars=1528 **est=20** | sections=4 activities=5 body_chars=3054 est=15（模型复议） |
| 预算行注入 budget_line_calls | **1** | **0** |
| capacity_note 事件 | 0（预算内收敛，缺席合法） | 0 |
| **边界三行 known/tier/precedent_line_calls** | **1/1/1** | **1/1/1（不挂 est 臂，两臂同注入）** |
| R10-R13 条款读数 | 全 1 | 全 1 |
| 超纲专名抽查（软读数） | 命中 0 | 命中 0 |
| faces 概念面 | beta_canonical+alias 命中 | 同左 |

结论：#177 输入面（已知名单行）、#178 档位行、#180 先例行在真管线各命中一次；
est 铸值 on 臂精确锚节点 est、off 臂如实回落模型复议；超纲专名两臂零命中。
判据①②③⑤ hard 全过。

## n5 开放题判分——grade 双臂（开关臂 NOMI_LEARNING_OPEN_JUDGE）

| 读数 | on（judge 评审链） | off（旧 grade 链） |
| --- | --- | --- |
| EXIT | **0（四判据+脱敏全过）** | **0（四判据+脱敏全过）** |
| ①病历作答（语义覆盖未按句式） | **判对 0.88 / 缺失 0** | 判对 0.75 / 缺失 0（结构无字段） |
| ①reflection 病历 | 1.00 | 1.00 |
| ②真误区 | **判错 0.10 + 缺失要点 5 条**（点评点名「与复利思维核心结论相反」） | 判错 0.20（无缺失字段） |
| ②reflection 空泛 | 判错 0.00 + 缺失 3 条 | 判错 0.10 |
| ③离题 | 判错 0.00 不崩 | 判错 0.40 不崩 |
| ④断 LLM | JudgeExhausted 如实上抛 | 如实上抛 |
| 脱敏回读 | 观测面零判底零作答原文 | 同左 |

**on 臂 ④ 的 WARN 链路是修复批降级收窄的活证据**：thinking 通道 Conflict
「structured output unavailable」（合法降级信号）→ 降级普通 json 通道 →
同 Conflict → 重试环 3 次带原因回灌 → JudgeExhausted fail-loud。超时/BadGateway
不降级的收窄语义由单测三件套钉住（timeout/transient/sticky）。

**双臂对照结论**：方向判定两臂一致（判对/判错/离题全同向）；增量 = on 臂的
missing_points 可行动反馈（真误区点名 5 条缺失 vs off 臂 0 条）与更严的离题
惩罚（0.00 vs 0.40，仍判错）。explanation 判底真机有效（病历作答未按题面句式
仍判对——ADR-0037 语义覆盖口径落地）。

## n1 题面渲染——单测面证据 + 走查留观

- 组件/页面测试 19 个（KaTeX 无裸 `$` / p 解包 / 坏公式 fail-visible / `___`
  三形态 / 裸 HTML 转义 / `\(...\)` / 选项公式渲染 / 双语兜底）全绿；修复批后
  learning+Markdown 分片 109 pass。
- **两主题手工走查未做**（真机阶段跑 smoke 二进制，未起 dev 栈）——如实留观：
  下次 dev 会话打开任一含公式课时即可目验（组件断言已覆盖渲染正确性，
  走查只余视觉层）。

## 环境事实（复现备注）

- 基库 = learn0928 88 链新播种（`learning-smoke xp` 零 LLM 迁移）+ 自
  `AppData/Local/Flowy/Nomi-dev/flowy-backend.db` 搬 provider 四表；
  各臂 `data/encryption_key` = Nomi-dev 同款（密文同源可解）。
- 一臂一次瞬断（connection retry attempt=1）被 provider 重试纪律吸收，未影响
  判据——真机重试面顺带实证。
