# Map: 题面质量与练习体验波（sq1–sq5）

<!-- wayfinder:map -->
<!-- 状态：**已关波（2026-11-19）**：五票施工（sq3 4074e38fc / sq5
      489319671 / sq1 2acc9bc86 / sq2 488757c4b / sq4 c6ec3d875）+ 双轴
      评审（Standards 5 + Spec 6）+ 修复批 7 提交（3d0f2bb05 尖端）+
      广谱闸全绿（cargo check --workspace + bun run check）+ **真机冒烟
      矩阵五臂全过**（读数见「真机冒烟矩阵（实测）」节）。本波零迁移。
      运维事故留痕：smoke F 臂曾误以 nomifun-web 无 --data-dir 起服
      务，落到默认目录 Flowy\Nomi 触发 automatic-legacy-retirement（8
      月旧集 8.4M，non_v3_dataset）——已完整恢复原位、运行残留隔离至
      仓外 allo-smoke-g1/nomi-fresh-sideeffects/；活跃 Nomi-dev 零影响。 -->

## 已核验事实（2026-11-19 对码 + 对库取证）

- **同一课时第四次出缺陷**：pf 波取证过本课 matching 判别度为零；本波
  新实锤三题——ordering 裸指代（position 3）、回收超纲（position 7）、
  reflection 跳过无门（position 6）。生成链路是同一条：agent 线
  【活动契约】。
- **ordering 题（activity 01a1e70e-631b-7610-9b2b-2a1e14c9d66e）**：
  题面「把演示中值建立与失效的先后顺序排好。」，绑定 s3 演示节，
  evidence=NULL；五个选项全是 s3 mermaid 时序图的剧情碎片（订单名 /
  件数=3、单价=200 / 临时金额=600），**题面不携带任何剧情材料**——
  本节内靠回滚三屏勉强可答，进刷卡后不可答。且答案键与演示自身叙事
  冲突：演示 mermaid 把「订单名」写了两遍（第 1 步「主流程建立 订单名
  （局部变量）」在最前、第 8 步「建立 订单名，装入返回的引用」），落库
  答案把「主流程建立名字 订单名」排**最后**——照图顺读的学习者会排
  第一。裸指代题连答案键都是可争议的。
- **evidence 全课失守**：本课 8 题 evidence 全 NULL。传统线有
  evidence_missing 硬门（generation/activities.rs:340），agent 线契约
  无此条款、审计无此 finding——**换线即丢件**。
- **回收超纲题（position 7，s4）**：正确选项判定轴 =「两套写法各自的
  回收路径与持有者」。节点 scope 声明逐字「不教指针/引用语法，不教回
  收细节，留待后续」；s3 正文边界句逐字「此处不展开那套机制，下一节点
  起再谈」。正文面尽力了，题目面没接住：scope_findings 词面机（z03）
  只扫节标题与正文，**不扫题面（prompt/options）**；审计无题面 scope
  finding。
- **reflection 跳过无门**：用户在文本框键入「跳过」二字，被 AI 判 0
  分 + 六条缺失清单。submit_attempt（progress.rs:131）AI 批改题提交即
  LLM 判分、空答案被 evaluate 拒；learning_attempts.score REAL NOT
  NULL CHECK(0.0–1.0)——skipped 无第一类语义。
- **section_key 绑定消费面对码（诚实结论）**：后端零消费（种卡
  progress.rs:685 / 复习出队 review.rs:57 / 判分 evaluate / 变式装配
  quiz.rs 全不读 activities.section_key）；前端唯一消费 model.ts:58-84
  stepActivities——outline 硬契约「恰好一个收尾练习节」下
  earlierPracticeKeys 恒空、全部题归收尾轮，**绑定对学习者可见面零效
  果**。唯一活效果是反向的：绑定条款（lesson_content_loop.rs:304）+
  旧线每节配额句（generation/lesson.rs:1074）诱导「按节覆盖」出题——
  本课 8 题按节均摊（s1×2/s2×2/s3×1/s4×2/s6×1）即实锤。
- **刷卡链放大裸指代**：完成课时即种卡（progress.rs:110→685，ordering
  有出队资格）→ 出队 SELECT 只带 prompt/lesson_title（review.rs:57）→
  卡面=题面+选项+课时标题（ReviewSession.tsx:164）。演示剧情不在任何
  字段里。
- **存量救济缺口**：activities 无 update/delete 路由
  （routes.rs:128-137 只有 create/generate/attempts）——坏题无人手下
  架路径。
