---
id: n1
title: "题面渲染管线接入——InlineMarkdown 内联通道（KaTeX 一等公民）+ fill_in_blank 转义"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# n1: 题面渲染管线接入（B 线）

**What to build:** 学习模块所有题面（题干/选项/排序条目/答错揭示）从纯文本插值
切到轻量 InlineMarkdown 内联通道——题面契约允许的 `$…$` 公式与 Markdown 内联
形态终于有消费面。吸收 learnhub 3efeb68（真机截图报障：无理数选择题选项 `$…$`
原样露出），外加 allo 特有的 fill_in_blank `___` 与 GFM 冲突处置。

## allo 现状（三代理深挖 2026-10-28，file:line 为准）

- **五修面**（纯文本插值中）：
  ① 题干 `LessonStudy.tsx:75` `{activity.prompt}`；
  ② 选项 `ActivityInput.tsx:142-176`（arco `Radio.Group`/`Checkbox.Group`
  children → `<label><span class="arco-radio-text">` 短语内容位）；
  ③ 排序条目 `ActivityInput.tsx:41`；④ 复习题干 `ReviewSession.tsx:191`；
  ⑤ 复习答错揭示 `ReviewSession.tsx:293-298`（正确答案 + feedback）。
- **口径不一致现症**：同一 `explanation`，课时面走 `<Markdown>`
  （LessonStudy.tsx:100-104 feedback 通道），复习面走纯文本 `<Text>`（:298）。
- **三保持面**（learnhub「截断行有意不动」同款）：题库管理行
  `QuestionManager.tsx:160-170`（line-clamp + Tooltip）、标记待编辑
  `ReviewSession.tsx:439`（line-clamp-2）、匹配题右列 `Select.Option`
  （ActivityInput.tsx:107-111，候选条目本就要求 SHORT）。
- **基建**：`components/Markdown/index.tsx` 有 remark-math + rehype-katex +
  `convertLatexDelimiters`（`\(...\)`→`$…$`），KaTeX CSS 全局 import（:13），
  rehypeRaw 仅 `allowHtml` 时启用（:145，学习面从未开）。**不可复用该组件当
  内联通道**：ShadowView 每实例 shadow root + `*{font-size;line-height}` 钉死
  排版、块级文档流、remarkBreaks 单换行变 `<br>`。
- **触发概率高**：正文提示词明确授权 KaTeX（VISUAL_STANDARD_BLOCK
  generation/lesson.rs:747-49；图线 lesson_content_loop.rs:90 $$公式$$），
  题目被要求只考正文所教且 evidence 逐字抄正文；parser.rs:84-87 专门为 LaTeX
  反斜杠加固过 JSON 解析——后端全程按「题面可能含 LaTeX」设计，唯独前端没接住。
- **quiz 回合 UI 未建面**（q2 后端 only）：`QuizGradeFeedback`（models.rs:2226）
  上线时必须复用本票管线（票面留注，不入本波）。

## learnhub 参考（3efeb68，单提交）

- `src/shared/ui/content/inline-markdown.tsx`（49 行全文）：外层
  `<span data-testid="inline-markdown">` 包 ReactMarkdown；**与正文同插件链**
  （remarkGfm + remarkMath / rehypeKatex，无 rehype-raw）；`components.p`
  覆写为 span 解包（多段落降为多 span，合法嵌入 `<p>`/`<button>`/`<label>`）；
  `a` 覆写强制 `target="_blank" rel="noreferrer noopener"`；RenderBoundary
  fail-visible（渲染抛错降级为「渲染失败+原文」，不静默丢文）。
- 组件选择逻辑：**短文本+短语内容位 → 内联；多段长解析 → 块级
  （MarkdownContent，720px 阅读宽度）；截断列表行 → 保持纯文本**。
- 消费面 diff：session-runner.tsx:524-526（题干）/ :545（选项 button 内）/
  :423-432（答错揭示·正确答案与要点拼串）/ :586-587（判分解析换块级）；
  question-drawer.tsx:106-138（抽屉题干/选项/解析全内联）；review-page.tsx:
  174-176（复习卡 stem）。
- 测试：content.test.tsx:67-89 三例（`$0.375$` → 2 个 .katex 且无裸 `$`；
  双段落 → 无 `<p>`；坏公式 `$x^{$` → fail-visible 原文可读）；
  learn-page.test.tsx:364-408（整会话流断言 3 个 inline-markdown + KaTeX）。

