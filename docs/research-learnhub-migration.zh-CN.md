# 调研：learnhub 学习引擎概念向 allo 学习域的迁移方案

调研日期：2026-09-09
调研范围：仅两份一手材料——`C:\Users\test\Desktop\my\learnhub-plugin`（dsh 学习引擎插件，下称 learnhub）与本仓库 `feat/zyj0907` 分支（下称 allo）。全部结论附出处（`learnhub-plugin:路径` / `allo:路径:行`）；未逐一核对的点明示「未验证」。本文件只读调研产出，不构成实施决策。

## 0. TL;DR

1. **底座已经同源**：allo 已落地题目级 FSRS 卡（每道客观题一张独立记忆曲线）、Forgot 申报、答对后 Hard/Good/Easy 自评、自建题、tags/归档/edit-pending、9 种题型、分节生成与练习节逐题作答 + 节步进门禁——learnhub 的核心评分模型本体（`allo:crates/backend/nomifun-db/migrations/039_learning_review_question_level.sql:1-14`、`allo:crates/backend/nomifun-learning/src/service/review.rs:83-85`）已在，不要再把它当缺口。
2. **最值得迁的第一批是「调度诚实度三件套」**：逐次复习日志（rating/rating_source/elapsed_days/复习前 S·D·R 快照）→ 真实保留率与预测对照仪表盘 → FSRS 参数优化器。learnhub 为此立了 ADR-0012 并证明「一条日志喂两个消费方」（`learnhub-plugin:docs/adr/0012-a2-memory-health-dashboard-and-param-optimizer.md`）；allo 的 `learning_review_events` 目前只有 `(source, item_id, created_at)`（`allo:crates/backend/nomifun-db/migrations/042_learning_checkins.sql:13-32`），且 allo 依赖的 fsrs-rs 6.6.1 **自带 `compute_parameters` 优化器**（`allo:Cargo.toml:221` + fsrs-6.6.1 `src/training.rs:261`），比 learnhub 借道 napi binding 成本更低。
3. **第二批是「会选题的调度」**：复习队列从 `ORDER BY due_at` 升级为「预测遗忘风险 R 分档升序 + 档内难度渐进」（learnhub #56，`learnhub-plugin:src/engine/index.ts:1094-1100`）；补上 CONTEXT.md 已承诺但代码缺失的「一卡一日一推进」门与 Forgot 5 秒回忆门；学习图推荐从「前置满足布尔门」升级为「R 衰减软闸 + 理由文案」（learnhub #54，`learnhub-plugin:src/engine/sessions.ts:114-123`）。R 的计算原语 allo 已具备（fsrs-rs `current_retrievability`，fsrs-6.6.1 `src/inference.rs:54`）。
4. **第三批是「学习者自主与账本」**：JOL 预测-校准、今天学它 pin、XP 时间账本（est×难度校准 settle）。均为读侧/旁路设计，不碰 canonical 调度（`learnhub-plugin:docs/adr/0009-arc-e-learner-output-boundary.md`），适合在调度层稳定后做。
5. **明确不迁**：vault 无 DB 架构、dsh 宿主接缝（16 工具/discuss 桥）、Anki 通道、Project/Habit/Receipts/Skill Entry（learnhub 自己也只有 ADR 无实现）、交互件体系（allo ADR-0002 已裁决暂缓）。P/U/D 三区见 `learnhub-plugin:docs/design/2026-09-learning-expansion-requirements.md`，属 learnhub 未来方向而非存量能力。

## 1. 两边基线速览

| | learnhub | allo |
|---|---|---|
| 形态 | dsh 插件，TS 引擎 `src/engine/`（~11.8k 行），Obsidian Vault 为唯一事实源、无数据库（`learnhub-plugin:README.md:24-42`） | `crates/backend/nomifun-learning/`（Rust），SQLite（sqlx），迁移族 015→050（`allo:docs/architecture/learning.zh.md:75-81`） |
| 调度 | ts-fsrs，日粒度（`enable_short_term=false`、无 fuzz，`learnhub-plugin:src/engine/srs.ts:24-41`） | fsrs crate 6（fsrs-rs），日粒度 + 本地 02:00 翻日 + 亚日重学步（`allo:crates/backend/nomifun-learning/src/scheduler.rs:99-160`） |
| 评分模型 | 题目即卡：作答对错直接推进该题 FSRS（`learnhub-plugin:README.md:116-126`） | 同构：review_item = 一道客观题一张卡（`allo:.../service/review.rs:83-85`） |
| 词汇 | `learnhub-plugin:CONTEXT.md`（38 词条） | 根目录 `CONTEXT.md` 自述「术语体系继承自 learnhub-plugin、两边词汇保持同源」；差异以 allo 为准（`allo:CONTEXT.md:3`） |
| 近期工作 | Arc A1-A3/E1-E5 已实施（reviewQueue R 排序、A1 自适应、软闸、JOL、pin 等） | 练习节逐题作答 + 节步进门禁、节质检门、Section 状态语义、单节重写（d88570c5e/5641b435d/888883e8f/ccf4fcca6，`allo:git log`） |

## 2. learnhub 概念清单

