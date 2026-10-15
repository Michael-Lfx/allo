<!--
id: goal-decomposition
source: 结构改编自 KSB_V4 v3.6(sharptoolbox/visual-model,CC BY 4.0)经 learnhub 课程引擎中转;allo 按自有契约改造——产出形态、字段与纪律均为原创重写。
scope: 只为「知识结构」产物(spines/blocks/edges 快照)生成服务——不做前置地形、不排教学顺序、不解释校验与消费方实现。
notes: 正文措辞与人读契约刻意不同,对齐只锁条款级。四条最易错:块只有五个键(判据形字段一回来就是概念表形态,违规)、assumes 边源=需求方/目标=前置(方向写反全网皆错)、门口编号只能写 [G1] 起的占位并由系统对账、goal_type 与 domain_type 由模型自判(用户不给)。门位口径 v2.10:地板以上全进结构,入门档贴门口起排。存储为 JSON 快照单列,正文只描述形状不描述落盘。开发面事实住本头注。
review: cargo test -p nomifun-learning——金样(golden/)、围栏、契约后置三闸必过;模板任何改动先看金样 diff。
-->
## 任务
为下面的学习目标生成「知识结构」产物：一张课程内部的参考地图，回答「这门课由哪些知识构成」。
输入是产物标量字段的事实来源，不得改写、不得省略。

### 输入
- 目标原句：{{goal}}
- 程度声明：{{proficiency_statement}}
- 重试反馈（首次为「无」；非空 = 上一次产物的逐条款违规清单，必须**逐条修正**后重新生成，不得原样重交）：
{{retry_feedback}}

## 结构规范
- 顶层形态：单个 JSON 对象；顶层键只允许 goal、goal_type、proficiency_statement、domain_type、form_reference、source_tier、generated_by、disclaimer、domain、spines、blocks、edges、gaps、quality。
- 八个标量键全必填：goal（目标原句回执）、goal_type（你在两种目标型里自判：capability = 达成后能独立做成某件事；coverage = 系统覆盖一个领域或科目，只允许填 capability 或 coverage）、proficiency_statement（照抄输入；输入明确要求你扩写时才依据目标扩写成一句可检验的程度声明）、domain_type（你在四种领域型里自判，只允许填 **理论 / 工程 / 历史 / 技能** 四字之一，**不带「型」后缀**；按所选型的维度与关系偏好取舍，见下）、form_reference（固定写 `Flowy 学习结构模板 v1`）、source_tier（名录 或 章节）、generated_by（你的模型名）、disclaimer（一句诚实边界：出处档位与已知局限）。
- 领域适配四型（domain_type 四选一，并按所选型取舍维度与关系偏好）：
  - 理论：概念 × 方法 × 难度；assumes 与演进、类比类说明为主；
  - 工程：理论 × 实践 × 工具；assumes 与组装为主；
  - 历史：时间 × 地域 × 人物；演进与对照为主，弱因果；
  - 技能：环节 × 难度 × 工具；块是操作步骤，assumes 表达环节先后。
- `domain`（领域界定）七个键全必填：core_definition（核心定义一句）、includes（包含，≥3 条）、excludes（不包含，≥3 条）、prerequisite_doors（门口编号列表：门口编号只能写 `[G1]` 形态的编号，首次生成时没有门口清单，写 `[G1]`、`[G2]` 占位并靠系统对账，不得自定义门口判据）、audience（受众一句）、scenarios（典型场景，恰好 2–3 条）、granularity（粒度规划一句）。
- `spines`（主干，顶层）6–12 条：每条 name（主干名，全文件唯一）、definition（一句定义）、key_points（核心要点，≥1 条）、instance（可验证实例：达标该主干时能做成的一件事）。
- `blocks`（知识块总表，底层）40–80 条、硬上限 100：每条只允许 name、slug、spine、stage、note 五个键——definition、operation、example、pitfall、source 等判据形字段一律禁止（块不是概念，不进概念表）；name 是中文块名，slug 是英文标识（小写字母、数字、连字符），spine 必须一字不差地取自 spines 的 name，**不是主干名自己**——给主干写前置边的端点也必须是块名，悬挂边违规；stage 只允许 入门/进阶/高级，note 是一句话主题描述（非判据）。
- 块粒度 = 原子教学主题：一条能独立解释一个现象或完成一个操作；一物一名——名字内禁并列连词（与、和、及）与并列分隔符（顿号、逗号、斜杠）。**数学等领域的惯用复合名也必须改造**：「排列与组合」拆成「排列」「组合」两条，或改用无并列的规范名「排列组合」；「与」只允许出现在「」包裹的官方术语段内（如 `异步语法「async/await」`）；**术语本字含并列字的照官方名整体放行**（如「求和」「和差」「和角积」——其中的「和/与」是术语的一部分，不是捆扎），拿不准就改用无并列的规范名。每条主干下的块 ≥5，不足就在该主干条目上加 `thin_spine: true` 与一句 `reason`（如 `thin_spine: true` + `reason: 该领域上游只分出 4 块`）。
- **入门档排序纪律（先具体后抽象）**：入门档从零基础**可直接上手的具体对象**起步——数的运算、式的化简、图形的识别，能当场做、当场对的内容；抽象语言与记号体系（集合、命题逻辑、函数记号一类课程内部通用语言）**不得占据入门档第一梯队**——它们是工具不是起点，标进阶并排在具体内容之后，随具体内容的需要引入。
- **门位口径（地板以上全进结构）**：门口（prerequisite_doors）画在学习者**真实起步地板**上；地板以上的一切内容都进本结构——入门档从门口贴地起排（开局贴地），块逐格可学、可挂 assumes（可学可挂）；门口与入门档之间不留无主地带——学习者站在地板上就要有第一格可踩。
- `edges`（知识结构网）边表：每条含 source、target、type、tier、note；type 只允许 assumes、composes、contrasts；tier 只允许 know、use、teach；source 与 target 必须一字不差地取自 blocks 的 name，悬挂边违规。方向语义：assumes 的 source = 需求方（要学的一方）、target = 前置（先有的那一方）——「学命题逻辑前要先懂什么是断言」写成 source=命题逻辑、target=断言，不要写反；composes 的 source = 组装体、target = 组件；contrasts 无方向含义。同 source、target、type 的边不得重复。
- `gaps`（盲区声明）≥3 条：每条 item（此处未覆盖或不确定什么）与 direction（补全方向）。
- `quality`（质量清单）≥5 条：每条 check（自检项）、status（met 或 unmet）、reason（unmet 必填理由）。自检项**直接抄用以下五条规范措辞**（自行发挥会误触禁用词）：「主干数在 6–12 档」「块名一物一名」「边端点全部可解析」「无顺序承诺」「盲区已标注」。
- 连边尽量让主干间充分连通（平均度 ≥2、孤块极少），做不到就在 disclaimer 注明。

## 输出契约（硬约束）
- 只输出一个 JSON 对象（除它之外不输出任何字符：无解释、无代码围栏）；顶层键只允许白名单键。
- 输出纪律：不得讨论任何规则，不得自证合规；排期、日程、时间表、进度、计划一类语词在输出任何位置（含 reason、note、注释、自证句）都不得出现——一切层号、阶段、分层都只是结构与难度标注，不是学习顺序；没有排期语义就不写这些词，想表达「没有排期语义」的正确方式是什么都不写，质量清单用 check 与 status 表达合规即可。
- 标量键、domain、spines、blocks、edges、gaps、quality 逐条按结构规范执行；块五键白名单与边表词表违者即废。
- 门口编号不得自定义；块名一物一名；同三元组边不重复。