## 实施要点

- [ ] **InlineMarkdown 组件**（`ui/src/renderer/components/Markdown/` 下新文件，
      light-DOM、无 ShadowView）：同 allo 正文插件链但**排除 remarkBreaks**、
      **复用 `convertLatexDelimiters`**（否则 `\(...\)` 题面不出 KaTeX）、
      **不开 allowHtml**（题面源 = 材料摘要+模型产物，`generation/mod.rs:39`
      明言 untrusted，裸 HTML 默认转义是底线）；p→span 解包；错误边界
      fail-visible（兜底文案进 i18n 双语——本票唯一新 key 面）。
- [ ] **fill_in_blank `___` 占位符预处理（allo 特有，最易漏）**：进 markdown
      解析前把 `___` 换成占位标记，渲染为带下划线空位 span（题干必含一个
      `___`——`generation/mod.rs:110` 契约），防 GFM 把独占行 `___` 吃成
      `<hr>`、成对吃成粗斜体。九题型里仅此题型触发。
- [ ] 五修面接入：LessonStudy:75、ActivityInput 选项/排序、ReviewSession:191、
      :293-298。复习面 feedback 口径 = **与课时面对齐走 Markdown 块级**
      （explanation 可能含 ```svg fence，纯文本会露源码）；正确答案文本内联。
- [ ] 判分解析/答错揭示含块级围栏时走块级 `<Markdown>`，短文本走内联——
      分流规则一处实现（组件内或调用点 helper），两页面共用。
- [ ] 测试：InlineMarkdown 单测四组（KaTeX 渲染无裸 `$` / p 解包 / fail-visible /
      `___` 空位）；learning 页面集成断言（含公式选项渲染 .katex）。
- [ ] 门禁：typecheck / i18n（新 key 双语对称）/ theme（语义 token，
      light-DOM 内联组件用 `text-t-primary` 族）/ dead-css。

## 验收

- vitest 全绿（新增组件 + 页面断言）；`bun run check` ui 侧全过。
- 手工走查：数理课程课时（含 `$…$` 选项）、fill_in_blank 空位、匹配题、
  复习卡答错揭示，两主题（light/dark）各一遍。
- **不实测不关票**：本票效果面就是渲染，截图/录屏附 Resolution。

## 非目标

- quiz 回合 UI 建面（未建，留注复用管线）。
- 题库列表行/Tooltip 截断面（保持纯文本）。
- 生成面题干格式条款（n3 若动 activities 契约可顺带补「题干允许 `$…$`、
  禁多段 markdown」一句，本票不动提示词）。
- AI 加题草案编辑表单（编辑面非渲染面）。

## 备注

- learnhub 判分解析选块级的理由：解析可能多段、需 720px 阅读宽度——allo 的
  explanation 还可能带 ```svg，块级通道已有（feedback 主通道），分流规则照此。
- ShadowView 变量表无需动（内联组件不进 shadow）；若未来有人把内联组件嵌进
  shadow 容器，KaTeX 样式穿不进去——票面留档此约束。

## Resolution（2026-10-29 关票）

- 实施 = `cf1ddee86`（InlineMarkdown light-DOM + 五修面 + `___` 私有区占位符
  + QuestionExplanation 分流，13 文件 +641/−10，19 新测全绿、typecheck 61
  基线持平、i18n 11530 键双语）。波次经 e08e65d60 并入 feat/learn0928。
- 实现中裁决（如实记录）：①测试运行器按仓库实际 = `bun test`（票面写 vitest，
  仓内无 vitest）；②分流谓词比「仅围栏」宽（多段/行首标题/列表/展示公式也走
  块级——span 宿主禁块级元素）；③错误边界加换文复位；④链接走
  `openExternalUrl` 平台纪律；⑤保留 remarkSafeBreaks（票面只要求排除
  remarkBreaks）；⑥复习正确态横幅与匹配题左列未动（票面修面清单外）。
- **单测面证据**：learning+Markdown 分片 109 pass（修复批后）。**两主题手工
  走查未做**（真机阶段跑 smoke 二进制未起 dev 栈）——如实留观：组件断言已
  覆盖 KaTeX/`___`/转义渲染正确性，视觉走查留待下次 dev 会话目验，
  [对照卡](../assets/verification-cards.md) §n1 有记档。
