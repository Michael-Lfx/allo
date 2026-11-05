---
id: p03
title: "smoke 读数与教练知情——overshoot scope 感知豁免、候选集空省略、四源知情条款"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# p03: 小项打包——读数不再误报、渲染不再空转、教练不再盲写

**What to build:** 三件独立小项：①smoke overshoot 超纲专名读数学会 scope
感知豁免（吸收 absorb-8 map 雾区的后续小票候选）；②提名候选集空块省略
渲染；③教练声明面纪律加一行四源知情条款。

## 现状（2026-11-05 实码核实，基线 a672c7eb4）

- **overshoot**：`nomifun-app/src/commands/learning_smoke.rs` ~`:2985-3000`
  ——四词硬扫（布朗运动/伊藤引理/GARCH/维纳过程）对 `view.sections` 的
  body_md 拼接文本计数，无否定豁免。真机病历（run-1793829277050）：
  scope 声明「不引入 GARCH 模型」，一过正文按声明优先条款写了诚实边界句
  （或真超纲——不可追溯），overshoot hit=1 与 scope_findings 0 并存；
  一旦某课程 scope 提到这四词之一，读数恒红失去信号价值。
- **候选集空渲染**：`service/learning_graph.rs` `render_nomination_candidates`
  （~`:2625`）——首批（登记表空）每条提名渲出「提名「X」的候选：」空块
  （真机 review user L146-156 五连空块）；z02 后 `concept_review` 签名带
  candidates_text（j04 机制），空块无信息量只占 token 与注意力。
- **教练盲写**：`learning_graph/mod.rs` 声明面纪律（~`:36` / ~`:488` 区）
  教 intent/scope 写法，但未告知这两个字段的词面会进评审机械点名四源
  （z02）——真机 intent/scope 源贡献 80/127 碎片，教练对点名负担零知情
  （不致错，但知情后可引导「照实写、不必堆同义边界词」的写法预期）。

## 实施要点

### A. overshoot scope 感知豁免（learning_smoke.rs）

- [ ] 读数扫描源增加豁免：命中词若出现在**本节点 scope/intent 声明原文**
  （fixture 播种的声明行——user prompt 的「教学范围：/创建意图：」行内）
  则不计 hit（与 scope_findings 的 MENTION_NEGATIONS 同律：声明过的词
  正文提及属边界诚实，不是超纲）。
- [ ] 读数行措辞更新：`overshoot read-out: ... hit={n} (scope-aware;
  human card)`——注明豁免语义；判据面非门禁不变。
- [ ] 单测/真机判据：播种 scope 含 GARCH + 正文含「不引入 GARCH」→
  hit=0；正文无豁免地出现「布朗运动」→ hit=1（两臂形状断言，纯函数
  化以便离线测）。

### B. 候选集空渲染省略（service/learning_graph.rs）

- [ ] `render_nomination_candidates`：候选为空的提名不渲染该条
  （或整块无任何候选时输出一行「（登记表为空——首批无候选，全部按新铸
  处置）」）；`concept_review` 提示词侧【提名候选集】条款补半句「候选
  缺席 = 登记表无近邻，直接裁新铸」。
- [ ] 真机对照：首批 grow 臂候选段不再出现空块；复用裁决行为零变化
  （空候选→新铸是既有缺省语义，纯渲染收敛）。

### C. 教练四源知情条款（learning_graph/mod.rs 声明面纪律区）

- [ ] 声明面条款（~`:36`）追加一句：「intent/scope 的词面与节点标题一并
  进评审机械点名（四源扫描）——照实写即可，评审按覆盖/边界处置；不必为
  避免点名而回避具体名词，也不要堆砌同义边界表述」。
- [ ] substring 断言更新（条款覆盖测试）。

## Non-goals

- 不动 scope_findings 的 MENTION_NEGATIONS（本票只修 smoke 读数面）。
- 不动候选集机械装配面（j04 机制零改）。
- 不做内容臂 faces/前置段（p02 领域）。

## 验收

1. `cargo test -p nomifun-app -p nomifun-learning` 全绿 + 新测
   （overshoot 两臂形状、候选渲染空省略、条款断言）。
2. 真机 content 臂 EXIT=0（overshoot 读数新措辞 + 判据不回归）；
   真机 grow 臂候选段无空块（首批）。
3. 复用裁决零回归：非首批场景（有登记表）候选集照常渲染（真机 grow
   第二批若触发，读数可见）。