| # | 概念 | 一句话机制 | 状态（相对 allo） |
|---|---|---|---|
| 1 | 题目级 FSRS 卡 | 每题自带调度卡，作答对错直接推进；同节点多题独立推进 | allo 已有 |
| 2 | 一卡一日一推进门禁 | 当日至多一次真实推进，重复作答只记统计 | allo 声明了、未实现 |
| 3 | Review Queue 遗忘风险排序 | 跨课程扁平队列按 R 分档升序 + 档内难度渐进 | allo 没有（按 due 排序） |
| 4 | Forgot 申报 + 5 秒回忆门 | 不作答翻面按答错记；卡面展示满 5 秒才允许申报 | allo 有申报、无计时门 |
| 5 | 复习/练习评分语义分岔 | 练习流自动映射（对=Good/错=Again），复习流自评（Hard/Good/Easy） | allo 已有 |
| 6 | Mastery 派生（反饱和） | 0.7·min(1, S/2S_MASTER) + 0.3·练习 EMA，读侧派生不落盘 | allo 没有（落盘 EMA） |
| 7 | 逐次复习日志（review-log） | 真实推进才落一条：rating + rating_source + elapsed_days + S/D/R 前快照 | allo 没有 |
| 8 | True Retention 与预测对照 | 到期复习实际答对比例 vs FSRS 自预测 r_pred 分箱校准 | allo 没有 |
| 9 | FSRS 参数优化器 | 从复习日志重训 21 参数，评估不优不写回 | allo 没有（但 fsrs-rs 自带） |
| 10 | XP 时间账本 + settle 对账 | 1 XP≈1 分钟；节点定价 = est×FSRS 难度校准 k，完成时锁定 | allo 没有（打卡按张数） |
| 11 | 乱猜惩罚 | 耗时 <5s 且答错 → 负 XP、不推进调度 | allo 没有（attempts 无耗时） |
| 12 | Complexity Tier / bloom / difficulty | 难度×bloom×前置闭包折叠成低中高档，驱动生成预算与审计 | allo 部分（大纲自声明档位；节点无 difficulty/bloom） |
| 13 | 难度跳跃门禁（Jump/R11-R13） | 相邻 pre 边 \|Δdifficulty\|≥2 或深度跨步 → warn/verdict | allo 没有 |
| 14 | R 软闸（gateAdvice） | 前置 R<0.85 不拦新课但给「先复习前置 n 道到期题」建议 | allo 没有 |
| 15 | enc 成分技能边 + struggle 回补 | 带权前置技能边；作答挣扎时按 w×(1−R) 定向回补 | allo 没有（表结构已预留 kind/extra_json） |
| 16 | 内容诊断归因 R1/R2 | 单题 lapses≥3 或节级低正确率 → 定向单节重写 | allo 部分（有单节重写，无归因触发） |
| 17 | A1 作答期难度微调 | 单节点会话内按静态+FSRS 难度合量流式选档 | allo 没有 |
| 18 | JOL 预测-校准 | 翻面前三点预测，沉淀校准曲线；只展示不进账 | allo 没有 |
| 19 | Today Pin（今天学它） | 学习者置顶当日推荐榜首，次日失效，附理由 | allo 没有 |
| 20 | 难度带 + 教练（E5） | 会话难度偏好作带权偏好；长期选择分布给只读提醒 | allo 没有 |
| 21 | Learner Card / 讲给我听（E1/E2） | 学习者产出卡独立调度域；费曼讲解会话 | allo 没有 |
| 22 | 笔记源卡（C1） | 任意 vault 笔记注册为复习源，只出题不动文 | allo 没有（对应物应是知识库文档） |
| 23 | Anki 镜像（C2） | Anki 纯作答通道，vault 唯一调度者 | allo 没有 |
| 24 | Practice 节点/交互件 | type=practice 交互模拟节点 + 自包含 HTML 交互件 | allo 无（ADR-0002 裁决暂缓） |
| 25 | Project/Habit/Receipts/Skill Entry/执行事件 | P/U 区一等实体（姊妹实体、零 FSRS 语义、自报证据） | 两边都只有 ADR，learnhub 引擎未实现（grep `project/habit/receipt` 于 `src/engine/*.ts` 无命中） |
| 26 | vault 即事实源（无 DB） | YAML/frontmatter/jsonl 全部落盘，Missing/Broken 二分 | 不迁（架构冲突） |

逐条展开（机制、解决的学习科学问题、出处）：

