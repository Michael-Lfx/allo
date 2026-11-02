# Map: 学习内容核验基建——verify_math 断言核验接口与练习题型升级（w1–w3）

<!-- wayfinder:map -->
<!-- 状态：开图（2026-10-31）。来源 = quality-fixes 波真机产物双轴评审揪出 a3 例题
      对照列算术瑕疵（偏离平方和 ≈0.7 实算 0.50，链内自洽）+ 内容验证四层版图
      设计讨论。设计全文见记忆 learning-verification-roadmap。 -->
<!-- 2026-11-02 复核：w1 已关闭（status: closed，2026-11-01 三臂真机）；learnhub
      在 bd31277..b6a8c4e 新增 21 提交（数学线 13），w2 从「无参考实码」变为
      「有完整判底铸值参考」；新增 h1（求值器预算硬化，P0）并作为 w2 前置。 -->

## 背景与裁决（2026-10-31）

- **病例**：a3 课例题对照列单根数错+链内自洽（0.7 → 0.23 → 0.5% 与错根一致，
  单步验算全对，只有从原始四数重算求和才翻得出）。管线全部现有门（结构/可视化/
  字数/R10–R13/命名/lint）都是**形态级**，没有任何算术检查——课文自许「可核查」
  却无机械背书。
- **用户裁决**：
  1. 计算器/核验工具**确切要做**（动机=练习题型升级的地基：numeric 答案键、
     变式换参、多步题 AI 批改）；
  2. **KB 溯源核验不适用**——生长式课程通常没有足够参考知识库，学习图课也未做
     绑定知识库功能；
  3. **网络交叉质证 = 唯一可行的事实核验路径，暂时不做**（届时取用已备设计结论：
     judge 只配软闸 findings 不硬拦、按课开关、抽样核验报覆盖率、冲突=置信标注
     或「一说 X 另一说 Y」教学法）。
- **核心原则**：断言接口优先于解算器（接口今天定、解算器明天换）；数值采样优先于
  CAS；**确定性 oracle 才配硬闸，judge 家族永不硬闸**。

## Tickets

| id | 票 | 优先 | 依赖 |
| --- | --- | --- | --- |
| w1 | verify_math 断言核验接口 v0（求值器+采样核验+工具 op+R14+smoke oracle+f3 读数修复）——**closed（2026-11-01）** | **高** | — |
| h1 | verify_math 求值器预算硬化（2048 字符/64 层防爆栈）——**2026-11-02 新增** | **高** | — |
| w2 | 练习题型接核验器（numeric 答案键工具铸值/变式换参/修复变式 tol 硬编码）——已收窄；2026-11-02 复核有 learnhub 实码参考 | 中 | w1, h1 |
| w3 | 核验 kind 扩展（导数/积分/极限/ODE 采样核验） | 低 | w1 |

## 裁决记档（2026-10-31 grilling，六问全按推荐）

- **失败语义**：工具层硬反馈（error ToolResult 回模型自愈）+ 发布面仅信息性
  warning（finish 门后追加 `math_verify_failed`），不做 danger；发布面无法重放
  核验是诚实限制，v0 执法面 = 工具反馈 + R14 + smoke oracle。
- **f3 读数缺陷修复归 w1**：viz_lint_events 写端缺字段（观测帧不含、只在 WS 帧），
  真机 `viz_lint_events=0` 系结构性恒 0 非证据——f3 票面已补回写。
- **接线包**：工具两面；R14 进 V2 块双写（EN 传统线无工具只写纪律、中文绑工具——
  非对称记档）；白名单向量随批；REPAIR 表加 math_verify 行+清 stale 行。
- **smoke oracle**：字符串包含期望值 + 调用计数 ≥1 + 读数行。
- **w2 收窄**：numeric 答案键工具算 + 变式换参 + 修复 `tol: None` 硬编码缺陷
  （现状模型口算误差直接判错学习者）；**多步题型+部分分剥离为后置票**。
- **编排**：w1 先行单分支 `feat/verify-math`；w2/w3 隔波；合并等通知。

## 雾区与后置（触发条件在案）

- **多步题型 + 部分分**（从 w2 剥离）：九题型无多步载体、settle 部分分通道生产
  恒 0/1、quiz 纯 objective 是有意设计——属新题型产品设计；触发条件 = 产品要求
  数学多步题。
- **符号引擎（sympy sidecar）**：v3 决策门——仅当采样核验真机证明不够用才评估；
  带 python 运行时成本，非默认。
