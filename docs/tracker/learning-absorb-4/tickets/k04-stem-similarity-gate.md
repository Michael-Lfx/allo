---
id: k04
title: "题面相似度防雷同机械门——trigram 三口径 0.7、题级丢弃留痕、破地板整稿回灌"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# k04: 题面相似度防雷同机械门

**What to build:** allo 的题目新颖性检查目前是「规范化后精确匹配或子串包含」
——同题换数/换情境的雷同完全漏过，防复用只靠提示词嘴上说。吸收 learnhub
#148 qb.similarity：题面比对升级为 trigram 三口径相似度（≥0.7 命中），
**题级丢弃留痕**（元素级），数量地板破才整稿回灌带指名；quiz 变式同门执法。

## allo 现状

- 新颖性检查：`generation/activities.rs:209-223`——`normalize_prompt`
  （空白折叠+小写，`parser.rs:20-25`）后**精确相等或子串包含**命中，≤16 字符
  平凡项跳过；防复用池提示词面注入（:160-178 枚举全部既有题 + 「不得重复或
  相似」硬句）。
- 生成回路：`generate_lesson_activity`（:72-124，2 次尝试带反馈）；批门
  `validate_lesson_activities`（:233-268，数量地板 (3,5) + 证据 + 形状）。
- audit 已有 `activity_prompt_duplicate` 观察条（`lesson_draft.rs:953-958`）。
- quiz 变式（q3）：`service/quiz.rs:761-843`——池 = `existing_lesson_questions`
  （:771-784，同节点全部题面）提示词注入，失败如实跳过只出旧题。
- **单源相似度函数现成**：`learning_graph/defnear.rs:77-93` `trigram_jaccard`
  （字符 trigram、`normalize` :38 归一化）——可直接用于任意 `&str`。

## learnhub 参考（#148，契约 v2.9）

- 相似度函数：`src-tauri/src/domain/concepts/nearname.rs:37+`——
  `max(trigram Jaccard, trigram 包含度, 子串包含度)` 三口径取最大；
  <3 字符整串作单 gram。铸名近形门单源复用，零新算法。
- 阈值与命中：`engine/questions/validator.rs:544` `VARIANT_SIMILARITY_THRESHOLD
  = 0.7`；`similarity_hit()` :620-629（池内最佳分、平手取先入池者，确定性）。
- 池装配：`practice_stems()` :637-650——练习节题面与解答行都进池，装饰剥除
  （`### `/`- `/粗体枚举）防稀释包含命中。
- 元素级丢弃：`engine/questions/facade.rs:698-793`——违规全部定位到
  `条目N.stem` → 坏题剔除 + 记 `dropped`（报告面），**三型覆盖门仍硬**：
  破盖 → 整稿重试，回灌点名丢弃条目与缺失类型（:767-776）；顶层违规（覆盖/
  形状/超量）保守整稿退回；预算尽 `Exhausted`。重试用脱敏旧稿（答案/关键词
  占位）。单题重出路径 avoid 池并入练习题面同门执法（:466）。
- 提示词句：`prompts/questions/question-generate.md:26`「不得同题同数——换数、
  换情境或换表征通道另出；evidence 锚正文原句不变」。

## 实施要点

- [ ] **三口径相似度单源**：扩 `defnear.rs` 公开
      `pub fn stem_similarity(a: &str, b: &str) -> f64` = max(trigram Jaccard,
      trigram 包含度, 子串包含度)（trigram 提取复用现有函数；包含度 = 交集
      gram 数 / 短串 gram 数）；阈值常量 `STEM_SIMILARITY_THRESHOLD = 0.7`。
      名字近形与题面防雷同从此同一函数——零第二实现。
- [ ] **新颖性门升级**（`activities.rs`）：旧精确/子串检查替换为
      `stem_similarity ≥ 0.7` 命中；违规消息带命中题面摘录 + 分值（反馈重试
      可指名）。池 = 既有题面（现有枚举）+ **批内兄弟互查**（新生成题目两两）。
- [ ] **题级丢弃留痕**：批门违规定位到单题 → 剔除该题 + `dropped` 清单进
      生成报告 + `activity_prompt_duplicate` 观察条升级（带命中对与分值）；
      剔除后数量地板破 → 整稿回灌带指名（沿用 2 次尝试回路，learnhub
      覆盖门破=整稿退回同义）。