1. **题目级 FSRS 卡**：把调度粒度从「概念/课时」降到「题」，每题独立 stability/difficulty/due/reps/lapses，同节点多题互不干扰；解决「概念级调度抹平题目难度差异」的问题。`learnhub-plugin:src/engine/question-bank.ts:47-51`（`fsrs` + `stats` 字段）；allo 同构已落地：`allo:crates/backend/nomifun-db/migrations/039_learning_review_question_level.sql:1-14`（迁移注释明说 "one item represents exactly one objective activity"）。
2. **一卡一日一推进**：`alreadyAdvanced = stats.last === day`（真实互动通道）与 `alreadyScheduledOn = stats.last === day || fsrs.last_review === day`（Anki 回放通道）两个守门收口在 advance 模块（`learnhub-plugin:docs/adr/0014-advance-module-single-guard.md`、`learnhub-plugin:src/engine/advance.ts:23-31`）；解决「同日反复作答刷爆调度证据/防刷」。
3. **Review Queue 遗忘风险排序**：到期卡按 R 分档升序（档宽 5 个百分点），档内难度由易到难，再 due/节点/题序稳定兜底；解决「due 相同的大量卡没有先后语义」——先刷最可能忘的（`learnhub-plugin:src/engine/index.ts:1000-1017` 注释与 `:1091-1100` 排序实现）。
4. **Forgot + 5 秒门**：申报「想不起来」不作答直接翻面，调度与统计按答错记、不奖励 XP；卡面展示满 5 秒才允许申报，防止以申报代替回忆（`learnhub-plugin:CONTEXT.md:31-33`「Forgot」；`learnhub-plugin:docs/adr/0006-review-vs-practice-rating-split.md`）；给不会的人一个不靠乱猜的诚实出口。
5. **复习/练习评分分岔**：练习流追求节奏（自动映射），复习流承担调度保真（自评三档 + 忘记门控）；刻意分岔、禁止未来重构「统一」（`learnhub-plugin:docs/adr/0006`）。allo 同构已存在：课程内作答只喂 mastery 不碰 FSRS（`allo:crates/backend/nomifun-learning/src/service/progress.rs:184-190`），复习流答错立即 Again、答对等自评（`allo:.../service/review.rs:1398-1481`）。
6. **Mastery 派生（反饱和）**：`masteryValue = 0.7·min(1, S/(2·S_MASTER)) + 0.3·练习EMA`，S_MASTER=30 天，复习把 S 推向 60 天才渐近满分，一次全对首学只到三成左右；它是唯一 canonical 的读侧派生值、不落盘，正确率立为独立词汇「作答正确率」只服务完成门禁（`learnhub-plugin:docs/adr/0007-mastery-canonical-derived-not-persisted.md`、`learnhub-plugin:src/engine/srs.ts:144-162`）；解决「正确率一次全对饱和到 1.0，表达不了长期记忆状态」。
7. **逐次复习日志**：只真实推进 FSRS 卡时落一条 9 字段记录（ts/course/node/qid/rating/rating_source/elapsed_days/stability_before/difficulty_before/r_pred），与作答流水、账本刻意分库；synthetic（完成学习时的调度初始化）不是真实作答，诚实度统计与训练数据一律排除（`learnhub-plugin:src/engine/types.ts:146-166`、`learnhub-plugin:docs/adr/0012`）。一条日志同时喂仪表盘与优化器。
8. **True Retention 与预测对照**：到期复习实际答对比例（每卡每天只取第一次推进；答错/Forgot 记失败），按 r_pred 分箱对照 FSRS 自身预测形成校准曲线与按时点遗忘曲线；只读诚实度度量，不进门禁不是 Mastery（`learnhub-plugin:CONTEXT.md:43-45`、`learnhub-plugin:src/engine/memory.ts:58-116`）。
9. **FSRS 参数优化器**：从复习日志 (rating, delta_t) 序列重训 21 参数；学习者级一套参数；门禁 = in-sample 新旧参对照不优不写回；真实日志 ≥400 条才训（`learnhub-plugin:docs/adr/0012`、`learnhub-plugin:src/engine/optimize.ts:1-19`）。learnhub 需借 `@open-spaced-repetition/binding`（napi）；allo 依赖的 fsrs-rs 6.6.1 原生带 `compute_parameters` + `evaluate`/`evaluate_with_time_series_splits`（fsrs-6.6.1 `src/training.rs:261`、`src/inference.rs:357,558`），迁移成本低一个量级。
10. **XP 时间账本**：1 XP≈1 分钟有效专注；过程信号（对 +w×d、错 0、乱猜 −1、同日重复 0）+ 完成时按 N₀×k settle 对账锁定定价，总账 = Σ已完成节点 N₀×k，与作答路径无关；ETA = 剩余预算÷每日目标，随证据越学越准（`learnhub-plugin:README.md:128-143`、`learnhub-plugin:src/engine/xp.ts:26-68`、`learnhub-plugin:src/engine/params.ts` XP_* 常量）。定位是「记录系统」，激励只是衍生需求（`learnhub-plugin:CONTEXT.md:51-53`）。
11. **乱猜惩罚**：作答耗时 <5s 且答错 → 负 XP 且不推进调度（含首答）；乱猜判定优先于同日重复判定防绕过（`learnhub-plugin:src/engine/xp.ts:22-34`、`learnhub-plugin:src/engine/params.ts` `XP_GUESS_SECONDS=5`）。需要前端提供每题作答耗时。
12. **Complexity Tier**：difficulty(1-5) × bloom(六层) × pre 闭包规模(p75) 折叠为低/中/高三档，锚定节段数区间/篇幅预算/题量/模型 effort；est 降级为容量上界不作分档信号（`learnhub-plugin:docs/adr/0005-est-not-primary-for-content-scope.md`、`learnhub-plugin:src/engine/complexity.ts:49-64,174-178`）。解决「est 是档位感常数，按它推导节数会让简单节点膨胀、复杂节点展不开」。allo 已有锚点表（`allo:crates/backend/nomifun-learning/src/models.rs:493-549`），但档位由大纲阶段自声明而非折叠（`allo:docs/adr/0002-lesson-sections-and-generation-pipeline.md:52`、`:104-105`「蓝图 schema 不加 difficulty / bloom 字段」）。
13. **难度跳跃门禁（Jump）**：相邻 pre 边 |Δdifficulty|≥2 或深度跨步 → 认知跨步候选（R13，需 verdict）/ R12 认知-时长失配（info）；图结构可标出候选、判定归属图的作者（`learnhub-plugin:CONTEXT.md:107-109`「Jump」、`learnhub-plugin:src/engine/quality.ts:21-44`、`learnhub-plugin:src/engine/audit.ts:209-223`）。
14. **R 软闸（gateAdvice）**：前置 R < R_GATE(0.85) 时新课候选不拦截，但推荐事件抬一档并附「前置 X 保持率已衰减（R=…），建议先复习它的 n 道到期题再学本节（仍可直接学）」，建议项携带直达复习入口（`learnhub-plugin:src/engine/sessions.ts:62-123`、`:364-391`；`learnhub-plugin:src/engine/params.ts` R_GATE=0.85）。解决「布尔前置门学完即永远通过，无视遗忘」。
15. **enc 成分技能边 + struggle 回补**：enc = 本节点练习真实调用的前置技能边（带权、须在 pre 传递闭包内）；作答正确率 <0.6 且 ≥3 次判定 struggle 后，按 `w×(1−R)` 降序给出「先回补成分技能」定向复习建议（`learnhub-plugin:docs/adr/0008-enc-scheduling-semantics-and-coverage.md`、`learnhub-plugin:src/engine/sessions.ts:82-151`）。
16. **内容诊断归因 R1/R2**：题目作答证据经 q.section 绑到节；R1 = 单题 lapses≥3，R2 = 自节版本锚点以来 (题,日) 去重 ≥4 次且正确率 <0.5；命中产出 diagnostic 事件并附「重写此节」直达动作（`learnhub-plugin:src/engine/attribution.ts:1-12`、`learnhub-plugin:src/engine/sessions.ts:393-410`）。解决「错题数据不回流内容质量」。
17. **A1 作答期难度微调**：单节点会话内每题难度 = 静态题面难度(1-3) 与 FSRS difficulty(1-10) 等权合成 0-1 标量，起点先验 = 节点 Mastery → 目标带，连对升档、错/忘降档流式选序；只作用于已调度题会话（`learnhub-plugin:src/engine/adaptive.ts:1-16`、`learnhub-plugin:src/engine/index.ts:1077-1084`）。
18. **JOL 预测-校准**：约 1/3 抽查率（优先到期边界 R∈[0.5,0.85]、难度中段、曾有偏差卡）在翻面前弹「会/不会/没把握」，配对沉淀校准曲线；只展示给学习者、不进 Mastery/XP/canonical（`learnhub-plugin:src/engine/jol.ts:1-77`、`learnhub-plugin:docs/adr/0009`）。
19. **Today Pin**：学习者把节点 pin 成当日推荐榜首（课程内置顶、跨课程仍按全局序），仅当日有效，附「你选了它」理由；Learner Output、只改读侧（`learnhub-plugin:src/engine/goals.ts:1-27`、`learnhub-plugin:src/engine/sessions.ts:410-447`）。
20. **难度带 + 教练**：会话开始可选简单/标准/挑战作带权偏好；长期选择分布喂只读教练提醒（总选简单→提示该刷难题了），无门禁（`learnhub-plugin:CONTEXT.md:147-149`、`learnhub-plugin:src/engine/coach.ts:1-17`）。
21. **Learner Card / 讲给我听**：学习者自注/挖空/讲解卡独立卡域（独立调度、自评语义、三不进：不进 Mastery/XP/canonical）；费曼讲解会话 AI 扮初学者追问、判词只入 E 档案（`learnhub-plugin:src/engine/learner-cards.ts:1-18`、`learnhub-plugin:src/engine/explain.ts:1-13`、`learnhub-plugin:docs/adr/0009`）。
22. **笔记源卡（C1）**：任意 vault 笔记/文件夹注册为复习源；引擎对用户笔记零写入，派生物（指纹 + 题库 + per-question FSRS）落镜像区；删除/改名 = Missing（卡池挂起）、正文编辑 = 内容漂移（提示重出/归档）、永不判 Broken（`learnhub-plugin:docs/adr/0010-c1-note-source-mirror-readonly.md`、`learnhub-plugin:src/engine/note-source.ts:1-21`）。
23. **Anki 镜像（C2）**：到期卡导出 AnkiConnect、作答事件回写；Anki 是纯作答通道，调度永远由 vault 的 ts-fsrs 重算，镜象可丢弃（`learnhub-plugin:docs/adr/0011-c2-anki-channel-bidirectional.md`、`learnhub-plugin:src/engine/anki.ts:1-15`）。
24. **Practice 节点/交互件**：`type: practice` 交互模拟节点不出题，交互成绩进练习证据 EMA（mastery 30% 权重、上限 0.3 的有意妥协，`learnhub-plugin:docs/adr/0001-practice-node-fsrs-compromise.md`）；交互件是自包含 HTML + widget-config + 完成上报契约（`learnhub-plugin:README.md:90-108`）。
25. **P/U 区实体**：Project（Course 姊妹实体、active/paused/delivered/archived、无 FSRS/mastery、里程碑对账，`learnhub-plugin:docs/adr/0015`）、Receipt（自报即证据、零 XP 不推卡，`learnhub-plugin:docs/adr/0016`）、Habit（执行意图+自动化曲线+宽容 streak、零 FSRS 语义，`learnhub-plugin:docs/adr/0017`）、Skill Entry/执行事件（FSRS 数学复用边界、1-4 评级入口契约，`learnhub-plugin:docs/adr/0018`）。**注意：这些在 learnhub 引擎 `src/engine/` 里均无实现**（对 project/habit/receipt/SkillEntry/ExecutionEvent 的 grep 无命中，2026-09-09 核查）。
26. **vault 即事实源**：无数据库，题目/节点状态/图/流水全 YAML+jsonl 落盘，Missing（合法空态）与 Broken（契约损坏须暴露）严格二分（`learnhub-plugin:README.md:24-42`、`learnhub-plugin:docs/adr/0004-user-data-must-not-degrade-silently.md`）。