- **网络交叉质证**：用户接受暂不做（裁决见上）；立案时直接取用已备设计结论。
- **KB 溯源**：已裁决不适用；若未来学习图课支持绑定知识库，重开。
- **图文指代一致**（「低波动的两条线」类图-文失联）：R 系候选条款；机械查难，
  先观察频率，随任意触碰内容提示词的票顺带。
- **波动率年化（√t 定标）概念位**：进阶节点立项时随批，不单独立票。
- **quality-fixes f3 后置三项**（fix_mermaid_quotes 顺序翻转/末轮收容语义/
  传统线 demo 降级死臂）：方向级留裁决，见 f3 票 Resolution。

## 验证总纲

1. w1 真机判据：smoke content 种子预埋已知收益序列（期望 σ 事先算死），生成正文
   的例题数字与工具核验对账吻合；含数值例题的生成至少一次「核验失败→打回→
   自修改→过闸」真机走通（或如实记录零打回）。
2. w2 真机判据：quiz smoke numeric 题答案键与工具输出一致；变式换参后答案键仍精确。
3. verify_math 属 allo 首创域（learnhub 无对应物），无需参考实码比对，票内记档即可。
   **2026-11-02 更正**：learnhub 已在 `bd31277..b6a8c4e` 独立实现 verify_math
   （ADR-0038 + #186/#187/#188 + #195 铸值 + #196 写侧工具 op）；w2 改为参考
   实码对账，见下方复核节与
   [assets/2026-11-02-w2-numeric-cast-impact.md](assets/2026-11-02-w2-numeric-cast-impact.md)。

## 2026-11-02 复核：learnhub 数学线（b6a8c4e）

**来源**：learnhub `C:\Users\test\Desktop\my\learnhub`，`bd31277..b6a8c4e`
（21 提交 / 66 文件 / +5742 −241；数学线 13 提交）。完整影响面、逐文件落点、
决策矩阵与参考归档见
[assets/2026-11-02-w2-numeric-cast-impact.md](assets/2026-11-02-w2-numeric-cast-impact.md)。

**关键发现：**

1. **w1 的诚实限制被 learnhub 用另一条路补上，但不建议照搬**：learnhub #187
   用 `claims` 自报围栏 + `cc.math.shape/verified` 硬闸 + 装配剥离 + 覆盖率读数，
   让发布正文可重放核验；allo w1 的执法面是工具 op + R14 + finish warning。
   learnhub ADR-0038 增补（2026-11-02）自己把「自报+事后核验」降为过渡脚手架，
   终态 = 写侧工具 op + 判底铸值——allo 已在终态，**claims 围栏不吸收**，
   w1.5 暂不立。
2. **`c14467e` 的求值器预算是 allo 现缺的 P0**：`MAX_EXPR_LEN=512` /
   `MAX_DEPTH=64`；allo 的 `Parser` 无 depth、`eval_expr` 无长度检查，模型
   深嵌套表达式可爆栈。→ **h1 立票**，作为 w2 前置。
3. **#195 铸值是 w2 的直接参考**：模型只给 `claim + precision(0–10) +
   unit(原值|百分数)`，`answer` 由系统 `cast_display(eval(claim))` 铸出；
   变式 `qv.math.claim_required`；管理面重铸；claim 与 answer 同等待遇脱敏。
   w2 采纳其「系统铸值/显示精度容差/claim 脱敏」骨架，但保持 allo 的
   `answer` JSON number + `claim{expr,params}` 参数化，不照搬显示字符串。
4. **#196 写侧工具 op 是 allo 已覆盖面**：learnhub 刚把工具 op 补进 content-write
   （单面），allo w1 已是生成+修复双面 + 计数 + finish warning；不重复吸收。
5. **开放题判分尾部（#181–#185 的 5 提交）不吸收**：allo 无 keywords 机械兜底，
   `degraded`/`CachedGrade` 链无存在条件；`unavailable` UX 与 allo fail-loud
   裁决冲突，属产品拍板项，不立项。
6. **ADR-0039 / dsh 研究 / 清理规则不吸收**：治理约定与 ops，无学习域代码可搬。

**编排**：h1 先行（小票，P0，独立）；w2 紧随（`blocked_by: [w1, h1]`）。
w3 仍隔波，待 w1 真机后按翻译质量抽验再定。

**账目修正**：w1 票面 `status: open` 与正文「真机验收 status: closed」不一致，
本次同步 frontmatter 为 closed；w2 票面追加 2026-11-02 Resolution 增补（旧
2026-10-31 裁决保留，零删除）。
