---
id: m5
title: "生长收敛梯子对账加固——有状态复审 + #175 三断层（#128/#129 残差）"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# m5: 生长收敛梯子对账加固

**What to build:** 对账 learnhub #128/#129（ADR-0026）+ #175（三断层修复）与
allo 生长重试回路的差异，把真缺口补上：有状态复审（前轮评审意见随重试注入 +
「已修不重提、已过不翻案」反乒乓纪律）、advice 范围条款进模型面、点名守卫
全量枚举、polish 穷尽/守卫违规时评审意见回灌重排。**开票第一步 = 逐项对账**
（下表已列 allo 现状锚点，实施时先核实再动手）。

## 对账表（2026-10-26 深挖结论，实施时逐行核实）

| learnhub 机制 | allo 现状 | 裁决 |
|---|---|---|
| 铸名轮级暂记回滚（ADR-0026） | **不需要**——allo 评审先行于 apply_batch（growth_body :1783 → :2107），门失败 = 零 DB 写；apply 单事务 :2723-2932 | 结构性豁免，Resolution 记录 |
| 别名注册不回滚 | allo 同构（register_alias_tx 在 apply 内，门失败不触达） | 无缺口 |
| polish ≤2（点名点修，未点名逐字节不变） | 有等价物：原地重试恰一次不占预算（service :1608-1646）+ `redraft_guard_violation`（growth_retry.rs:74-97） | 核实守卫语义后决定是否收敛措辞 |
| 有状态复审（`{{prior_review}}` 槽 + 已修不重提/已过不翻案） | **无**——重试反馈带条款报告（j03）但无前轮评审记录注入，反乒乓纪律缺失 | **吸收** |
| advice 范围条款进模型面（#175A：node 填显示名才构成点名/「批」级意见不点名节点/advice 提到的名字不构成点名） | 无模型面条款（守卫是机械的 `ng.redraft.scope` :99-107，但评审提示词无 advice 范围措辞） | **吸收** |
| 守卫全量枚举（#175B：一次收集全部违规，非首错即停） | 待核实——`redraft_guard_violation` :74-97 实现细节（首错早退 or 收集全部） | 核实后补齐 |
| 评审意见回灌重排（#175C：polish 穷尽/守卫违规/契约命中都带问题清单进重排） | 部分有——重试 ≤3 带反馈；但「前轮语义评审意见」作为独立注入段（非合并进同指纹原因）待核实 | 核实后补齐 |
| 机械命名碎片合成问题（#175 Q5：未处理碎片按 teaches 归属合成问题，回退「批」） | 无 | 随 m4 盘问（依赖 naming list 形状），本票只留接缝注记 |
| 「二拒放行」废止 | allo 无此规则（早停 = 同指纹连败，语义不同） | 无缺口 |

## allo 现状锚点

- 重试回路：`growth_retry.rs`（`GROWTH_MAX_ATTEMPTS=3` service:40、
  `should_stop_early` :52-57、`redraft_guard_violation` :74-97、
  `REDRAFT_CAPACITY_CODE` :110、`normalize_report` :41-51）；原地重试
  service:1608-1646；重排提示词 `coach_redraft`（mod.rs ~:590s）。
- 评审调用：growth_body :1783（concept_review）；评审系统提示词
  CONCEPT_REVIEW_SYSTEM :704+（k07 后含 V2 子查 + 开关）。

## learnhub 参考（714923b + 7d856f6；锚点为 learnhub 仓现行树）

- `node_growth/facade.rs` attempt doc :379-396（轮级暂记语义 + 失败不耗批号）；
  `reviewer.rs` ReviewInput.prior_review :186-188；polish 轮 2 起注入前轮点名
  记录。
- #175A 模型面条款：`node-growth-review.md`「advice 范围(点修守卫对齐)：修法
  需要改动某个节点的任何字段 = 该条问题的 node 必须填那个节点的显示名…node
  填『批』的问题只许要求改动 new_concepts/new_aliases 提名自身…advice 文字里
  提到的节点名不构成点名」；`node-growth-polish.md`「点名集 = 评审问题清单
  各条的 node 字段（填『批』的批级意见不点名任何节点）」。
- #175B：`assert_unnamed_unchanged` 全量收集单报告（reviewer.rs/facade.rs
  :216-230，测试 `unnamed_guard_collects_all_violations` :292+）。
