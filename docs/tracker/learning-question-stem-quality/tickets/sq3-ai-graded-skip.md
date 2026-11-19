---
id: sq3
title: AI 批改题跳过：练习轮 skip 按钮 + skipped attempt 语义 + 账本不变式
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

开放式问题（reflection/open_question）对精力和其它条件的要求相对苛刻
——要求学习者现写一段自己项目里的代码并逐值分析才判分。应该提供跳过
按钮。实锤：用户在 reflection 文本框键入「跳过」二字，被 AI 判 0 分
+ 六条缺失要点清单——跳过目前没有第一类语义，只能伪装成作答挨批。

## 已核验事实（2026-11-19 对码）

- submit_attempt（progress.rs:131-262）：AI 批改题提交即 LLM 判分
  （n5 新评审链/旧臂两路）；空答案被 evaluate 拒（「无实质内容」判 0）。
- learning_attempts 表约束（migration 015）：score REAL NOT NULL
  CHECK(0.0–1.0)、passed INTEGER NOT NULL IN(0,1)——skipped 无法以
  NULL 直存，零迁移方案必须走标记面。
- 前端作答态：isAnswered = hasAttempt(attemptResults)
  （LessonStudy.tsx:475）；stepLocked 以 attempt 为门（model.ts:94）。
  **跳过必须持久化为 attempt 才能保持门开**（否则刷新后门重关）。
- 复习/检测面无副作用：种卡双闸 is_objective() && review_eligible()
  （progress.rs:693）把 AI 批改题挡在复习队外；检测轮选题
  OBJECTIVE_KIND_SQL_LIST（quiz.rs:297）同样不含——跳过无复习面涟漪。
- AI 批改判分失败（LLM 错/超时）会以错误浮给学习者阻断——跳过顺带成
  为该终局失败的逃生门（票面范围内的红利，不另立面）。

## Terms（方案草案，施工前过裁决）

- **R-a 后端 skipped attempt**：提交通道加 skipped 语义——裁决点：
  复用 attempts 端点 + response marker `{"skipped":true}` vs 独立
  /skip 端点（倾向前者：少加面）。落库 score=0.0/passed=0 + marker +
  feedback 固定文案（schema 零迁移）。**消费面排查清单（全要留对码证
  据）**：薄弱集查询（u9）剔 skipped、XP 读数/ETA 不计 skipped 失败、
  打卡/账本不变式不受污染（学习账本独立性，ADR-0017 族）、复习种卡面
  （本就不种，确认不回归）。
- **R-b 前端**：练习轮 AI 批改题卡加「跳过」钮（reflection/open_question
  限定；**客观题不加**——防刷门语义）。跳过后 hasAttempt 为真、门放
  行、卡片呈「已跳过」态（ danger 红评语不再出现）；已跳过题保留「重
  答入口」（attempt 追加式与现有一致——裁决点：是否允许重答，倾向允
  许）。i18n 双语 + theme token + dead-css 闸。
- **R-c 语义边界**：跳过 ≠ 作答 ≠ 失败——不进薄弱集、不计 XP、不改
  记忆面；重复进入练习轮已跳过题保持已答态（与既有 attempt 行为一致）。
- **R-d 复习线不做跳过**：AI 批改题本不入复习队/检测轮，本票范围只在
  课时练习轮——如实写明，不开新面。

## Acceptance

- 后端 skipped 单测：落库形状 + 薄弱集/XP/ETA 三消费面剔除各一例。
- 前端交互测试：门放行/已跳过态/客观题无此钮/重答入口。
- 账本不变式测试不回归（打卡/XP 与跳过零耦合）。
- typecheck / i18n / theme / dead-css 绿；真机目验欠账。