## 3. 差距分析（对照 allo 现状）

### 3.0 四个必须先澄清的事实（避免把已有东西当缺口、把声明当实现）

- **allo 的 FSRS 调度粒度已是题目级**：迁移 039 把 review item 从概念级重建为「每道客观题一张卡」（`allo:crates/backend/nomifun-db/migrations/039_learning_review_question_level.sql:1-14`），课时完成时逐题种卡（`allo:crates/backend/nomifun-learning/src/service/progress.rs:603-627`），队列注释明说 "Each row is one review item = one question card"（`allo:.../service/review.rs:83-85`）。自建题也自带独立 FSRS 行（`allo:.../service/review.rs:816-878`）。
- **练习逐题作答与节步进门禁刚落地（本地 UI 态）**：commit ccf4fcca6「练习节逐题作答与节步进门禁」——练习节单题推进、已答只读回看、练习节绑定题全答前步进不能越节；门禁是本地 UI 态不持久化，与 ADR-0002「节不新增持久化完成状态」一致（`allo:docs/adr/0002-lesson-sections-and-generation-pipeline.md:111-127`；实现 `allo:ui/src/renderer/pages/learning/model.ts:118-140`）。d88570c5e 进一步拆出 LessonStudy 学习面与 model.ts 纯决策层。**没有** learnhub 的连对/struggle 机制（ADR-0002:119 明确不引入，错题靠反馈与 FSRS 复习兜底）。
- **mastery 未派生化**：allo 的 mastery 是**落盘的概念级 EMA**（`learning_mastery_states.mastery = 旧·0.7 + 新分·0.3`，`allo:.../service/progress.rs:561-601`；表定义 `allo:migrations/015_learning_engine.sql:242-261`），不是 learnhub 那种「FSRS 稳定度 + 练习证据」的读侧派生复合值；消费方是传统课程推荐阈值 0.8（`allo:.../service/course.rs:734`）与前端「全部掌握」判定（`allo:ui/src/renderer/pages/learning/model.ts:10-19`）。复习流答对/答错会经 score 映射回灌该 EMA（`allo:.../service/review.rs:1321-1344`），但 FSRS 稳定度分量不存在——即 learnhub ADR-0007 描述的「两口径并存」问题在 allo 表现为「单口径但不含记忆分量」。
- **XP/时间账本不存在**：allo 的每日目标是**复习张数**（默认 15 张，`allo:.../service/checkin.rs:79-98`），streak 从 checkin 锁定日派生（`allo:.../models.rs:1546-1553`）；`learning_attempts` 无耗时字段、无 XP 结算（`allo:migrations/015_learning_engine.sql:215-240`）。

### 3.1 逐概念对照表