- #175C：`AttemptFail.review_history` 独立字段（非并入同指纹原因）→
  `into_feedback` 注入「前轮语义评审意见」（facade.rs:136-144，测试 :331-382）。
- 反乒乓纪律句：按 advice 修的项不再重提、已过项不翻案。

## 实施要点

- [ ] **对账核实**（开工第一步）：逐行跑上表，把「待核实」三行落到Resolution
      （核实结果 + 实际 diff 或「已等价」结论）。
- [ ] 有状态复审：评审往返记录（点名集合 + 结论）随 attempt 传递；重排/点修
      提示词增前轮意见段 + 反乒乓纪律句；注入走既有反馈通道（不新开 LLM 面）。
- [ ] advice 范围条款进 CONCEPT_REVIEW_SYSTEM（或 redraft 提示词）+ 点名集
      定义句；条款覆盖短语断言随批。
- [ ] 守卫全量枚举（若核实为首错早退）：一次收集全部违规单报告，原地重试
      带全量清单。
- [ ] 评审意见回灌：AttemptFail 等价物（allo 的失败报告）增独立 review_history
      面，polish 穷尽/守卫违规/输出违约三路都注入。
- [ ] 开关 `NOMI_LEARNING_REVIEW_STATE=off`（有状态复审与前轮注入退 off 臂 =
      现行为）；机械守卫面（全量枚举）不建议开关（纯确定性改进）。

## 验收

- **单测（红绿）**：守卫全量枚举（两处违规一次报全——learnhub 同名测试移植）；
  前轮意见注入（重排提示词含前轮点名 + 反乒乓句，off 臂不含）；advice 范围
  （「批」级意见不构成点名——守卫不误伤）；三路回灌（polish 穷尽/守卫违规/
  契约命中各带清单）。
- **smoke grow 档双臂（开关臂）**：硬判据——两臂 EXIT 既有判据不劣化；on 臂
  观测存档里重试调用提示词含「前轮评审意见」段（机械 grep）。
- **对照卡**：两臂同 fixture 真机各跑（若 fixture 能自然触发一次重试最有说服
  力；触发不了如实记录 + 用 scripted 夹具补机械面证据）——读数：attempts_used /
  同指纹早停次数 / 重试后通过率并排；人读抽判：重试后是否重复前轮已修问题
  （反乒乓实效）。两臂都跑，落 Resolution。
- `cargo test -p nomifun-learning` 全绿。

## 非目标

- 铸名暂记/回滚移植（结构性豁免，见对账表）。
- #175 Q5 碎片合成（m4 形状定后另议）。
- 回放器/行为回归工具（41c9c44，C 组回放套件，不吸收）。

## 备注

learnhub #175 的三条断层全是**真机两连越权/盲试**实证出来的——对账表的
「待核实」行如果核实为已等价，Resolution 如实记录即可，不为凑数造工作。

## Resolution（2026-10-27 关票，implement-spec 波收口）

- **对账表九行全部裁决**（三行「待核实」核实结果：守卫确为首错早退→已补全量枚举；意见回灌确为部分有→已补三路；前轮注入确缺→已补有状态复审；其余六行已等价/结构性豁免维持）。
- **交付**（learn-v5/m5-convergence-ladder，5dda65516 + f96437f3f）：`AttemptFail.review_history` 独立通道（不并入指纹原因，指纹免漂移）+ `prior_review_section` 注入段 + 反乒乓纪律**单源** `ANTI_PING_PONG_DISCIPLINE`（code-review 修复：双写已漂移 → mod.rs 定义、两处消费，头句按面保留）+ `ADVICE_SCOPE_CLAUSE` 两臂同织 + `review_point_names` 机械面 + 守卫全量枚举（按上一稿节点序单报告）+ 三路回灌（polish 穷尽/守卫违规/输出违约）+ NOMI_LEARNING_REVIEW_STATE 开关 + smoke `[review_state: …]`。
- **双臂对照（开关臂）**：assets/m5-convergence-ladder-hardening-comparison-card.md——两臂读数行在场、自然重试 1/1 重试后通过、早停 0；**如实缺口**：injections=0（跨轮注入面未自然触发——三路注入由单测覆盖，机械面证据足）。
