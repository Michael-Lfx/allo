---
id: k03
title: "est 时长预算行进图课时提示词 + 超容自报 capacity_note（形状零拦截）"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# k03: est 时长预算行 + 超容自报 capacity_note

**What to build:** allo 的 est（j01 全贯通命名）从未进过课时生成提示词——模型
规划节容量时看不见节点尺寸锚。吸收 learnhub #154：图课时两阶段提示词注入
「时长预算」行，计划产物加可选自报键 `capacity_note`（超容如实一句、不超容
不写=缺席合法、形状条款只查非空、零内容拦截），报告面透传。

## allo 现状

- est 字段链：`ProposedNode.est`（`learning_graph/mod.rs:100-108`）→ 契约键
  `ng.nodes.est`（`growth_contract.rs:20`，1-60）→ DB 列
  `learning_lessons.estimated_minutes`（迁移 015:62，**NOT NULL DEFAULT 10——
  无缺席路径**）→ `LessonView.est`（`models.rs:1904`）。
- **est 不进任何课时提示词**：图路径用户面 `lesson_user_text`
  （`nomifun-ai-agent/src/lesson_content_loop.rs:509-560`）注入目标/程度声明/
  前导路径/概念面，无 est；课时行 SELECT（`service/lesson.rs:731-737`）未取
  est 列。agent 系统提示词只有 `estimated_minutes：5-60 的小整数`
  （`lesson_content_loop.rs:90`，那是让 agent **回填** est 的工具条款）与
  篇幅 `{prose_budgets}`。
- 传统路径已有机械预算：每节「本节文字预算」行（`generation/lesson.rs:608/831`）
  + 节数/字数规则（`models.rs:722/:758`）+ 纪律块「15-30 分钟课时」措辞
  （`lesson.rs:764`）。
- 审计观察条先例（j07）：`lesson_draft.rs:955-1036`。

## learnhub 参考（#154，契约 v2.10）

- 预算行装配：`src-tauri/src/engine/contentgen/facade.rs:194-200`——
  `est Some → "- 时长预算:{est} 分钟(节点尺寸锚)"，None → 整行省略`；进
  `node_brief`，**两阶段共用**（plan :232/:239、write :261-271）。
- 超容自报：`prompts/contentgen/content-plan.md:35` 填写时机句（节清单定稿后
  对照时长预算粗估，明显超容才写，如实留痕不改安排）；形状条款
  `cp.fields.capacity`（`validator.rs:1267-1280`：出现时非空字符串、空白=空、
  **永不退稿**）；提取 `extract_capacity_note`（facade.rs:546-552，trim、
  空→None、不造）；透传 `GeneratedContent.capacity_note` → 投影
  `NodeContentSummary.capacity_note` + 落位日志 `over_capacity` 开关
  （`app/generation.rs:1059-1120`、facade.rs:301）；schema 白名单随批互钉。

## 实施要点

- [ ] **图路径预算行**：`service/lesson.rs` 课时行 SELECT 补 est 列 →
      `lesson_user_text` 图分支（`lesson_content_loop.rs:517-535` 段）注入
      `- 时长预算：{est} 分钟（节点尺寸锚——规划与写作两阶段对照此预算）`。
      allo est 列 NOT NULL DEFAULT 10，learnhub 的省行分支不适用（恒注入）。
- [ ] **capacity_note 可选键**：计划工具（ls_plan）输出 JSON 加可选键
      `capacity_note`（serde default 容旧包；出现时非空字符串形状条款——
      恶劣值裁剪为 None 不退稿）；`LessonDraft.capacity_note` 字段透传 +
      观察读数（j07 审计条同款 warning 通道 + `over_capacity` 日志行）；
      ls_inspect 投影可见。
- [ ] 写作阶段提醒句（learnhub write 面共用同预算行）：节正文系统提示词
      （`section_body_*` / agent 写作工具条款）加一句对照预算的取向句，
      零新机械门。
- [ ] 开关 `NOMI_LEARNING_EST_BUDGET=off`：预算行不注入、capacity_note 键
      忽略（=旧行为）。
- [ ] 传统路径**不改**（机械预算已足够，见 allo 现状；范围注记进契约文档）。

## 验收

- **单测（红绿）**：预算行装配断言（含 est 值；off 臂不含该行）；capacity_note
  形状三态（缺席合法/非空通过/空白串按空裁 None）；透传与观察读数在场；
  旧课时包（无键）反序列化兼容。
- **smoke content 档双臂（开关臂）**：硬判据——on 臂观测存档
  （`calls.jsonl`）里课时用户提示**含「时长预算」行**（对存档机械 grep），
  off 臂不含；EXIT 码两臂均按既有判据。capacity_note 出现与否打印读数行
  （非门禁——自报是自愿行为，learnhub 同款零拦截）。
- **对照卡**：双臂同 fixture 课时各一次真机采样——预算行在场性（机械）+
  节容量 vs est 预算读数并排 + capacity_note 文本（若有）如实落 Resolution。
- `cargo test -p nomifun-learning` `-p nomifun-ai-agent` 全绿；跨 crate 改动
  走 nomifun-ai-agent 桥（lesson_content_loop 属该 crate，j07 先例）。

## 非目标

- 不做超容机械门/退稿（learnhub 明确零内容拦截， allo 同）。
- 不动传统路径节预算与课时数字排布（kb-v2 h 系刚收口）。
- 不加受众行（learnhub audience_line 依赖课程宪章 domain.audience，allo 无此
  数据源；R11 语域条款在 k05 以既有输入面措辞吸收）。

## 备注

契约文档 `learning-structure-artifacts.md` 若涉课时输入面，随批加一行注记
（est 预算行 + capacity_note 形状），bump 小版本。
