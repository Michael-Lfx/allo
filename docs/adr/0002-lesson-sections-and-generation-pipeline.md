# ADR-0002: 课时分节模型与三段式生成管线

Status: Accepted

Date: 2026-09-08

## Context

学习模块参考同作者的 learnhub-plugin 升级：其术语 context 99% 适用于本项目（已落在根目录
[CONTEXT.md](../../CONTEXT.md)，两边词汇保持同源），大愿景一致，当前聚焦学习模块。

现状：层级为 Course → Module → Lesson，课时正文是单条 `summary` markdown，强制三个
`##` 小节（描述/例子/验证）；题型 4 种（single_choice / true_false / reflection /
fill_in_blank）；每课时两阶段生成（整篇文档 → 活动 JSON）；提示词硬编码为 const。
整篇长文档一次生成的质量与可读性都到顶了。

分节的核心价值（按权重排序）：**可读性**、**学习节奏**、**大模型注意力**——一次调用只写
一节，单节质量显著高于整篇文档一次生成。learnhub 的"大纲 → 逐节生成 → 出题"管线已被
验证，本 ADR 记录移植到 allo 的决策与取舍。

课时容量标准两个项目一致：**绝大多数课时应低于 30 分钟；只有不可拆分且非常复杂的课时
才允许超过 30 分钟，但绝对不超过 60 分钟。**

## Decision

**术语**：Lesson 对齐 learnhub Node 语义，沿用 Lesson 命名（代码已落地，不做大规模改
名）；引入 Section（节段）。词汇表见 CONTEXT.md。

**存储**：新表 `learning_lesson_sections`（lesson_id, position, section_id, title,
type, status, version, body_md, points），manifest 字段对齐 learnhub 的
SectionManifest，作为逐节生成进度的持久事实源，支持断点续跑与单节重写。正文按节存。

**节类型**：首期 5 种——概念、例题、演示、小结、练习。演示节由现有 svg / jsxgraph /
mermaid / KaTeX 渲染承载；练习节为题组承载（正文只写能力目标与作答引导，题目从
activities 出）。

**题型**：4 种 → 9 种，完整采纳 learnhub 的 AlloKind——新增 multi_choice、numeric
（带容差）、ordering、matching、open_question。7 种规则判卷、2 种 AI 判卷
（reflection 沿用现有 AI 批改；open_question 走 0–10 分制，≥6 及格）。需重建两张题目
表的 kind CHECK 迁移。

**管线三段式**（每课时）：

1. 大纲：一次调用产出节清单（严格 YAML：id / title / type / points），同时自声明该
   课时复杂度档位（低/中/高）；
2. 逐节串行生成：每节一次调用，注入前节已生成正文保连贯；质检门（超纲、篇幅、格式）
   失败带定位回灌修复；
3. 出题：课时级一次调用，模型拿到全部已生成节正文，产出全部题目并标注 section id
   （跨节综合题标"通用"）。

轮次预算 = 大纲 1 + 节 N + 出题 1（N+2）。复杂度锚点沿用 learnhub（低 [1,3] 节 /
中 [3,5] / 高 [4,6]，上限 8 节；每节题量低 2 / 中 3 / 高 4），护栏只拦方向性极端
（低档 >7 节、高档 ≤2 节、任意 >8 节）。

**修复宽容度**：每节最多 2 次修复轮，整课时修复轮总数封顶 4 次。修复仍失败 → 该节标记
failed 跳过继续，课时终态 partial，该节可单独重试。

**提前进入学习**：至少一节 ready 正文即可进入课时；未生成节显示"生成中"；练习入口在出
题轮完成前禁用并提示"题目生成中"。生成全程后台继续。这是化解串行生成时长担忧的关键。

**提示词**：保持硬编码 const，内容按 learnhub 重写（节类型菜单 + 各类型创作要求；防超
纲 contextPack 三件套：禁止使用概念 / 前置摘要 / 复杂度档案；可视化为主、文字为辅；
节内不写子标题）。内置三种讲解风格变体：标准 / 苏格拉底 / 费曼。风格在课程创建对话框
选择，存 `learning_courses`，课程内所有课时一致。