| learnhub 概念 | allo 状态 | 证据 |
|---|---|---|
| 题目级 FSRS 卡 | **已有** | `allo:migrations/039_learning_review_question_level.sql:1-14`；`allo:.../review.rs:83-85` |
| 一卡一日一推进 | **声明了、未实现**。`rate_review`/`answer_review` 均无当日检查 | 声明：`allo:CONTEXT.md`「Review Queue……一题一天只推进一次调度」；代码 grep `same_day|review_day_number` 于 review.rs/progress.rs 零命中；`rate_review` 直接 `schedule_review`（`allo:.../review.rs:1285-1346`） |
| 复习/练习评分分岔 | **已有**（同构） | 课程内 attempt 只喂 mastery（`allo:.../progress.rs:184-190`）；复习流答错→Again、答对→等 `rate_review` 自评（`allo:.../review.rs:1403-1481,1285-1355`） |
| Forgot 申报 | **已有**；`answer_review(forgot=true)` 跳过判卷按 0 分记并自动 Again | `allo:.../review.rs:1403-1445`；自建题同款 `:1119-1185` |
| Forgot 5 秒回忆门 | **没有**。前端 Forgot 按钮仅 `disabled={locked}`，locked = 别的卡在忙 | `allo:ui/src/renderer/pages/learning/components/ReviewSession.tsx:204-218`、`:473`；allo CONTEXT.md 已写「受主动回忆门控——卡面展示一段时间后才允许申报」但无计时实现 |
| Review Queue R 排序 | **没有**。`ORDER BY r.due_at, r.review_item_id`，前端再按 due 稳定排 | `allo:.../review.rs:70`、`:205` |
| R 软闸 / 学习图推荐理由 | **没有**。学习图推荐 = 前置全部 satisfied 的就绪集按拓扑序取前 10；传统课程按 mastery<0.8 找薄弱前置 | `allo:.../learning_graph.rs:484-509`；`allo:.../course.rs:734-799` |
| Mastery 派生（反饱和） | **没有**（落盘 EMA，无记忆分量） | `allo:.../progress.rs:561-601`；`allo:migrations/015:242-261` |
| 逐次复习日志 | **没有**。`learning_review_events` 仅打卡聚合用 5 列 | `allo:migrations/042_learning_checkins.sql:13-32` |
| True Retention / 预测对照 / 遗忘曲线 | **没有**（无数据源） | 同上；`allo:routes.rs:78-89` 的 reviews/stats 面无 memory 端点 |
| FSRS 参数优化器 | **没有**，但参数盘已留口：`learning.fsrsParameters` 偏好 + `SchedulerSettings.parameters` 已生效；fsrs-rs 6.6.1 自带训练 API | `allo:.../checkin.rs:7-54`；`allo:scheduler.rs:20-28,112-115`；fsrs-6.6.1 `src/training.rs:261` |
| XP 时间账本 / 乱猜惩罚 | **没有**（打卡按张数） | `allo:.../checkin.rs:79-120`；`allo:migrations/015:215-240`（attempts 无 elapsed_s/xp） |
| Complexity Tier 生成锚点 | **部分有**：锚点表/文字预算/配比齐备，但档位由大纲自声明，非 difficulty×bloom×闭包折叠 | `allo:models.rs:493-549`；`allo:docs/adr/0002:44-53`（大纲自声明）、`:104-105`（蓝图不加 difficulty/bloom） |
| 节点 difficulty / bloom 字段 | **没有**。图节点仅 id/title/min/group/necessity/is_anchor；题目有 difficulty 1-3 | `allo:learning_graph/mod.rs:44-62`；`allo:models.rs:792-794` |
| 难度跳跃门禁（Jump） | **没有**（无 difficulty 字段即无从算 Δ） | `allo:learning_graph/audit.rs:14-76` 的检查清单无难度项 |
| enc 边 + struggle 回补 | **没有**，但表结构已预留：`learning_graph_prerequisites.kind`（beta 恒 'prerequisite'）+ `extra_json`（注释明说承载「权重/掌握度衰减系数等」） | `allo:migrations/048_learning_graph.sql:19-49` |
| 内容诊断 R1/R2 → 单节重写 | **部分有**：单节重写管线与前端入口已落地（`POST /lessons/{id}/sections/{key}/rewrite` + LessonStudy 重写入口），无作答证据归因触发 | `allo:docs/architecture/learning.zh.md:50-52`；`allo:git show d88570c5e` |
| JOL / Today Pin / 难度带 / Learner Card / 讲给我听 | **没有** | `allo:routes.rs` 全量路由清单无对应端点 |
| 笔记源卡（C1） | **没有**。最接近的是自建题（可挂概念）与知识库本体（nomifun-knowledge），无「注册文档为复习源」 | `allo:.../review.rs:816-878` |
| Anki 通道 | **没有** | — |
| 交互节 / practice 节点 | **裁决暂缓**（DB CHECK 不含 interactive，扩型需再迁移） | `allo:docs/adr/0002:84-87` |
| skipped 节点态 / 归档 / edit-pending / tags / 9 题型 / teaching_style | **已有** | `allo:migrations/048:55-88`、`043`、`044`、`036`、`049:60-63,145-146` |
| vault 无 DB / Missing-Broken 文件语义 | **不适用**（SQLite + CHECK 约束承担类似契约） | `allo:migrations/015` 全文件 CHECK 约束风格 |

## 4. 迁移项评估表

价值 = 对学习效果的提升潜力；成本构成 = 数据模型/迁移 SQL + 后端逻辑 + 前端 + i18n。迁移编号续接点：当前迁移末位为 050（`allo:migrations/050_learning_section_visual.sql`），新迁移从 051 起 append-only。