- **正文「回收」提及判定（2026-11-19 用户质询，二次裁决**推翻初判
  「合法边界说明」**）**：三处提及全部违规形态——
  1. **承诺不可验证**：后续节点（堆分配/垃圾回收/引用计数）在
     learning_lessons 里 content_generated 全空——正文写作时「下一节
     点」只是图上的名字+一句话 note，实际教学范围由教练后续独立决定，
     教练生成节点**不依据**正文给的承诺。「下一节点起再谈」是正文对自
     己无权支配的内容做担保。正文最多预告**同节点内**的下一节（manifest
     同次生成可控），根本不该预测其它节点。
  2. **功能性提及违反既有条款**：概念注册表零「回收」命中——「回收机
     制」是名单外未立名概念；构件词边界条款逐字「名单外的领域名词不立
     名……不作讲解构件」，而 mermaid Note「这一块归回收机制处理」/观
     察点 3「要交给回收机制的问题」正是拿它**承载功能意义**——违反既
     有契约，只是零机械执行面（词面机只扫 scope 排除词，粒度还漏）。
  3. **零收益正账**：孤儿事实在已教框架内已闭环（「值仍然在堆区，只是
     再也用不到」+「栈区从不会有孤儿」已是完整判定依据）；提及的代价
     = 给学习者悬空名词（对回收无概念的学习者零接收面）+ 给出题链「正
     文教过」的假依据（本次回收超纲题的直接弹药），收益 = 0。删除三处
     提及，教学效果不变。
- **排除词匹配粒度缺口（2026-11-19 补查）**：scope 词面机整词子串匹配
  （has_unexempted_mention :194），排除词「回收细节」拦不住题面「回收
  路径/回收方式」——sq2 词面闸必须词干级匹配，否则本例回归证据直接
  漏过。

## 影响面清单（2026-11-19 全量清点）

**A. 出题生产面（sq1/sq2 契约与闸须三面全覆盖）**

| 面 | 入口 | 闸的执行点 |
| --- | --- | --- |
| agent 线整课题库面 | lesson_content_loop【活动契约】ls_patch_activities | lesson_draft audit_findings + ls_finish 发布门 |
| 传统线整课题库面 | build_activities_prompt + LESSON_SYSTEM_TAIL | 同上（lesson_draft 审计共库） |
| 单题 LLM 面（AI 手动加题 + **检测轮变式站**） | lesson_activity_system_prompt → generate_lesson_activity → validate_generated_activity | validate_generated_activity（变式站 generate_quiz_variant 复用此面，**以旧题面为换皮底——旧题面坏则变式遗传坏**） |
| 手动表单加题（无 LLM） | create_lesson_activity 直 INSERT，不过审计 | 无从拦——sq5 删除兜底（如实留痕） |

**B. 正文生产面（sq2 正文半）**：agent 线逐节写作（ls_set_section_body）、
传统线逐节写作（section_body_* 三风格）、AI 节重写（rewrite_lesson_
section——**执行点缺口：persist_rewritten_section 直 UPDATE 不重跑全量
审计**）、手动编辑正文（同落库路，无 LLM 不可拦）。

**C. 机械门面**：lesson_draft audit_findings（新 finding ×3：裸指代/
题面 scope/evidence）+ scope_findings 词面机（粒度适用界+扫描面扩
activities）+ validate_generated_activity（单题面同步加校验）+ 修复环
（agent 修复轮吃 findings 自动闭环）。

**D. 消费面**：练习轮/步进门（sq3/sq5/sq4）、复习卡（自足题面后可答，
卡面不加上下文=R-d 不做）、检测轮 quiz（objective-only + 变式）、种卡/
结算（sq3 剔除三处、sq5 级联）、i18n 双语（跳过/删除/已跳过文案）。

**E. 存量面（dev 库实测 2026-11-19）**：1 课 5 课时 33 题。题面裸指
代 1（本课 ordering）；**evidence 全空课时 3/5**（竞态条件/值存储位置/
消息传递共 15 题——「参数传递」5/5、「判定类型检查时机」6/8 有值，
回归按代次发生）；正文跨节点承诺 2 节（值存储位置 s3 + 判定类型检查
时机 s3，后者施工时逐句核）；后续节点零内容=承诺全悬空。救济=sq5 删
除+手动补题，规模小人工可行。

**F. 测试面**：两线契约一致性测试、golden 常量测试、audit/scope_findings
单测、pf5 注册表派生一致性测试（sq4 摘句须同步）、前端 model/LessonStudy
测试（sq3/sq4/sq5）、i18n 双语断言。

## 票表

