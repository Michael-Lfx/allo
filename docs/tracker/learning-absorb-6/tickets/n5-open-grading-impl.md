---
id: n5
title: "开放题判分重写实施——judge 评审执行器 + explanation 判底 + 重试环/超时/脱敏 + smoke grade 档"
type: task
status: closed
assignee:
blocked_by: [n4]
part_of: ../map.md
---

# n5: 开放题判分重写实施（A 线）

**What to build:** 按学习路径把判分链升级为 learnhub #181/#182 形态（#184
生成面部分随 D1(b) 后置）。**n4 已关（2026-10-28 九裁决全按推荐落定，见
n4 Resolution）——本票即定案配置，可直接开工。**

## allo 现状（file:line 为准）

- 判分链：`submit_attempt`（service/progress.rs:105-195）→
  `grade_open_answer`（:206-247，单次裸调 `completer.complete`）→
  `parse_open_grading`（:323-333，`{score,feedback}` 两键宽容解析）→
  INSERT learning_attempts（:171-186）→ `passed = score >= 0.6`（:167）。
- 提示词：OPEN_QUESTION_GRADING_SYSTEM（:274-290，0–10 ≥6 及格）/
  REFLECTION_GRADING_SYSTEM（:254-269，0–1）。
- 判底缺席：build_open_grading_prompt（:296-316）只吃 题面+作答+
  lesson_title/summary；`config.explanation` 非空必填（models.rs:928）从未
  进评审。
- 三重脆弱：无超时、无瞬态重试、no-thinking 非 json 通道
  （LiveLearningCompleter.complete learning_completer.rs:97-121；带思考
  json 通道现成 complete_json_with_thinking generation/completer.rs:124-186）。
- 消费面：settle_node_completed（xp.rs:33-108，分子分母不对称在 :70-95）；
  quiz/review 客观题 only（quiz.rs:204 / progress.rs:519-539）——零交界。
- 前端：AttemptResult（ui/src/renderer/pages/learning/types.ts:314）+
  LessonStudy.tsx:96-104 渲染；无 missing_points 承载。
- 观测：ArchivingCompleter（nomifun-ai-agent/archiving_completer.rs）冒烟
  专用全文落盘零脱敏；生产零判分观测。

## learnhub 参考（Reference 表——实码先读铁律）

| 件 | learnhub 路径 | 取什么 |
| --- | --- | --- |
| ADR-0037 | `docs/adr/0037-open-question-judge-grading.md` | 置位权移交公理、七裁决原文（照抄合法/同义容错/空答抄题离题判错） |
| judge 执行器 | `engine/questions/judge.rs` 全文（bd31277） | 五槽 prompt（题面/判底/要点/作答/重试回灌）、重试环 N=2 带原因回灌（「上一轮输出未过校验：{reason}」）、90s 超时（线程+recv_timeout、超时只弃果）、JudgeExhausted 绝不静默 |
| schema | `infra/json_contract.rs schema_question_grade` | 连续分适配版：{score,feedback,missing_points} 全必填、score≥及格 ⇔ missing_points 空（n4 D3） |
| 脱敏归档 | judge.rs L251-323 | redact_prompt/redact_response 占位替换、归档失败降级不阻断 |
| 模型档 | judge.rs PromptSpec | 思考开/温度 0（n4 D7：complete_json_with_thinking + 专项预算） |
| smoke-grade | `src-tauri/src/bin/smoke-grade.rs` | 四判据：病历判对+点评非空 / 真误区判错+缺失非空 / 离题判错 / 断 LLM 如实上抛；+ 脱敏回读 |
| #184 条款 | `prompts/questions/{question-generate,question-regenerate,quiz-variant}.md` diff | 仅当 n4 D1(b) 启动：要点 3–6 条可判定/同义容错/避开公式段；answer=评审底稿 |

## 实施要点（推荐配置骨架）

- [ ] **判底进评审器**（n4 D1(a)）：build_open_grading_prompt 增
      explanation 槽（存量为空的历史行容错——Option + 「(无：以课时上下文
      为准)」装配）；OPEN_QUESTION/REFLECTION 两 system 随批改写
      （ADR-0037 口径域中立移植：作答相对判底的语义正确性、同义改写合法、
      照抄模仿合法、空答/抄题/离题判错；分数形态按 n4 D3）。