| 候选迁移项 | 价值 | 成本 | 依赖前置 | 建议阶段 |
|---|---|---|---|---|
| M1 逐次复习日志（051 新表 `learning_review_log`：9 字段对齐 learnhub ReviewRec，另加 review_day 复用 02:00 翻日） | 高（一切诚实度度量与个人化的数据地基；不落地则 M2/M3 永远缺数据） | S-M：SQL 一张 append-only 表；`rate_review`/`answer_review`/`rate_custom_review`/`answer_custom_review` 各加一次 INSERT；种卡处落 synthetic 行；前端零改动 | 无 | Phase 0 |
| M2 一卡一日一推进门 | 高（防刷 + 让 CONTEXT.md 声明成真；learnhub 还靠它保证 True Retention「每卡每天取第一条」口径成立） | S：`rate_review`/`answer_review`/custom 通道加 `last_reviewed_at ≥ 本复习日起点` 检查（`review_day_start_utc` 现成，`allo:scheduler.rs:43-53`）；补 i18n 提示 | 无（建议与 M1 同迁移） | Phase 0 |
| M3 True Retention + 预测对照 + 遗忘曲线 + 负载预报（统计端点 + 统计页面板） | 高（调度预测与实际一致的唯一诚实度仪表；learnhub 四面板全是纯聚合函数可直接照抄口径） | M：后端聚合模块（照 `learnhub-plugin:src/engine/memory.ts` 口径翻译成 Rust）；一条 `/api/learning/stats/memory` 路由；前端统计区 + zh/en i18n | M1（要数据） | Phase 1 |
| M4 复习队列 R 分档排序 | 高（learnhub 自评「Arc A 里唯一不用等任何前置、可直接立项」的首选项，`learnhub-plugin:docs/research/2026-09-big-directions.md:44-52`） | S-M：`due_reviews` 逐行用 `fsrs::current_retrievability` 算 R 后内存排序；响应带 r 字段；前端 ReviewSession 可选展示预测回忆率 + i18n | 无 | Phase 1 |
| M5 Forgot 5 秒回忆门 | 中-高（防「以申报代替回忆」，保住 Forgot 通道的证据价值；成本极低） | S：ReviewSession 卡面计时 + 按钮禁用态 + i18n（注意 `learning.json` 正在被修改，落地时续接） | 无 | Phase 1 |
| M6 Mastery 派生化（课时级读侧派生 = 0.7·min(1,S/60)+0.3·EMA，保留 `learning_mastery_states` 作 EMA 证据源，新增派生视图而非改语义） | 中-高（解决「全对首学即满 mastery」的失真；但涉及 0.8 阈值消费方与 UI 口径，需要一次领域对齐——learnhub 为此专门立 ADR-0007 并付出「LessonView 头部数字变低」的沟通成本） | M：后端派生函数 + lesson 详情/课程视图换算；前端 tooltip 语义更新 + i18n；不迁移表 | M1（要稳定度快照）或直接读 review_items.stability | Phase 1-2 |
| M7 FSRS 参数优化器（从 M1 日志重训 21 参，评估不优不写回，写回 `learning.fsrsParameters` 偏好） | 中-高（个人化上限；但冷启动要 ≥400 条真实日志，短期无感） | M：后端命令式端点（手动触发），fsrs-rs `compute_parameters` + `evaluate` 现成；无前端（或统计页加按钮 + i18n） | M1（≥400 条） | Phase 2（依赖数据积累，可提前合入代码） |
| M8 学习图节点 difficulty/bloom + Complexity Tier 折叠 + Jump/R12 审计 | 中（生成弹性与图质量；allo 现行「大纲自声明档位 + 质检门」刚稳定，动输入信号有回归风险，ADR-0002:104-105 明确「若护栏误判增多再考虑」） | M-L：052 迁移给 `learning_lessons`（或 graph_meta）加 difficulty/bloom 列；lg_* 工具契约与提示词、scope 契约、audit 新检查、前端图卡展示 + i18n 全链路 | 建议先观察现行档位门误判率 | Phase 2 |
| M9 R 软闸推荐（gateAdvice：前置 R 衰减 → 建议项 + 理由文案 + 直达复习入口） | 中-高（把「学完即永久通过」的布尔前置升级为遗忘感知；软闸不拦人，符合 allo 现有 skipped/回跳自由度） | M：学习图视图加 advice 字段（复用 M4 的 R 计算 + `learning_graph_prerequisites`）；前端 DAG/列表琥珀提示 + i18n | M4 | Phase 2 |
| M10 内容诊断归因 R1/R2 → 重写建议事件 | 中（错题数据回流内容质量；learnhub 证据是工程合理性而非学习科学效应） | M：后端从 `learning_attempts`/`learning_review_log` 聚合节级信号；附在课时/课程视图；前端LessonStudy 入口已就绪 | M1 更准（lapses 按日去重），但 attempts 也可起步 | Phase 2 |
| M11 JOL 预测-校准 | 中（学习科学纯度最高、成本最低的自主项；+8.9% RCT 证据见 `learnhub-plugin:docs/research/2026-09-big-directions.md:159`） | S-M：052/053 给 `learning_attempts`/`learning_review_log` 加 `predicted` 列（老记录 null 兼容）；复习流 UI 三点一档（可全局关）+ i18n；校准曲线挂 M3 统计页 | M1、M3 | Phase 3 |
| M12 XP 时间账本 + 乱猜惩罚 + streak/ETA | 中（动机与规划面；learnhub 定位「记录系统」非核心，且 XP 与调度解耦做得很干净可整体照搬） | M-L：attempts 加 elapsed_ms；新账本表或复用 checkins/journal 式流水（052/053 append-only）；课时完成 settle 对账；前端统计/目标/ETA + i18n（体量最大的一块前端） | 无硬依赖，但建议排在调度面之后 | Phase 3 |
| M13 Today Pin + 推荐理由 | 中（自主所有权，成本低） | S：新表或偏好 JSON；推荐接口加覆盖层；前端「今天学它」+ i18n | 建议在 M9 之后（理由文案同源） | Phase 3 |
| M14 难度带 + A1 会话内自适应 + 只读教练 | 中 | M：纯规则层可照抄 `adaptive.ts`/`coach.ts`；会话方持有状态；i18n | M4（难度标尺同源） | Phase 3 |
| M15 enc 边 + struggle 定向回补 | 中（差异化大，但 learnhub 自己承认全库 enc=0 的覆盖缺口，`learnhub-plugin:docs/adr/0008`） | L：kind='enc' 语义 + 权重落 `extra_json`（表已预留）；生成管线产出 enc 候选；struggle 判定与回补建议 | M8（要有 difficulty/bloom 生态）、M9 | Phase 3 / 远期 |
| M16 笔记源卡（C1 变体：知识库文档注册为复习源） | 中（扩大复习素材面；allo 已有 nomifun-knowledge，形态应是「从知识库文档出题并挂源」而非 vault 镜像） | L：源注册表 + 指纹/漂移语义 + 出题管线复用 | 无硬依赖 | 远期（单独立项） |
| M17 Learner Card / 讲给我听（E1/E2） | 中（生成效应/教学相长证据扎实；依赖 agent 会话面） | M-L：独立卡域表 + 会话编排 | 建议最后 | 远期 |

## 5. 推荐开发顺序

原则：先立「诚实的数据地基」（没有日志一切度量免谈），再做「消费数据的调度与统计」，再上「学习者自主与账本」；每阶段都是 append-only 迁移、可独立发布、后一阶段消费前一阶段的产出。

### Phase 0 —— 数据地基（一次迁移 051，几乎无 UI）

1. **051 迁移（append-only 新表）**：`learning_review_log`，列对齐 learnhub ReviewRec 9 字段 + `review_day`（`user_id, source('course'|'custom'), item_id, rating(1-4), rating_source('auto'|'self'|'synthetic'), elapsed_days, stability_before, difficulty_before, r_pred, review_day, created_at`；出处 `learnhub-plugin:src/engine/types.ts:149-166`）。不修改既有表。
2. **写入点收口**：`rate_review`/`answer_review`（auto/self）、custom 两通道、`seed_lesson_review_items` 落 synthetic 锚点行——对齐 learnhub「synthetic 不是真实作答、统计与训练一律排除」（`learnhub-plugin:docs/adr/0012`）。
3. **一卡一日门**（M2）：在复习流四处推进入口加当日检查，语义对齐 advance.ts 的 `alreadyAdvanced`（`learnhub-plugin:src/engine/advance.ts:23-26`）；同时兑现 `allo:CONTEXT.md` 已声明的「一题一天只推进一次调度」。
4. **attempts 加 `elapsed_ms`**（可与 051 同迁移 append 列）：为乱猜判定与未来 XP 铺路。

