# Map: 并回兼容收口——feat/learn0928 → main 合并前必修四票（m1–m4）

<!-- wayfinder:map -->
<!-- 状态：收图（2026-11-09）。四票 m1–m4 全部施工完毕、双轴代码评审（规范
      0 硬违规 + 规格零实现错误）后闭票；真机判据（m2 升级演练 / m3 目验 /
      m4 手验）留待下次真机冒烟批次执行，FAIL 按票重开。集成 =
      feat/learning-merge-compat，已并回 feat/learn0928。
      来源 = 并回 main 前的兼容性审计（本会话三路子代理深查 + 逐项实码复核
      + 影响面复查），用户裁决后立票。审计结论：迁移层
      （070_learning_domain_rebuild）闭环安全——8 张 rebuild 表逐列
      INSERT SELECT 零丢行、CHECK 只松不紧、基线迁移测试覆盖已发布链
      001–065 + 存量数据升级路径；无 BLOCKER 级数据损坏风险。 -->
<!-- 基线：feat/learn0928 尖端 85edbb6ad；集成分支尖端见 git。 -->

## 用户裁决（2026-11-09，本波次全部工作的依据）

1. **图课程允许破坏式更新，不再做兼容，但至少不能导致程序报错**（panic/500/
   白屏都算报错）；一次性清掉所有之前的图课程（留着也不可能继续兼容）。
2. **普通知识库生成的传统课程需要进行兼容**：已发布构建（main，迁移 001–065）
   写下的行在 070 之后必须继续正确加载与计数。
3. **课程行已失联的孤儿复习卡子树统一清理**（含 main 时代传统删课「保留复习」
   的卡——行为变化已接受，与新删除语义「删课 = 卡清」一致）。

## 审计发现 → 四票映射

| 发现 | 分类 | 归票 |
| --- | --- | --- |
| ① 结构修补回合：LLM ops 重复 merge source → `.expect` panic → 自动维护任务死亡且标志位永不复位（停摆到进程重启） | 程序报错红线（图课程不报错裁决） | **m1** |
| ② due 队列 join 口径变更 + 课程行失联孤儿子树：到期数/日历/记忆预测永久虚高 + 题库幽灵行（传统课程兼容裁决） | 传统兼容回归 | **m2** |
| ③ 结构 tab 六文件 43 处 `t()` 漏 `learning.` 前缀 → 用户看到裸 key 文本 | 用户可见缺陷（图功能） | **m3** |
| ④ 课内答题提交被「未选模型」门拦死，规则判分的客观题也被拦（main 无此门） | 功能回归（传统用户） | **m4** |

## 影响面核查关键事实（2026-11-09 实码核实，票面直接引用）

- **孤儿修复永远不删 historical 引用**：`id_schema_contract.rs:595-601` 宏文档
  明写 "repair never deletes these rows"；`learning_enrollments.course_id`（:907）
  与 `learning_modules.course_id`（:898）都是 `historical_text_ref`——main 时代
  「删课保留复习」留下的子树不会被启动修复清理，070 清理段是唯一收口手段。
  repair 只作用于 `RequireParent` 引用（:1452-1454）。
- **清理集恰好限定旧形状 11 表**：070 新生的表（xp_events / mounts / findings /
  repairs / maintenance / quiz_rounds / basis / system_actions / capacity_feedback）
  在 070 之前不存在，对被清课程零 legacy 行——清理段不碰它们；KeepHistory 语义
  （xp_events/review_events/findings/repairs）不受影响。
- **图模块生产代码 panic 候选全量仅 2 处**：structure_ops.rs:637（必修）与
  name_adjudication.rs:157（有前置校验「每名恰一裁决 + 拒多余名字」把守，
  同类别顺手加固）；fragment_verdict.rs:400/408 的 expect 全在 `#[cfg(test)]`。
- **`t()` 漏前缀全仓恰好 43 处 / 6 文件**（`grep -rn "t('learning[A-Z]"` 全
  renderer 扫描），key 本身双语齐全，纯机械修；`check:i18n` 只查词典文件奇偶
  不扫调用点，故未拦住。
- **模型门全仓 3 处仅 1 处错**：`LessonQuestionDialog.tsx:137`（生成题目）与
  `LessonStudy.tsx:752`（节重写）是真需要 AI 的操作，门是对的；错的是
  `useCourseLearning.ts:139` 的答题提交门。判分谓词单源 =
  `ActivityKind::is_ai_graded`（models.rs:1521-1524）= `Reflection | OpenQuestion`；
  后端对缺省模型有回退（models.rs:2747-2756），故 AI 题的门是 UX 选择不是正确
  性需求，客观题的门则纯属误伤。
