---
id: sq3
title: AI 批改题跳过：练习轮 skip 按钮 + skipped attempt 语义 + 账本不变式
type: task
status: in-progress
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

## Resolution（2026-11-19 施工落地，wave/sq3）

### 裁决点落地

- **R-a 跳过通道**：按票面倾向落地——复用 attempts 端点
  （`POST /api/learning/activities/{id}/attempts`）+ response marker
  `{"skipped":true}`，独立 /skip 端点不开（少加面）。落库 score=0.0 /
  passed=0 / feedback 固定英文账本 prose（`SKIPPED_ATTEMPT_FEEDBACK`）+
  信封内 response marker，schema 零迁移。判据 `response_marks_skipped`
  （progress.rs）；**客观题提交标记 = 400 BadRequest**（防刷门语义的
  服务端自守半边，不依赖前端自觉；守卫缺位时标记会被规则判卷吃成一次
  「答错」attempt 污染账本）。
- **消费面排查（三处对码证据 + 剔除）**：
  1. **薄弱/趋势面（唯一 attempt 直读面，实改）**：`fold_behavior_digest`
     （service/learning_graph.rs:3084 起，窗口候选 + 停滞基线两查询）与
     `covered_node_signals`（service/quiz.rs 作答触及流）加统一剔除谓词
     `attempt_not_skipped_sql(alias)`（`COALESCE(json_extract(
     response_json,'$.response.skipped'),0)=0`，NULL 语义安全——`NOT(…=1)`
     会被 NULL 吞行）。u9 薄弱集复习会话本身走 review 队列（种卡双闸
     `is_objective() && review_eligible()`，progress.rs:692 起——AI 批改
     题本就不种卡），结构性不受影响，不回归：跳过无复习面涟漪如实成立。
  2. **XP 读数/ETA（零改动，结构证明）**：`settle_node_completed` 的
     answered/passed 双侧同筛 `OBJECTIVE_KIND_SQL_LIST`（xp.rs:68-92，
     n4 D6 既有口径）——skipped 只存在于 AI 批改题型（服务端守卫），
     结构上进不了分子分母；`xp_reading` 今日面/ETA 读 `learning_xp_events`
     （xp.rs:255 起），跳过零事件写入。单测
     `skipped_attempt_leaves_xp_ledger_and_checkin_untouched` 钉住。
  3. **打卡账本（零改动，同源证明）**：`checkin_today` 只读
     `learning_xp_events` + `learning_checkins`（checkin.rs:92/102），
     与 attempts 零 join——跳过与打卡零耦合，同一单测钉住。
- **R-b 重答裁决**：允许重答（票面倾向）。实现为前端本地「重答」钮
  （已跳过态内）：点开即回未作答形态（可再跳过或作答），提交走既有
  attempt 追加式——`skip_attempt_marks_skipped_without_grading` 断言
  重答后 attempts 行数 = 2（只增不改）。已跳过态：中性 secondary 徽标
  （danger 红评语/缺失要点块不再渲染）。
- **R-b 跳过钮范围**：reflection/open_question 限定，客观题不加
  （防刷门语义前后端各半）。落点在 ActivityBlock（练习轮 + 旧清单挂题
  面同卡同语义）；复习线（ReviewSession）不动——R-d 如实不开面。
- **模型门豁免**（票面外的必要半边）：`useCourseLearning.submitAttempt`
  的模型门对跳过提交放行（`isActivitySkipResponse`，model.ts 单源）——
  跳过零 LLM 调用，模型门把它锁死则逃生门名存实亡；同时跳过天然是
  「判分失败终局」的逃生门（票面红利成立）。
- **语义边界 R-c**：跳过 ≠ 作答 ≠ 失败全数落地——不进薄弱集/趋势
  （剔除谓词）、不计 XP/ETA（结构隔离 + 单测）、不改记忆面（种卡双闸
  不回归；跳过零 review_log 写入）、步进门放行（attempt 存在即
  isAnswered，与普通作答同轨）。

### 落地哈希

- `00dc34852` feat(learning): AI 批改题跳过——skipped attempt 语义 +
  消费面剔除 + 练习轮跳过钮 (sq3)（分支 wave/sq3，作者 zhangyj）

### 检查记录（2026-11-19）

- `cargo test -p nomifun-learning`：996 passed / 0 failed（含新增 5 例：
  跳过落库形状与追加重答、客观题拒绝、行为摘要剔除、节点信号触及剔除、
  XP/打卡账本不变式）。
- `bun run typecheck` 绿；learning 前端测试 `bun test src/renderer/pages/
  learning`：498 passed / 0 failed（含新增 LessonStudy.activity-skip 8 例
  + model 跳过标记 2 例）。
- `bun run gen:i18n && bun run check:i18n` 绿（i18n-keys.d.ts +4 键）；
  `bun run check:theme` / `bun run check:dead-css` 绿；
  `cargo fmt -p nomifun-learning` 已跑。
- 真机目验欠账（票面 Acceptance 末条，留给双轴审查后 smoke）。