**兼容**：旧课时双读——节清单为空时回退渲染 summary，新生成走分节；不做数据回填。

**前端 UX**：课时内改为节进度 stepper——内容节 = 读 + 本节练习并成一步，练习节 = 一等
练习轮。课时完成语义保持课时级不变（完成全部活动），节不新增持久化完成状态。

## 妥协与暂缓（供以后参考）

- **交互节（learnhub 第 6 种节型）暂缓**：依赖 learnhub 独有的交互件体系（自包含
  HTML widget、LEARNHUB_COMPLETE 完成上报、LEARNHUB_TEACHER AI 老师接口、交互件门
  禁 200KB/禁外联），allo 无此体系。立项时需先设计交互件契约，再补第 6 种节型。
  DB 的节类型 CHECK 已按不含 `interactive` 收紧，扩型需再迁移。
- **节清单用严格 JSON 而非 ADR 初稿所写的 YAML**：实现期选择复用 crate 已有硬化
  JSON 解析（容错围栏/尾逗号/LaTeX 转义 + 定位重试）而非引入 YAML 解析器；约束
  力等价（schema 校验 + 护栏不变）。
- **出题从 learnhub 的"逐节出题"妥协为"课时级一次出题"**：learnhub 每内容节一次出题
  调用 + 综合题（轮次 ≈ 2N+1），为控制轮次与时长改为一次调用。若实测出现绑节错误或跨
  节遗漏，回退方案是恢复逐节出题。
- **提示词不做版本化落盘 / 用户可编辑**（learnhub 落盘到 `state/提示词/*.md`）：allo
  的 agent loop 与 learnhub 插件 harness 不同，轮次与时长预算更紧，先保持硬编码以
  控制轮数；扩展风格只需加 const 模板，落盘机制后置。
- **agent loop 保留单篇文档契约作为旧路径**：`ls_set_document` 仍是合法工具（旧会话
  与测试兼容），但提示词与工具面以分节契约（`ls_set_section_manifest` → 逐节
  `ls_set_section_body` → 绑节出题）为主路径——每节一次工具调用即"分职责轮次"，
  在现有 50/20 轮预算内；审计对两种形态各自把关。
- **手工出题表单首期只覆盖 4 种经典题型**（单选/判断/填空/反思）：multi_choice /
  numeric / ordering / matching / open_question 由生成管线产出并全链路可作答可复习，
  但"添加问题"对话框暂不提供手工创建（排序/匹配需要成对编辑器）。后续按需补表单。
- **蓝图 schema 不加 difficulty / bloom 字段**：档位由大纲阶段自声明；若护栏误判增
  多，再考虑蓝图阶段补字段由后端折叠档位。
- **旧课时不回填分节**：双读回退（节清单为空渲染 summary）即可；重新生成是用户显式
  动作。目录视图不加载节正文（体积），只在课时详情加载。
- **`estimated_minutes` 现状默认 10 与 30 分钟容量标准不符**：待新管线落地后核对生成
  est 分布，如仍对不上按 bug 处理。出题提示词已按 10-30 典型 / 60 上限约束。

## Consequences

- 迁移均为 append-only：learning_lessons 下新表；learning_activities 与
  learning_custom_questions 的 kind CHECK 重建。
- 每课时生成轮次 ≈ N+2（另加修复 ≤4），串行生成的时长通过"提前进入 + 后台继续生成"
  化解，不阻塞学习与复习。
- 前端 `ActivityInput` 需新增 5 种输入形态（多选 / 数值 / 排序 / 匹配 / 开放题），
  题型渲染按 kind 注册表化；课时页从整页 Markdown 改为按节 stepper 分步交付。
- 生成提示词全部重写，`validate_lesson_document` 的三小节契约由节清单 + 节级校验
  取代。