- [ ] **judge 评审执行器**（learning 侧新模块）：重试环 N=2 带原因回灌 +
      90s 显式超时 + schema 校验（判对-缺失互斥连续分适配）+ JudgeExhausted
      如实上抛；通道 = complete_json_with_thinking（专项预算票内实测定），
      thinking 不可用降级普通 json 通道 + 观测标记。
- [ ] **脱敏归档器**：冒烟档占位替换（判底/作答），feedback 原文保留；
      归档失败降级不阻断判分。
- [ ] **AttemptResult 扩展**（n4 D3）：增 missing_points（serde default 容
      旧客户端）；feedback 文本通道不动（前端 Markdown 管线复用 n1 口径）；
      前端「缺这些要点」展示 + i18n 双语。
- [ ] **XP 对称修复**（n4 D6）：answered/passed 子查询过滤 AI-graded
      题型，与 total_questions 对称；测试钉住；对照卡记行为变化。
- [ ] **smoke 新档 `grade`**：布景（含 explanation 的开放题 + reflection
      各一）；四判据 + 脱敏回读进 EXIT；断 LLM 臂如实非零。
- [ ] **双臂验证**：机制面开关臂（评审器新链 off = 旧 grade_open_answer
      路径，同二进制）；效果面对照卡 = 同一批作答 off/on 评分并排 +
      thinking on/off 双跑（n4 D7）+ 人读判分合理性清单。

## 验收

- `cargo test -p nomifun-learning` 全绿（新单测：schema 互斥红绿样、重试
  回灌序列、超时路径、脱敏占位、XP 对称、存量空 explanation 容错）。
- smoke grade EXIT=0（有凭据真机）；观测档判底原文零命中（回读断言）。
- 对照卡两臂并排落 Resolution；**只跑一臂不算关票**。
- typecheck / i18n / workspace check 净。

## 非目标

- D1(b) 参考答案+要点契约反转（后置票，触发条件见 n4 Resolution）。
- 结构化 unavailable 三态与前端重试 UI（后置，触发条件见 n4 Resolution）。
- HeldSession 式 verdict 缓存 / 重复提交折叠（伪需求/雾区）。
- EvidenceTier 证据档（n4 D5 显式豁免）。
- quiz 线判分（客观题规则面不动）、复习线判分（客观 only 不动）。
- 迁移（推荐配置零迁移；missing_points 走 API 层不落新列——若 n4 裁决要求
  落库，迁移从 073 起回 map 对账）。

## Resolution（2026-10-29 关票）

- 实施 = `28bfbabd1`（judge 执行器 + explanation 判底 + 重试环/超时/thinking
  16384 + 脱敏归档 + smoke grade 档）+ `0e8a5c26d`（AttemptResult
  .missing_points 前端承载 + i18n 双语），learning 644；评审修复批内 thinking
  降级收窄（仅 Conflict「structured output unavailable」信号、超时/BadGateway
  不降级、一次定臂粘滞——三单测钉住）。波次经 e08e65d60 并入 feat/learn0928。
- 实现中裁决（如实记录）：①互斥校验取双向（判错分必须点名缺失——按票面
  「⇔」字面，learnhub 原版单向）；②thinking 预算 16384（真机未跑前的取值按
  learnhub 实证算术，双跑校准点见对照卡）；③判底槽经 build_open_grading_prompt
  统一装配，旧臂 (None,None) 逐字节保形保证对照臂纯净；④脱敏注册走
  ArchivingCompleter::register_redaction（生产不挂零开销）。
- **双臂真机全绿**（详见 [assets/verification-cards.md](../assets/verification-cards.md)）：
  grade on/off EXIT=0；on 臂病历判对 0.88（explanation 判底生效）、真误区
  判错 0.10+缺失 5 条、离题 0.00、断 LLM 链路 WARN 全序列实证（降级→回灌重试
  →JudgeExhausted fail-loud）、脱敏回读零泄漏；off 臂旧链方向全同向。
  真机暴露布景 position 撞 UNIQUE（02e502309）与判据②臂盲（8335d719c）已修。
