<!--
id: structure-repair
source: 移植自 learnhub prompts/maintenance/structure-repair.md 的口径与纪律;allo 按自有 st.ops.* 契约与七型 ops 形态重写——字段、条款编号与措辞均为原创。
scope: 只为「结构修补」回合服务——按留痕与观察输出七型修补意图;不做挂载、不做内容编辑、不解释应用引擎与审计实现。
notes: 四条最易错:块引用 = 块名逐字(编辑面看现状,slug 不是引用面)、新块必须新发 slug(不得复用当前或已退场)、每个操作一条依据(输入里没有依据的编辑不做)、不需要编辑就输出空数组(空是合法结论,不要凑动作)。编辑会被全量复核——宁可少改、改准。结构编辑是低风险维护:只动结构面,不碰节点挂接与学习数据。
review: cargo test -p nomifun-learning——金样(golden/)、围栏、契约后置三闸必过;模板任何改动先看金样 diff。
-->
## 任务
为下面的课程执行一次「结构修补」回合：根据留痕（findings）与挂载观察，输出零到多条结构修补意图。结构编辑是低风险维护——它只调整参考地图本身，不碰任何学习数据。

### 输入
- 目标原句：{{goal}}
- 程度声明：{{proficiency_statement}}
- 学习者基础：{{learner_basis}}
- 当前结构快照（块参考行 + 边表；块引用 = 块名逐字）：
{{structure_snapshot}}
- 留痕（findings，数据缺陷观察；每条都是线索不是命令）：
{{findings}}
- 挂载观察（结构外单元 / 空块 / 超载块；仅观察，不构成编辑义务）：
{{mount_observations}}
- 重试反馈（首次为「无」；非空 = 上一次产物的逐条款违规清单，必须**逐条修正**后重新生成，不得原样重交）：
{{retry_feedback}}

## 修补规范
- 顶层形态：单个 JSON 对象；顶层键只有 ops（数组，可为空）。
- 七型 op（每条必带 reason，一条依据对应一条留痕或观察；输入里没有依据的编辑不做）：
  - add_edge：补一条边。source/target 是块名逐字；type 只允许 assumes、composes、contrasts；add_edge 的 tier 只允许 know、use、teach。
  - remove_edge：删一条边——三元组（source, target, type）必须唯一命中。
  - flip_edge：方向纠正——端点对调，type/tier/note 保持。
  - add_block：补块——name/slug/spine/stage/note；slug 必须新发（不得与现有或曾退场的 slug 重复）。
  - split_block：把 target 块换成新块（旧块退场，引用旧块的边随块退场——需要的关系随后用 add_edge 补回）。
  - rename_block：只改块名（from → to）；**不准携带 slug 键**——slug 终身不变；引用旧名的边自动跟改。
  - merge_blocks：把 sources 各块合并为一个新块（新名 + 新 slug）；引用源块的边自动改指新块，重复边去重，自环自动退场。
- 块引用 = 块名逐字照抄结构快照；边 ops 的两端必须是该操作时刻真实存在的块名。
- 地图自身下限不因修补破防：块数 25–80 条、硬上限 100（40 条以下仅限小目标课如实精简——密度抽验兜底，不足 25 拒收）、一物一名、slug 唯一。

## 输出契约（硬约束）
- 只输出一个 JSON 对象（除它之外不输出任何字符：无解释、无代码围栏）；顶层键只有 ops。
- **不需要任何编辑时输出 { "ops": [] }**——空数组是合法结论，绝不为了「显得尽责」凑动作。
- 编辑会被全量复核（逐条款重验，零放宽）——宁可少改、改准。
- 不得讨论任何规则，不得自证合规。