- **070 未发布可原地修**（先例 217030a7c 净形折叠）：已在本分支跑过 070 的
  dev 库谱系本就要 reset:dev（070 头注既有约定），原地修无额外代价。
  已发布用户库（001–065）走 001–068+070 全新链，前缀兼容不受影响。
  070 头注「已发布迁移 001–068」措辞不准（实际已发布 001–065，066–068 随
  本次合并首发），随 m2 一并订正。

## 后置清单（不阻塞并回，本波不修，留观/后续票）

| 项 | 事实 | 为什么后置 |
| --- | --- | --- |
| XP 不回填显示失真 | 070 建 `learning_xp_events` 空、零回填；卡片文案 settled 优先（course.rs:975-979）出现「1/20」vs 进度条 55%；`xp.rs:278-302` ETA 虚高 | 需产品裁决（回填 vs 换文案 vs 接受从零起算），不报错 |
| 打卡规则切换 | `checkin.rs:84-157` 改按 dailyXpGoal(默认40) 锁日，旧 `dailyCheckinGoal` 读侧已删，老 streak 语义断裂 | 产品语义切换，不报错；真机观察后议 |
| 结构 tab 状态混淆 | 加载失败显示「生成中」（LearningGraphWorkspace.tsx:156-159/643-654 + StructureGraphTab.tsx:57-64）；合法空结构显示「已生成 0 主干」而非提示重生成 | 图课程 UX 打磨；错误顶部横幅可见，不白屏 |
| 读侧枚举不宽容 | 070 刚把枚举 CHECK 从 DB 移走，但 `CourseKind::try_from`（course.rs:945-948）、`LessonStatus`（learning_graph.rs:4231-4232）遇未知值炸整个列表 | 仅手改库可达；违反 ADR-0015 读侧宽容精神，低成本可随手修 |
| structure_json 坏形状 500 分支 | learning_graph.rs:3905-3909 `map_err(internal)?` 与「恒 200」注释不符 | DB CHECK `json_valid` 把门，分支不可达；注释措辞问题 |
| 迁移谱系测试缺口 | db_lifecycle.rs:1997 只测 checksum 不匹配；不测多余迁移行/错版本号；基线测试未播种复习卡/attempt/checkin 守恒面与「删课保留复习」fixture（m2 验收补后者） | 谱系机制本身工作正常；增量补强 |
| 传统线 demo 节降级死臂 | demo 硬门 vs 纯文字降级稿恒失败（lesson.rs:295-327/561） | f3 票已记档的存量项，需方向裁决 |
| 裸 key 扫描闸缺位 | check:i18n 不扫 `t()` 调用点，m3 修掉现有 43 处但无防复发闸 | 动态键误报需小设计，独立小票候选 |
| 前端健壮性小项 | 课程列表 Promise.all 一坏全坏（index.tsx:86-95）；generationTasks 瞬断 500 终止 35 分钟轮询（useCourseLearning.ts:55）；XpPanel 星期 label 作 React key 撞重（XpPanel.tsx:192）；GraphHistory/ConceptTable 错误态不随成功清理 | 均有兜底 UI 不白屏；攒一波前端打磨票 |
| 分节课时无 practice 节时题目隐藏 | 活动恒无 `section_key` 字段（后端不发）而前端 `=== null` 判断（model.ts:51、types.ts:307）——main 上已存在的存量问题 | 非本分支回归；改 `== null` + 标可选即修，随前端打磨票 |
| 孤儿课时 404（原审计 M1） | main 删课遗留课时现被 inner-join 拒答（course.rs:497-513） | **随 m2 清理段自然消失**（子树清掉后不再有该人群） |
| delete_course/by_title 重复级联提取 | course.rs 两段同形 DELETE 级联（m2 被迫两处同改；评审发现） | 存量形状；提取共享删除助手属重构，随学习域清理票 |
| panic 守卫同形下沉 shared | liveness guard/catch_unwind 与 nomi-agent tool_execution.rs 同形复制（评审发现） | 权衡「shared 保持稀少」；第二处再现时再议 |
| 门谓词两端漂移风险 | useCourseLearning 裸字面量镜像 ActivityKind::is_ai_graded，靠注释维持同步（评审发现） | 低成本收口=契约测试钉两端；随学习域测试加固票 |