| id | 票 | 严重度 | 类别 |
| --- | --- | --- | --- |
| sq1 | 题面自足性：契约条款两线同步 + 审计词面门（裸指代 danger）+ agent 线 evidence 条款恢复 | 高（刷卡不可答的根因） | 生成质量/机械门 |
| sq2 | 题目超纲闸：scope 不教名单扩扫题面；边界句点名的概念不得作考点轴 | 高（回收超纲=教学事故实锤） | 生成质量/机械门 |
| sq3 | AI 批改题跳过：练习轮 skip 按钮 + skipped attempt 语义 + 账本不变式 | 中（开放式题精力门槛，用户点名） | 作答 UX/后端 |
| sq4 | section_key 绑定退役：拆「按节覆盖」诱因，题组取向改学习效果中心 | 中（消费面闲置+反向诱导） | 契约/退役 |
| sq5 | 存量题救济：题删除路由 + 前端入口 + 级联面裁决（attempts 账本保留） | 高（存量坏题无下架路径） | 救济面 |

施工顺序建议：sq5 最前（救济面让存量坏题立即可清理）→ sq3（作答体
验，独立面）→ sq1+sq2+sq4 合并施工（同一【活动契约】面改一次，避免
三改同一提示词块）→ 双轴审查 + 广谱闸 + **真机冒烟矩阵（验收主关卡，
下节）**。

## 真机冒烟矩阵（验收主关卡，2026-11-19 用户指令：重点进行足够的真机
## 冒烟测试，确定优化效果）

原则：冻结二进制（CARGO_INCREMENTAL=0、跑前核 nomicore.exe mtime）、
各臂严格串行、NOMI_CHANNEL=dev 显式带、--data-dir 放子命令前。效果判
定用**机械可计数指标**，对照基线=本波取证（存量缺陷画像，见已核验事
实）。

| 臂 | 验什么 | 效果指标（对照存量基线） |
| --- | --- | --- |
| G1 生成冒烟（陷阱节点） | sq1+sq2+sq4 生成契约与机械门端到端。节点故意带陷阱要素：demo 节+练习节（裸指代风险）、scope 带「不教 X」边界、多节正文 | 题面裸指代词表命中 **0**（基线 1）；objective 题 evidence 覆盖 **100%**（基线 0/8）；题面/选项 excluded-term 命中 **0**；正文跨节点承诺词表命中 **0**（基线 2 节）；新题 section_key 全 NULL；题组不再按节均摊 |
| G2 审计门负样本 | 人为构造裸指代题+越界题投草稿 → ls_audit | 新 finding 全部命中且 ls_finish 被阻；按修复指引改写后过闸 |
| F1 练习轮跳过（真机 UI/API） | sq3 跳过钮端到端 | skipped attempt 落库带 marker；零 LLM 判分调用；步进门放行；薄弱集/XP/ETA 读数不含该 attempt |
| F2 删题救济（真机 UI/API，对真实库） | sq5 删除两道坏题（ordering+回收题） | 题消失且练习轮/复习队列收敛；learning_attempts 行保留（账本不变式）；删除后手动补题可走通 |
| R1 复习出队回归 | 自足题面+课时标题=卡面可答；deprecated/已删题不出队 | 出队清单无裸指代题、无已删题 |

读数随关波文档落 map（pf 波先例：真机 smoke EXIT=0 + 首跑产出读数）。
UI 臂（F1/F2）优先用浏览器驱动真机 dev 页面走查；后端效应指标用库态
直查（bun:sqlite readonly）复核。

## 真机冒烟矩阵（实测读数，2026-11-19）

执行条件：冻结二进制（nomicore.exe 17:00 mtime 已核，CARGO_INCREMENTAL=
0）+ 真实 dev 库 VACUUM INTO 快照 + NOMI_CHANNEL=dev + 显式 provider
0190f5fe…/AIPC-deepseek-v4.1-flash。臂间严格串行。