为什么先做：M3/M4/M7/M11 全部悬在 M1 上；门禁不动数据模型、风险最小；且这一步修复的是「CONTEXT.md 声明与代码不符」的现存裂缝。

### Phase 1 —— 调度质量（学习者可直接感知）

1. **R 分档排序**（M4）：`due_reviews` 内存排序升级（主键 R 档、次键难度、再 due/题序，照 `learnhub-plugin:src/engine/index.ts:1091-1100`）；R 用 fsrs-rs `current_retrievability`（fsrs-6.6.1 `src/inference.rs:54`）+ `scheduler.rs` 的 `days_elapsed_between`。
2. **Forgot 5 秒门**（M5）：前端计时 + i18n。
3. **记忆健康统计**（M3）：`/api/learning/stats/memory` 四面板（负载预报/状态分布/True Retention+预测对照/遗忘曲线），聚合口径逐条对照 `learnhub-plugin:src/engine/memory.ts`（尤其 `dueReviewFirstPushes` 的「每卡每天第一条 + 排除 synthetic + 排除首学」过滤，`memory.ts:61-75`）；前端统计区 + i18n。
4. **（可选）Mastery 派生化**（M6）：如做，先在本仓库立一条 ADR（learnhub 的 ADR-0007 是必读前例——它记录了口径收敛的代价与沟通成本）。

为什么这个顺序：M4/M5 不依赖 M1；M3 依赖 Phase 0 的日志开始积累；三者都不碰内容管线，与进行中的 i18n/前端改动冲突面最小。

### Phase 2 —— 图感知调度与内容回流

1. **R 软闸推荐**（M9）：学习图视图 recommended 附 advice（弱前置 + R + 到期题数 + 理由），软语义不拦截；文案口径对照 `learnhub-plugin:src/engine/sessions.ts:364-391`。
2. **节点 difficulty/bloom + 折叠档位 + Jump/R12 审计**（M8）：052 append 列；触发条件建议以 ADR-0002:104-105 的预设为准（护栏误判增多才动）。
3. **内容诊断 R1/R2**（M10）：节级作答聚合 → 课时视图 diagnostic 建议 → 接既有单节重写管线（`allo:docs/architecture/learning.zh.md:50-52`）。
4. **FSRS 参数优化器**（M7）：代码可先合入，手动触发端点 + 「评估不优不写回」门禁（照 `learnhub-plugin:src/engine/optimize.ts` 的 in-sample 对照协议）；生效依赖 Phase 0 日志积累 ≥400 条。

为什么：M9 依赖 M4 的 R 计算；M8 为 M15（enc）铺路；M7 需要数据，放中期正好。

### Phase 3 —— 学习者自主与账本

1. **JOL 预测-校准**（M11）：053 加 `predicted` 列；抽查逻辑照 `jol.ts`（默认 1/3、可关）。
2. **XP 时间账本**（M12）：定价 = est×k、完成 settle 锁定、乱猜负分、streak/ETA；账本表 append-only；前端体量最大，单独立项。
3. **Today Pin / 难度带 / 教练**（M13/M14）。
4. **enc + struggle 回补**（M15）远期起步。

解锁关系总结：M1 →（M3, M7, M10, M11）；M4 →（M9, M14）；M8 →（M13 顺带, M15）；M5/M2 独立可先行。风险最大的两块（M8 动生成契约、M12 动前端统计面）刻意排后，且 M8 有 ADR-0002 预设的观察期。

## 6. 明确不建议迁移的清单及理由

| 项 | 理由 |
|---|---|
| **vault 即事实源 / 无 DB 架构 / YAML 题库手编** | 与 allo 的 SQLite + sqlx + 迁移族根基冲突；allo 的对等物是「CHECK 约束 + append-only 迁移 + 管理界面」。learnhub 的 Missing/Broken 二分精神可通过「查询返回显式空态 vs 校验错误」继承，但文件事实源不迁（`learnhub-plugin:README.md:24-42`） |
| **dsh 宿主接缝**：16 个 `learnhub_*` agent 工具面、`learnhub:discuss` 宿主桥、`window.opener` 会话注入、cordis patch/link: 安装 | 纯 dsh 专有（`learnhub-plugin:README.md:13-22`）；allo 的对应接缝是 `nomifun-ai-agent` 的 `LiveLearningCourseSink`（`allo:docs/architecture/learning.zh.md:60-65`），形态已定 |
| **Anki 通道（C2）** | 价值真实但属生态扩张，需先有稳定的复习日志与「vault/DB 唯一调度者」纪律；learnhub 也是 Arc C 里最靠后的。建议远期、且只迁「纯作答通道」语义不迁镜象实现（`learnhub-plugin:docs/adr/0011`） |
| **Project / Habit / Receipts / Skill Entry / 执行事件（P/U 区）** | learnhub 引擎尚无实现（grep `src/engine/*.ts` 无 project/habit/receipt/SkillEntry/ExecutionEvent 命中，2026-09-09 核查），只有 4 条 2026-09-09 刚立的 ADR（0015-0018）与需求池（`learnhub-plugin:docs/design/2026-09-learning-expansion-requirements.md` §3 P/U 区）；没有可迁的「实现」，只有可参考的「领域裁决」。等 learnhub 落地后再评估 |
| **交互件体系（widget-config / LEARNHUB_COMPLETE / vendored three+katex）** | allo ADR-0002 已裁决「交互节暂缓」，理由依然成立：需先设计 allo 自己的交互件契约（`allo:docs/adr/0002:84-87`） |
| **笔记源 C1 的 vault 镜像形态** | 形态绑死 Obsidian；allo 若做，应基于 nomifun-knowledge 做「知识库文档注册为复习源」，只借「零写入用户数据 + 指纹漂移检测 + Missing 不判损坏」的语义（`learnhub-plugin:docs/adr/0010`） |
| **图谱生成期的批次提案自动 apply / 健康分 0-100 / Scale Floor** | allo 学习图已走「agent 循环 + lg_* 工具 + 确定性审计门禁 + 发布前 LLM 终审」的等价且更严格的路径（`allo:docs/architecture/learning.zh.md:36-47`、`allo:learning_graph/audit.rs:1-76`）；learnhub ADR-0003 是对其自身流程的放权，不构成 allo 的增量 |
| **learnhub 面板 UI 反向移植** | 该面板本就「组件移植自 allo learning 模块」（`learnhub-plugin:README.md:16`），迁回属循环移植；只迁上面各机制背后的**交互语义**（R 展示、四面板、5 秒门），不搬代码 |
| **Python 时代遗留参数**（REVIEW_RATIO/DAILY_CAPACITY/OVERLOAD_FACTOR/SOFT_RESTART_GAP 等，`learnhub-plugin:src/engine/params.ts`） | 是否仍被 TS 引擎消费未验证；即使有，也是 learnhub 单机任务包模型的产物，与 allo 的队列/打卡模型不对应 |

