# Map: main 同步收尾杂项——i18n 标点残留、死导出、零 panic 红线、outline 终态契约、测试隔离竞态、仓库卫生（s1–s7）

<!-- wayfinder:map -->
<!-- 状态：立图（2026-11-11）。来源 = 并回 main 前全面审计 + origin/main 并入
      feat/learn0928 后的合并树验证（2026-11-11）。同步合并提交 b79af8f4f（解
      5 处冲突 + tempfile 重复键）+ 490a14c09（lockfile 补齐）已推；
      CI（Ubuntu）四作业首趟全绿，PR #350 翻 MERGEABLE/CLEAN。审计遗留项与
      两族测试隔离缺陷全部立案，本波一次收口后学习侧即无已知未立票项。 -->
<!-- 基线：feat/learn0928 尖端 490a14c09（main 同步后）。 -->
<!-- 状态更新：收图（2026-11-11）。s1–s7 施工完毕、验证全绿后闭票；集成 =
      feat/learning-main-sync-followups 并回 feat/learn0928。两票验证协议：
      s5 以本机并行全套连跑三次为判据（修复前 3/3 必败），s6 以全套连跑两次
      + 单跑为判据（1/2 命中的偶发无法确定性复现）。 -->

## 用户裁决（2026-11-11）

「都立票然后处理一下」——审计报告挂出的全部遗留项一次立票施工，无逐票裁决。
范围以审计报告「还挂着的」清单为准：学习内小项四件 + 测试隔离缺陷两族 +
仓库卫生一件。

## 发现 → 七票映射

| 发现 | 分类 | 归票 |
| --- | --- | --- |
| ① 8 处 `join('、')` + 1 处全角括号模板，en-US 用户可见中文标点 | i18n 红线违规 | **s1** |
| ② 5 个零引用导出（StructureEmpty / sectionStatusLabel / COURSE_PHASES / LESSON_PHASES / isKnownPhase） | 死代码 | **s2** |
| ③ `learning_graph.rs:3459` `.expect("just pushed")`，空名时 push 静默早退后 find 落空 | 生产路径 panic 红线（m1 同族） | **s3** |
| ④ v2 outline 任务成功也在注册表落 failed（守卫零 finish + Drop 兜底 failed；course_id = 会话 id 拉详情必 404） | 状态机缺陷（create-progress-ux 后置项 #1 正式立票） | **s4** |
| ⑤ nomi-agent profiler 全局计数器被 reset 测试并行清零，Windows 全套并行 3/3 必败 | 测试隔离（CI 卡点套件，Linux 窗口窄暂绿） | **s5** |
| ⑥ learning structure_gen 日志捕获断言偶发挂（tracing callsite interest 进程级缓存） | 测试隔离（非 CI 卡点） | **s6** |
| ⑦ `.zcode/plans/plan-sess_73a4b952…` 已提交入分支，`.gitignore` 无 `.zcode/` 规则 | 仓库卫生 | **s7** |

## 影响面核查关键事实（2026-11-11 实码核实，票面直接引用）

- **s1 证据**：`LearningGraphWorkspace.tsx:410/411/1155/1213-1217`、
  `structure/GateCoverage.tsx:62/153/193`、`structure/StructureGraphTab.tsx:142`
  共 8 处 `join('、')`；:1215 别名列模板 `` `${alias.name}（${alias.definition}）` ``
  全角括号。仓内已有键 `learning.listSeparator`（zh「、」/ en ", "），4 处在用
  （CourseGenerationPill.tsx:63、CourseWorkspace.tsx:215/235、
  CourseGenerationProgress.tsx:159）；`multiSourceDisclosure.test.ts:39` 已用
  测试禁止该写法。
- **s3 证据**：`push_nomination`（learning_graph.rs:3428-3443）对空名/重名
  静默早退，随后 `find(...).expect("just pushed")`（:3459-3462）唯一炸点是
  `new_aliases[].name` 空白；上游 `growth_contract.rs:374-386` parse 已 trim+
  拒空（红条 `ng.aliases.required`），目前不可达；模块自立零 panic 红线见
  `name_adjudication.rs:156-158` 注释；m1 修复先例 `structure_ops.rs:640-647`
  Err 化 + 红绿样测试（:1075-1130）。