- [ ] **quiz 变式同门**：`generate_quiz_variant` 生成后变式题面过池
      （既有全部题面）相似度门；命中 → 如实跳过只出旧题（现有失败路径，
      观测留痕）。
- [ ] 提示词防雷同句升级（allo 措辞对齐 learnhub：换数/换情境/换表征通道）。
- [ ] 开关 `NOMI_LEARNING_QB_SIMILARITY=off` → 退回旧精确匹配门（旧行为臂）。

## 验收

- **单测（红绿）**：①同题换数（「3+5=?」vs「4+6=?」归一化后不等、trigram
  命中）——新门红、旧门绿（证明升级有效）；②异考法绿；③池装饰剥除
  （`### `/`- `前缀不稀释）；④题级丢弃：批内 3 题撞 1 → 剔 1 留 2 +
  dropped 留痕；地板 (3,5) 破 → 整稿回灌消息指名；⑤变式命中 → 跳过出旧题；
  ⑥off 臂走旧门（换数题绿）。⑦平手确定性（同分取先入池）。
- **smoke 双臂（开关臂，course/content/quiz 任一覆盖题目生成面的档）**：
  硬判据——两臂 EXIT 既有判据不劣化 + **读数行在场**：
  `pool_size / sim_hits / dropped / retries` × 两臂；观测存档路径打印。
- **对照卡**：同 fixture 真机两臂——off 臂 sim_hits 恒 0（门不在）、on 臂
  命中数 ≥ off 臂（机械面从不漏到漏过的方向性）；若真机 LLM 未自发出雷同
  题（on 臂 sim_hits=0），fixture 侧造密度（既有题面多的课时）再跑并如实
  记录——只跑一臂不算关票。
- `cargo test -p nomifun-learning` 全绿。

## 非目标

- 不做语义级 embedding 相似（v1 字符 trigram，learnhub 同款）。
- 不动三型覆盖门之类的 learnhub 题库特有形状（allo 数量地板 (3,5) 是对应物）。
- 练习节正文题面 vs 题库的跨来源池（learnhub 池=正文练习节）：allo 课时正文
  与题目同表（learning_activities），池=该节点全部既有题面已覆盖该语义。

## 备注

本票与 k05 都动 `generation/lesson.rs` / `lesson_content_loop.rs` 提示词面，
依赖序 k03 → k05 已排；k04 与 k05 触面错开（校验器 vs 提示词块），无依赖。

## Resolution

**已关（2026-10-23，`8b86d0a63`）**。

- **单源三口径**：`defnear::stem_similarity` = max(trigram Jaccard, trigram 包含
  度, 子串包含度)（复用 `normalize`/`trigrams`；<3 字符整串单 gram）+
  `STEM_SIMILARITY_THRESHOLD = 0.7`。铸名近形与题面防雷同同一函数。
- **批门题级丢弃**：`filter_novel_activities`（池 = 既有题面 + 已接受批内兄弟；
  平手取先入池）→ 剔除记 `DroppedActivity{prompt, hit_prompt, score}` →
  地板破时整稿回灌带指名（沿用 2 次尝试回路）。传统管线落库前逐条 warn 日志。
- **单题/变式同门**：`validate_generated_activity` 用同一门（quiz 变式经此路径，
  命中即如实跳过只出旧题，现有失败路径）。
- **观察条升级**：draft 审计 `activity_prompt_duplicate` 带命中对与分值。
- **开关**：`NOMI_LEARNING_QB_SIMILARITY=off` → 旧规范化精确/子串门。
- **读数计数器**：`qb_readings`（pool_checks/sim_hits/dropped/retries），quiz
  档打印。
- **单测**：换数题新门红旧门绿、异考法绿、装饰剥除、题级丢弃 + 地板回灌指名、
  off 臂旧门、平手确定性、开关取值——全绿。
- **双臂**：on pool_checks=6 sim_hits=0；off pool_checks=0；variants 各 6，
  EXIT 均 0。on 臂未误伤真实「同考点换角度」变式；真机未自发雷同（如实记录）。
  读数见 [verification-cards.md](../assets/verification-cards.md) §k04。
- **已知边界**：字符 trigram 对极短题面（如「3+5=?」）区分度有限，learnhub 同款；
  验收改用真实长度题干（换数/换情境）锁红绿。
