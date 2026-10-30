---
id: f3
title: "课时可视化块机械 lint——坏 mermaid 不再过闸"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# f3: 课时可视化块机械 lint

**What to build:** 课时正文的 ```mermaid 可视化块加最小机械校验（围栏内语法
配平检查）——坏图打回重写一次，仍败降级剔除 + audit findings 留痕，不留坏
图进库。

## 法证证据（e2e2 真课「滚雪球」s1 节）

正文数学全对（110/121/133.1 ✓），但概念节的 mermaid 块：
- **语法坏**：`A[["第1期 本金 P] -->|"算利息 P×利率"| B[利息1"]]`——节点 A
  `[[` 开、单 `]` 收（括号不平衡），B 内游离引号；
- **语义碎裂**：A→B 一段 + C→D、E→F→G 两段，**三块互不连通**，不是一张图
  而是三个碎片。
- 成因画像：VISUAL_STANDARD 门要求「≥1 可视化块，纯文字会被质检门拒绝」——
  模型为过门交了低质图形（卡门产物）。对照：另一课同门的 ```svg 块质量良好
  （svg 有 sanitize 先例、生成更稳）。

## allo 现状（落点提示）

- 可视化门条款：`generation/lesson.rs:747-749` VISUAL_STANDARD_BLOCK（传统
  线）+ `nomifun-ai-agent/src/lesson_content_loop.rs:90`（图线，$$公式$$/
  ```svg/```jsxgraph/```mermaid/表格 五选一）。
- 装配/校验面：课时草稿 op 流（ls_set_section_body）与审计 findings
  （lesson_draft.rs 的 audit 观察条先例：over_capacity/naming_*）——lint
  落点应在草稿 op 接纳处或装配校验处（实现会话定位，两面取一，别双处）。

## learnhub 参考

- `engine/contentgen/validator.rs`（cc.* 条款族）：**他们没有可视化 lint**
  （cc.steps.present 只是节数下限）——本票是 allo 首创小闸；可参考的只有
  其「条款打回→重写→仍败降级」的通用形状与 svg sanitize 先例
  （`ui/src/renderer/components/Markdown` 的 CodeBlock svg 路由是消费侧）。
- learnhub 分面裁决（ADR-0014）注记：正文走文本接缝避开 JSON 转义雷区——
  同理 mermaid 语法错误模型侧自检不可靠，**机械闸是唯一可靠的**。

## 实施要点

- [ ] **最小语法闸**（Rust 侧轻实现，勿引 mermaid 解析器依赖）：围栏内逐行
      检查——①方括号/圆括号配平（`[[`→`]]` 成对）；②节点 id 被边引用则须
      有定义；③`---` 分隔符后页面声明的闭合。只做「明显是坏的」三查，
      不追求全语法。
- [ ] **打回一次 + 降级**：lint 失败 → 打回该节重写（带死因「mermaid 语法
      破损：括号不平衡」指引）恰一次；仍败 → **剔除该可视化块**（节正文
      其余保留）+ audit findings 留痕（新观察条 `viz_block_lint` warning，
      带节 key 与原因）——正文不因图烂而整课失败。
- [ ] 两种可视化形态同闸：```mermaid 全检；```svg 已有 sanitize（消费侧）
      ——生成侧只补「非空且含 <svg 标签」最小检查（防空壳块过门）。
- [ ] 红绿样：坏括号 mermaid 被打回；重写成功过闸；两连败降级剔除 + findings
      行；svg 空壳块打回。
- [ ] smoke content 读数行补 `viz_lint_events`（读数非门禁）。

## 验收

- `cargo test -p nomifun-ai-agent` + `-p nomifun-learning` 全绿；fmt；零迁移。
- 真机判据：连生两课，可视化块 lint 零打回或打回后自愈；audit findings 无
  viz_block_lint warning（出现即如实记录）。

## 非目标

- mermaid 全语法解析器（引依赖，超预算）。
- 可视化门本身（≥1 块的要求不动；模型改交表格/公式也合法）。
- jsxgraph 形态（先例少，随 mermaid 闸的括号检查顺带覆盖，不单列）。

## Resolution（2026-10-30 grilling 裁决，影响面核查后；真机结果可复议）

**裁决（用户：按推荐）：**

1. **打回语义：放弃「恰一次」**，lint 红条并入各线既有打回通道（图线 = op 拒绝
   reason + 修复对照行；传统线 = lint 并入 validate_body 走 attempts 循环）；
   发布/装配终检仍坏才触发剔除兜底。
2. **剔除兜底 = 修复轮前置 + 系统兜底**：修复对照行「修 mermaid 或换表格/公式
   承载」（真机对照另一课同门 svg 质量良好，模型多数能自愈）；预算尽仍坏 →
   系统剔坏块 + 同步降该节 visual 声明（系统 mutation，非模型 op）+ degraded
   留痕。纯剔块撞三道门（见勘误），降声明是合法存活前提。
3. **lint 查项修订**：①括号配平——块级累计（非按行，classDiagram 跨行块防
   误杀），引号段豁免，六对括号；②节点定义查仅对 `flowchart`/`graph` 前缀
   生效（sequence/state 参与者免定义，防误杀）；③「`---` 闭合」查砍掉（歧义
   语法+法证案例未死于此）。

**影响面核查勘误（2026-10-30 实码核实）：**

- 五选一条款实为 lesson_content_loop.rs:110（90 是 static 起点）；传统线
  VISUAL_STANDARD_BLOCK 实际 743-764，且 902 行已有 mermaid 引号卫生条款。
- **「learnhub 没有可视化 lint」不准**：cc.visual.form + cc.svg.spec
  （validator.rs:1000-1031；check_svg_block 495-611：viewBox/字号/白名单/容器
  闭合）就是 svg 规格闸——svg 侧有先例可抄形状，**首创的只是 mermaid 语法闸**。
- **learnhub 内容线 fail loud 无降级臂**（facade.rs:480/573 Exhausted）；降级
  先例在 allo 本仓：generation/lesson.rs:295-327（visual=无 保底重写）+ 561
  （`for degraded in [false, true]`）。
- **「只剔块保正文」撞三道门**：承诺门（models.rs:562-606，声明「流程图」只认
  mermaid，剔后 section_invalid danger）、demo 硬门（532-538，唯一块被剔=danger，
  不按声明豁免——degraded 豁免通道必须覆盖它）、文字下限（490-506；**实核不撞**：
  prose_char_count 本就剥围栏计算，剔块不减 prose）。草稿无单节改 visual 的模型
  op → 降声明必须系统侧 mutation；degraded_keys 图线恒空（lesson_draft.rs:947）
  → 需接线穿出。
- **「两面取一」不成立**：面 A（草稿 op 接纳）对传统线无效（不经过草稿）；面 B
  （装配）时模型循环已结束无法打回。落地形状 = lint 函数单源 + 各线循环接入
  （打回）+ finish 终检（剔除兜底）。lint 函数落位 models.rs（validate_body
  同文件邻域，两线共享汇点最短依赖）。
- findings 无持久表：LessonFinding.kind 裸 String 零注册成本；留痕通道 =
  草稿 findings warning（viz_block_lint 触发 / viz_block_lint_dropped 剔除）+
  tracing::warn（service/lesson.rs:2084-2091 先例）+ publish 帧计数
  （lesson_content_loop.rs:340-349）→ smoke `viz_lint_events` 读数（非门禁）。
- fix_mermaid_quotes（lesson_draft.rs:727 调用、generation/parser.rs:93 定义）
  零单测——lint 红绿样顺带补齐围栏状态机测试面。
- 消费侧：MermaidBlock 渲染失败静默源码陈列（MermaidBlock.tsx:112-118，无错误
  提示无修复按钮；svg/jsxgraph 有 AI 修复）——前端零改动，修生成门不修显示。
  FIGURE_REPAIR 端点只支持 svg/jsxgraph 不含 mermaid（completer.rs:66-103），不扩。

施工图：[blueprints/2026-10-30-f3-viz-lint.md](../blueprints/2026-10-30-f3-viz-lint.md)。

## 真机验收（2026-10-30，quality-fixes-verify）与实施备注记档

**判据全过。**content smoke 两课（a3/b3）EXIT=0 判据全过：

- `viz_lint read-out: viz_lint_events=0` 双零——两课零打回（读数非门禁，
  在场即判据面）；audit findings 零 viz_block_lint warning。
- 瞬断注入臂恢复设计语义：`injected_remaining=0 attempts_used=2
  stop_reason=success`。

**实施备注四条（记档，①③为方向级留裁决）：**

1. `fix_mermaid_quotes` 洗白效应：法证原行的单行 `[`..`]` 特异形态经引号
   改写后落入引号豁免段过闸（引号改写先于 lint 是蓝图禁区保持的顺序）。
   lint 层红样仍钉死原行；无闭合 `]`/多行失衡/引号奇数形态照拦。若真机
   后续出现该形态漏网成规模，再议顺序翻转（方向级）。
2. flowchart 裸边端点（`A --> B` 无形状）按蓝图伪代码从紧记
   MermaidNodeUndefined——严于 mermaid 语法（合法隐式节点），恰合
   VISUAL_STANDARD 质量 bar，绿样钉住。
3. 终检兜底可达性：op 面硬拒使正常路径草稿内无坏块，sweep 只覆盖旁路形态
   （重规划保留/跨版本续跑）；「模型反复交坏图」按 A3 由循环预算收口为整课
   失败——真机若现「需末轮收容」语义再议（方向级，未擅动）。
4. **存量发现（非本票引入）**：传统线 demo 节降级保底（lesson.rs:295-327/561
   visual=无 重写）对 demo kind 恒失败（demo 硬门 vs 纯文字降级稿）= 死臂，
   f3 保持原语义零新机制；修复需方向裁决，留后续票。

**验收解锁的 e4 存量缺陷修复（`ddb464348`）**：flaky_provider 计数下溢——
无条件 `fetch_sub` 在计数归零后回绕 usize::MAX，掐流永不恢复，content 档
三试耗尽 EXIT=2（真机实录「还剩 18446744073709551611 次」）。e4 关票真机走
HTTP 旅程未踩注入臂，故存量未曝；与本票零代码关联（f3 未触碰该文件）。
修复 = `compare_exchange` 仅在 >0 时扣减 + 归零恢复透传回归钉单测；修复后
注入臂恢复 attempts_used=2/success。提交 `c7048d231`/`515ac83c1`/
`b701cd1a2`（票本体）。