| 臂 | 结果 | 关键读数 |
| --- | --- | --- |
| G1 陷阱节点生成 | **EXIT=0 判据全过**（76.6s/20 调用） | 裸指代词表命中 **0**（基线 1）；evidence 覆盖 **4/4=100%**（基线 0/8）；scope 排除词（GARCH）进题面 **0**；正文跨节点承诺 **0**（基线 2 节，双节点课零承诺）；section_key 非空 **0**（全 NULL）；题组 5 题 4 题型全自携材料、不按节均摊；classify（pf2）真机复现；scope_findings first_pass_hits=0、越界专名 hit=0 交叉确认 |
| G2 审计负样本 | **过**（真实门代码单测） | `scope_boundary_hit_blocks_finish_and_the_repair_loop_publishes` + sq1 裸指代门/evidence 门两道 finish 环测试（命中→阻发布→按指引改写→过闸），1014 全绿内 |
| F1 练习轮跳过 | **过**（真实 dev 库 API 实弹） | POST attempts `{"response":{"skipped":true}}` → HTTP 200 **11ms**（零 LLM 直接证据）；AttemptResult `skipped:true` 回透；库态 score=0/passed=0+marker 落库；reflection 追加式 2 条（原作答保留）；skip 后新 XP 事件 **0**、XP 读数正常（账本不变式） |
| F2 删题救济 | **过**（对真实库删两道坏题） | 裸指代 ordering + 回收超纲题 HTTP 200；级联：activity 行消失、review_items 行消失、question_tags 消失、**attempts 账本保留（1+1）**；练习面余 6 题 |
| R1 复习出队回归 | **过** | 队列 **6→4**：两张坏卡（本应明晨 02:00 到期的不可答题）随删题消失，剩余 4 张全是健康卡；队列无悬挂无死卡 |

G1 附注：现有 content 夹具自带 GARCH 边界陷阱 + 双节点承诺诱惑，零改
动即覆盖四张门指标；「演示剧情」原缺陷形态由 sq1 词面门（题面零豁免）
+ 修复环测试覆盖，未复刻节点夹具（成本/收益裁决）。

### G1-R 真实课程重生臂（2026-11-19 追加，用户指令：用真实课程数据测）

执行：真实库 VACUUM INTO 快照 → 「值存储位置」节点重置（activities/
sections 清、content_generated=0，intent/scope/purpose 与概念登记表
原样保留=真实陷阱条件）→ 身份四文件重绑定新 generation（storage-
generation/owner/binding/dataset-v3.json 路径改写快照目录）→ nomifun-
web 真服务 `POST /lessons/{id}/generate?wait=true` 同步重生（187.2s，
HTTP 200）→ 库态直查。

| 指标 | 基线 | 实测 |
| --- | --- | --- |
| 裸指代词表命中 | 1 | **0** |
| scope 边界词「回收（垃圾回收细节）」进题面 | 1（回收超纲题） | **0** |
| evidence 覆盖 | 0/8 | **3/3 = 100%** |
| 正文跨节点承诺 | 2 节 | **0**（正文以「本节点不考堆内部布局与回收算法」诚实收口） |
| section_key 非空 | 绑定形态 | **0**（全 NULL） |

题面人工判读（5 题：single_choice×2/classify/fill_in_blank/open_
question+reflection 共 5）：每道题面自携完整代码场景（函数体/代码块/
收益序列内嵌），考「创建方式+生命周期」判据本身而非剧情记忆；classify
判别轴=「名字与它指向的记录是两个对象」（本课真实判别轴）。重生正文
以「本节点不考堆内部布局与回收算法」形式收口=合法边界说明（不立名、
不承诺其它节点）。

**观察项（新，候选小票不阻塞本波）**：重生后节集合出现新旧两套并存
（sA–sD 新清单 + s1–s5 旧行未清，标题重复）——「重生既有课时」路径的
manifest 替换语义疑似只增不清，属重生既有课时的既有行为缺陷，与本波
五票无关，留独立小票候选。另注：本次重生模型自选清单无 demo 节（意图
画像更轻），demo 陷阱形态仍由门+修复环测试覆盖。

## 用户哲学对齐（2026-11-19 两次裁决叠加）

- **「上下文里有 ≠ 用户能看到」**：生成器视角的「材料在场」不构成辩
  护面；题面自足性是硬要求，判定视角唯一合法的是学习者所见。
- **一切以学习者的学习效果为中心**，不是生成器的覆盖便利——「按节覆
  盖」出题模式否定；题组存在的理由是验证可迁移的判据，不是给每节配一
  道证明题。
- **正文不得跨节点承诺**（2026-11-19 二次裁决）：后续节点由教练后续独
  立生成，正文写作时其范围不存在——「下一节点起再谈」类预告是无效担
  保；正文最多预告同节点内的下一节。
- **未立名概念的零收益提及不合理**：提及的代价（学习者悬空名词 + 出
  题链「正文教过」假依据）恒为正，收益为 0——「归回收机制处理」类功
  能性提及判死；边界说明只许「本节点不展开 X」形式（不引入名单外名词
  承载意义）。
- **机械解决优先**（e 波 SVG 画布 R-b 同律）：能词面闸的不加概率条款；
  sq1/sq2 的机械门是主修，契约 prose 是取向配套；换说法的语义越界不设
  机制面（z03 同律），归真机走查。