- **s4 证据**：`generate.rs:132-135` 守卫 `_task` 全文件零 finish 调用；
  `generation_tasks.rs:217-229` Drop 对未收尾守卫写
  `STATE_FAILED "任务中断（未收尾即退出）"`；注册表 finish 首写即定
  （:508-510 非 running 直接 return）。注册表键 course 段 = 会话 id
  （generate.rs:129-134 注释），前端恢复态翻 done 后按
  `row.course_id` 拉 `fetchCourse`（useCourseCreation.ts:126-141），会话 id
  必 404 → 收敛 failed——课程实际建成。观测行（learning-tasks.jsonl）同样
  把成功 outline 记成 failed。注册表函数面（begin/pre_register/begin_registered/
  set_step/finish/cancel/cancel_running/snapshot/running_newest）无任何
  补挂真实 course_id 的 API。
- **s5 证据**：失败断言 `ledger.rs:456`「every probe must be reported to the
  profiler」——`before` 读数与最终断言之间起两次 `git` 子进程，期间
  `profiler.rs:243` `reset_zeroes_every_counter` 并行把全局 `COUNTERS`
  清零即挂。本机（Windows）并行 3/3 必败、`--test-threads=1` 959 全过、
  单跑全过；`horizon/ledger.rs` 与 `profiler.rs` 分支未碰、与 main 逐字节
  相同（既有缺陷）；Linux CI 绿（进程起得快窗口窄）。
- **s6 证据**：失败断言 `structure_gen.rs:669`（`captured.contains("structure
  generation warnings (non-rejecting)")`）——失败趟里生成行为断言（过门/
  删键/calls==2）全过，仅日志缓冲缺行。全套 1/2 命中、单跑必绿、crate 与
  合并前逐字节相同。机制：tracing 的 callsite interest 进程级缓存——该
  `info!` 点（:321）首次注册时若当前默认订阅者是别的过滤更严用例，interest
  缓存为 Never，本用例自己的订阅者（:635-637，`with_max_level(INFO)`）也
  收不到。:610-613 注释只论证了线程局部订阅的隔离，未覆盖 interest 缓存。
- **s7 证据**：`git ls-files .zcode` = `plans/plan-sess_73a4b952-8a68-4725-
  8f47-6be52755ed37.md`（70410f5e9 引入，10KB 会话计划）；origin/main 无
  `.zcode`；`.gitignore` 只有 `.zcodeignore`（:125）无 `.zcode/` 规则；工作
  树另有未跟踪 plan 文件易被 `git add -A` 卷入。

## 票列表

| 票 | 标题 | 状态 |
| --- | --- | --- |
| [s1](tickets/s1-join-separator-i18n.md) | 8 处 join('、') + 全角括号换 i18n | closed |
| [s2](tickets/s2-dead-exports.md) | 5 个零引用导出清理 | closed |
| [s3](tickets/s3-nomination-expect-err.md) | 提名装配 expect → Err 化 | closed |
| [s4](tickets/s4-outline-task-terminal-contract.md) | v2 outline 任务终态契约（finish_done + 真实 course_id） | closed |
| [s5](tickets/s5-profiler-reset-race.md) | nomi-agent profiler 测试竞态串行保护 | closed |
| [s6](tickets/s6-tracing-callsite-interest.md) | structure_gen 日志捕获 interest 缓存修复 | closed |
| [s7](tickets/s7-zcode-hygiene.md) | .zcode 会话文件出库 + gitignore | closed |

## 非目标 / 后置清单（收图时明记，均非本波回归）

1. **c1–c3 三票真机目验**：一直未做，须用户真机实测，FAIL 按票重开
   （learning-create-progress-ux/map.md 文末）。
2. **companion 宠物气泡 en-US 中文残留**（`companion/messages/project.ts:117,
   134` 渲染 `evt.text`）：裁决 2 划出学习模块范围，归 companion 侧。
3. **AppError 错误文案 i18n**（error-surface contract 管辖）：前端
   `utils.ts:123` errorMessage 裸透传、`useCourseCreation.ts:222`；
   后端 `CANCELLED_MESSAGE`（generation_tasks.rs:1517）与
   `structure.rs:1207` 裸中文进 HTTP/WS 错误面。
4. **EvolutionTab LearningModelRow/LearningSection 14 处中文 defaultValue**：
   main 侧存量（非本分支改动面），归 main 自身的 i18n 清理，不在本波。
5. **nomifun-ai-agent acp.rs 同款 tracing 捕获形状**：与 s6 同机制风险，
   未在本波核实是否已有同样断言面，留观。