## 7. 证据薄弱 / 未验证之处

1. **allo CONTEXT.md 的两处声明与代码不符**（「一题一天只推进一次调度」「Forgot 受主动回忆门控」）：我的依据是 grep 未见实现 + ReviewSession 组件无计时逻辑；未跑行为测试逐路径确认（例如是否存在其他中间层拦截）。
2. **learnhub Project/Habit/Receipts「无实现」结论**基于文件名与全文本 grep 无命中；未逐一阅读 `src/index.ts`（2942 行门面）全部工具注册，不排除存在未命名一致的入口（低概率，标注未验证）。
3. **fsrs-rs 6.6.1 `compute_parameters` 的调用面**（输入形状是否要求 FSRSItem 前缀展开、win 平台可用性）只核对了导出符号与文档注释，未实际编译验证；learnhub 侧的经验（binding 的「每卡每天第一条、首复习 delta_t=0」契约，`learnhub-plugin:src/engine/optimize.ts:9-14`）在 Rust 侧是否同构需要 spike。
4. **迁移编号续接点**：撰写时工作区仅 learning.json i18n 三文件有未提交改动（`git status`），未发现 >050 的未落盘迁移草稿；若分支上他人并行加迁移，编号需在实施时重新对齐。
5. learnhub 引用行号以 2026-09-09 工作区快照为准，learnhub 仓库亦在活跃开发中，行号会漂移。

## 8. 主要出处索引

- learnhub：`README.md`（数据主权 24-42 / 工具面 44-52 / 面板 54-63 / 评分模型 116-126 / XP 预算制 128-143）；`CONTEXT.md` 全文；`docs/adr/0001`～`0018`；`docs/design/2026-09-learning-expansion-requirements.md`；`docs/research/2026-09-big-directions.md`（§3 Arc A / §7 Arc E / §8 决策先行 / §9 反模式）；`docs/research/2026-09-node-content-quality.md`；`src/engine/{advance,srs,xp,params,sessions,index,memory,complexity,jol,adaptive,attribution,coach,goals,explain,optimize,note-source,anki,learner-cards,question-bank,grading,quality,audit,types}.ts`
- allo：`CONTEXT.md`；`docs/architecture/learning.zh.md`；`docs/adr/0002-lesson-sections-and-generation-pipeline.md`（含两轮追加决策）；`crates/backend/nomifun-learning/src/{scheduler.rs,models.rs,routes.rs}` 与 `src/service/{review,progress,checkin,course,learning_graph}.rs`；`crates/backend/nomifun-db/migrations/{015,029,036,037,039,040,042,043,044,048,049,050}_*.sql`；`ui/src/renderer/pages/learning/{model.ts,components/ReviewSession.tsx}`；`Cargo.toml:221`（fsrs = "6"）；git 提交 d88570c5e / 5641b435d / 888883e8f / ccf4fcca6

## 9. 拷问后修订（2026-09-09，grill 会话定案）

Phase 0/1 逐项拷问后定案，以下与上文冲突处**以本节为准**：

- **M2「一卡一日门」修订为「到期门」**：allo 调度器保留亚日重学步（`scheduler.rs`：Again 卡同日数分钟后重新到期，最短 1 分钟），字面的「一卡一日一推进」会拦下合法重学推进、且被拦的卡停在过期 due 上无限重现。定案：**推进仅当 `due_at ≤ now`**；未到期的重复作答（过期 UI/双击/双端并发）照记 attempt，不推进、不落日志。门不再依赖日志查询；日志退回纯统计职责。learnhub 之所以能用字面日门，因其禁用了短期调度（`enable_short_term=false`）——这是两边的正式语义分叉，词汇表「差异处以本文件为准」兜底。`CONTEXT.md` Review Queue 词条已同步改写。
- **M1 定案**：仅真实推进落日志（9 字段 + `review_day` 学习日 02:00 口径）；Forgot 记 rating=1 + auto；synthetic 种卡落日志、标 synthetic、不构成推进；051 单迁移打包 `learning_attempts.elapsed_ms`（可空；前端简单墙钟，可见性不感知）。
- **M4 定案**：照抄 learnhub 规格——R 五百分点分桶升序、档内难度升序（先易后难）、`due_at`→`review_item_id` 兜底；`due_reviews` 响应逐行带 `r`；前端卡片披露预测回忆率（zh/en i18n）。decay 参数来源实施前先做 fsrs-rs 编译 spike。
- **M5 定案**：Forgot 申报门固定 5 秒常量，仅复习流按钮，无设置项。
- **M3 定案**：`/api/learning/stats/memory` 四面板（负载预报/状态分布/True Retention+预测对照/遗忘曲线）一次到位；前端面板与 `CheckinPanel` 并列。
- **M6 已立 [ADR-0005](0005-mastery-derived-from-memory-and-practice.md)**：派生口径锁定（0.7·min(1, S/60) + 0.3·练习EMA，正确率独立成词，0.8 阈值消费方同步切换），实现排 Phase 2。
- 新增词汇（`CONTEXT.md`）：Review Log / Review Day / Synthetic Push / True Retention。
- **实施期修订（到期门的最终形态）**：门为并集语义——`due_at ≤ now` 放行；**当日尚无推进的未到期卡放行一次**（课程复习会话 `due_only=false` 会出示未到期卡，字面「仅到期可推」会把该既有功能全部拦死）；只有「未到期且当日已推进」的过期重复被拦。因此门在提前复习分支**仍需查询复习日志**判断「当日已推进」，本节上文「门不再依赖日志查询」一句作废。落地实现：`review.rs` 的 `push_allowed`；对应测试 `due_gate_admits_first_same_day_early_review` 等 5 项。