## 验证总纲

1. m1 红绿样：`sources:["A","A"]` 不再 panic（去重或 Err→既有重试反馈通道）；
   维护任务死亡时标志位复位（模拟 panic 不再永久停摆）。
2. m2 迁移测试：播种「课程行失联子树」（传统删课保留 + 升级前已删图课程两种
   形态）→ 断言全清、活课程传统行守恒、review_events/checkins 留存；
   `cargo test -p nomifun-db` 全绿。
3. m3：全仓 `grep "t('learning[A-Z]"` 归零；双语词典奇偶过 `check:i18n`。
4. m4：无模型配置时客观题可答题、reflection/open_question 仍被门 + 提示。
5. 汇总门：`cargo check --workspace && bun run check`；真机冒烟（smoke content
   档 + 一次 dev 库升级演练：001–065 库 → 070 后课程列表/课时/复习/打卡全通）。

## 双轴评审记录（2026-11-09，集成分支 @ c0aa5af73）

- **规范轴：0 硬违规。** 署名 16 提交全人类零 AI 尾注；070 改动符合头注自订
  契约；前端三红线无违。判断题四条（不入本波）：①服务内 delete_course/
  delete_courses_by_title 重复级联（下文后置清单）；②panic 守卫与 nomi-agent
  工具执行面同形（可下沉 shared，权衡「shared 保持稀少」）；③m4 门谓词裸
  字面量镜像后端（注释已令同步）；④`merge(learning):` 类型——仓内有
  usability-followup closure 先例，实为家规，撤回。
- **规格轴：零实现错误。** m2 §8 与票面 SQL 逐字一致、070 既有段一字未动、
  fixture 断言全过；m1 保序去重钉死；m4 谓词恰好两题型；m3 全形态归零。
  两处打折已补（m1 守卫测试走包裹层真实形状 / by_title 红绿样），一处措辞
  偏差记档（name_adjudication 走 Degraded 通道），真机项留观。

## Tickets

| id | 票 | 优先 | 状态 |
| --- | --- | --- | --- |
| m1 | 结构修补 merge_sources 重复源 panic → Err + parse 去重 + 维护任务标志位守卫 | **高** | ✅ closed |
| m2 | 070 增课程失联孤儿子树清理段（+基线测试 fixture + 头注订正 + question_tags 顺序 bug） | **高** | ✅ closed |
| m3 | 结构 tab 43 处 `t()` 补 `learning.` 前缀 | **高** | ✅ closed |
| m4 | 课内答题模型门收窄到 AI 判分题型 | **高** | ✅ closed |
| m5 | companion store v3 老库缺基线表——升级启动即崩（真机演练新发现） | **高（并回 blocker）** | 🔴 open |

## 真机升级演练（2026-11-09，m2 判据 + m5 来源）

方案 [blueprints/2026-11-09-m2-upgrade-drill.md](blueprints/2026-11-09-m2-upgrade-drill.md)
执行完毕（双二进制 8fd9a4fb → 03a0c5380，约 75 分钟，证据
`C:/Users/test/Desktop/work/mc-drill/evidence/`）：

- **判据 2–6 全 PASS**（机器证据级）：due_count 10→4 精确等于期望（幻影卡
  L2 4 + G1 2 归零）；日历/记忆预测归真；问题管理 49→25 行幽灵清零；活课程
  7/7 读写流全 200；保全面 SQL 断言 35/35（L1 内容逐格 diff 424/424 相同、
  review_events 10 行逐行一致含 6 行被清卡作答流水、L2+G1 十一表全零）。
  **阴性对照**：升级前库喂同套脚本仅 20/35 过，失败项恰落在清扫/互斥/迁移
  面——脚本真有抓幻影能力。
- **判据 1 有条件 PASS → 催生 m5**：070 迁移本身成功（_sqlx_migrations 69 行、
  066/067/068/070 全 success、零 retired 触发），但新二进制首次启动崩在
  companion store 缺 `companion_learning_xp` 基线表（票外存量缺陷，04c5da393
  起）——已立 m5，为当前并回 blocker。
- 演练取舍如实记档：due_at 回拨制造到期卡（卡本体全走老 API 真路径）、checkin
  走队列清空自动落行、tutorial pack 剥不存在知识库引用、sections 空表由
  config_json/summary 全行 diff 承担守恒。
