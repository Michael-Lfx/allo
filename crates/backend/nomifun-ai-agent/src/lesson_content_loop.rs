//! Two-loop agent engine for lesson content generation — the lesson sibling
//! of [`crate::course_outline_loop`]. The generation loop (`ls_start` →
//! `ls_set_section_manifest` → per-section `ls_set_section_body` → batched
//! `ls_patch_activities` → `ls_audit` self-check) drives the draft, then
//! audit-gated repair rounds drive publishing.
//!
//! Same layering as the outline engine: nomifun-learning holds only
//! [`LessonContentAgentEngine`]; this crate provides the provider-backed
//! implementation, and the app layer wires it via
//! `LearningService::set_lesson_engine`. The loop mechanics, the fail-closed
//! whitelist and the audit gate contract are shared with the outline loop
//! through [`crate::loop_core`]; this module contributes the lesson prompts,
//! the `ls_*` tool set and the draft/publish context.
//!
//! The lesson loop reuses the fallback pipeline's exact document + activity
//! contracts (enforced deterministically by the lesson draft audit), so the
//! agent path and the legacy two-stage path produce interchangeable output.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use nomi_providers::{LlmProvider, create_provider};
use nomifun_common::{AppError, ProviderId, UserId};
use nomifun_learning::{
    ClaimVerdict, ConceptSearchMode, LessonContentAgentEngine, LessonGenerationContext,
    LessonOp, LessonOutput, LearningService, MathClaim, PRACTICE_BODY_TARGET_CHARS,
    VISUAL_OPTIONS, prose_budget_rules, section_range_rules, verify_claim, visual_menu_text,
};

use crate::factory::provider_config::resolve_provider_config;
use crate::knowledge_completer::resolve_default_model;
use crate::learning_loop::{FlowCycle, HistoryCompaction, LoopBudgets, LoopChannel, WireConfig, run_loops};
use crate::loop_core::{LoopEventSink, json_compact};
use crate::one_shot::{OneShotDeps, OneShotTool, one_shot_handler};

/// 课时内容循环的显式预算表（ADR-0004）：与 loop_core 的共享默认逐字一致
/// （50 轮 / 8192 token / 600s）——显式声明而非隐式继承，每流的生效值在
/// 自己的文件里可见。
pub(crate) const BUDGETS: LoopBudgets = LoopBudgets {
    generate_max_rounds: crate::loop_core::GENERATE_MAX_ROUNDS,
    round_tokens: crate::loop_core::AGENT_MAX_TOKENS,
    timeout_secs: crate::loop_core::TOTAL_TIMEOUT_SECS,
};

/// 线上翻译差异表（ADR-0004）：课时流无 kind 标记、不翻译 round_feedback、
/// start 帧不带 phase（不上线），但保留轮次日志（「生成」标签）且走课时
/// 专属事件流（`learning.lesson-generation`）。
pub(crate) const WIRE: WireConfig = WireConfig {
    kind_tag: None,
    round_log_gen_label: Some("生成"),
    translate_round_feedback: false,
    generate_start_phase: None,
    repair_start_phase: None,
    lesson_stream: true,
};

/// 写作成色 V2（k05，learnhub ldata1 的 R10–R13 + w1 追 R14；y06 追
/// R15–R20；域中立措辞，零新机械门）：抽象对象首讲具象替身 / 例子情境取
/// 受众生活域 / 正式表述的记号术语在已知集内 / 断言-例子数字一致 / 可算量
/// 必须实算（绑 verify_math 工具）；y06 的 R15–R19 材料学与 R20 完备性
/// 写作面同块装配。与 nomifun-learning 的 `CONTENT_DISCIPLINE_V2_BLOCK`
/// 非对称孪生（两处手写各随其提示词风格；本面有工具循环 → R14 绑
/// verify_math，传统线 EN 版是纯纪律——蓝图 A4），开关
/// `NOMI_LEARNING_CONTENT_DISCIPLINE_V2=off` 时整块不装配（R14 同命，工具
/// op 仍在但不被条款点名）。R12 为 #177 收窄版：「先引入再用」逃生门仅限
/// 记号与读法，领域概念不适用。
const CONTENT_DISCIPLINE_V2_CN: &str = "
- 具象替身先（R10）：抽象对象（结构、记号、运算）首讲时，先给一个具象替身——几何图像、动作隐喻、或具体情境里摸得到的量——让随后的规则与性质能借替身解释；关键规则配一句「为什么如此」的语义说明，不让学习者靠裸符号死记。
- 受众语域（R11）：课程声明面向成人、职场等非低龄受众时，例子与情境的人物、场景取该受众的生活域——不幼态、不写低龄课堂情境；讲法保留实物与教具，情境不锁龄。
- 表述工具在场（R12）：规则的正式表述所用记号与术语，须在已知集内（本节点 teaches ∪ assumes；用户消息带「课程已知概念」名单时该名单同在已知集内）——不在已知集的不即兴使用，换自然语言表述。「正文内先正式引入再用」只适用于**记号与读法**（本节点教学必需的表述工具，如新符号怎么写怎么读）；承载知识功能的领域概念不适用——名单外的概念是后续节点的事，不得在正文立名引入，需要该意思时用日常词（范围、找不满足的例子、至少有一个）。
- 断言-例子一致（R13）：为后果类断言配的例子里，数字必须能实际推出该断言（漏数例报数应少于总数、重复例应多于总数）；操作型演示每一步的中间结果逐一验算，只核对终态不算验算。
- 可算量必须实算（R14）：凡能从本节已印数据直接推算的量（和、均值、单位换算、计数、日期差），先调 verify_math 实算再写入正文，并展示可复核的中间步骤；对照列与精确列同等展开，禁止从均值直接跳到和；「≈」只许用在最后开方/极限步。
- 例子四件齐（R15，材料）：成例交代对象、条件、结果与连接三者的过程，不写只有标签没有过程的结论式例子；操作型示范写「行动 + 可观察反馈 + 调整依据」，不套四件。
- 符号—对象—条件落点（R16，材料）：公式或定义首现时逐项说明每个符号指代什么对象、受什么条件约束；本条不新增术语，只映射 R12 已放行的记号，名单外术语仍然不立名、不借本条放行。
- 类比带边界（R17，材料）：每个类比明说对应什么、不对应什么，学习者不会把映射带过断点；费曼风格原有的边界句保留为该风格内的加码。
- 分段例组（R18，材料）：分界或切换的两侧各配一例，再配一个压线例与切换条件；线性关系不硬凑分段；与既有 3-6 例题密度并置，本条只塑形既有例题、不新增例题。
- 关系句可还原（R19，材料）：陈述关系的句子保留方向与条件，学习者能逐字还原；相关不写成因果，性质不升级为充要。
- 完备面不缩水（R20，写作面）：节清单分配的三个面在正文里一个不少——认得出什么（名称与边界）、推得出什么（结论与成立条件）、怎么推出来（中间步骤）；只在已教边界内补覆盖缺口，不扩到前提与前置节之外。
";

/// u05 批量核验句（on 臂追加在 R14 条款行尾；off 臂零出现——R14 逐字回退）。
const R14_BATCH_SENTENCE: &str = "一节写完后，把本节全部可算量一次性放进 claims 数组提交核验（单次至多 12 条），不要逐条多次调用。";
/// R14 行尾锚（块内唯一出现——批量句的拼接点）。
const R14_TAIL_ANCHOR: &str = "「≈」只许用在最后开方/极限步。";

/// u07 档位优先序条款（`NOMI_LEARNING_TIER_PRIORITY` on 臂注入「写作成色」
/// 段的「档位决定内容形态」行之后；off 臂 = 空串，模板逐字节回基线）。
/// 票面语义：知道档 = 认识性讲解、不引入需逐步实算的推导、数值例至多 1
/// （如有须经 verify_math）；例题密度/完备面要求只适用于会用、能教档。
/// 末半句是反向护栏（票面推翻条件的方向锚）：不为凑密度写推导课，也不把
/// 认识课删成只剩定义（概念面登场纪律不破）。单行条款（恰一个尾换行）。
const TIER_PRIORITY_CLAUSE_CN: &str = "- 档位优先序（u07）：有档位声明时，档位优先于其它深度条款——标「知道」的节点做认识性讲解（认得、会辨认，有情境引入），不引入需逐步实算的推导，数值例至多 1 个（如有须经 verify_math 实算后才写入）；「例题密度 3-6」「完备面不缩水」等深度要求只适用于标「会用/能教」的节点。档位与其它条款冲突时按档位收敛，不为凑密度把认识课写成推导课，也不把认识课删成只剩定义、无情境引入的空心壳。\n";

/// u07 条款装配面（纯函数——同进程双臂可测，env 不进单测；
/// `discipline_v2_block` 同款先例）。
fn tier_priority_clause(enabled: bool) -> &'static str {
    if enabled {
        TIER_PRIORITY_CLAUSE_CN
    } else {
        ""
    }
}

/// 写作成色块的开关装配面（纯函数——同进程双臂可测，env 不进单测；
/// `graph_budget_line` 同款先例）。`verify_batch` off = 基线块逐字回退
/// （直接回基线常量，结构保证）；on = 仅 R14 行尾追加批量句，其余条款
/// 逐字节不动。`content_discipline_v2` off 时本函数不被调用（整块摘除）。
fn discipline_v2_block(verify_batch: bool) -> String {
    if !verify_batch {
        return CONTENT_DISCIPLINE_V2_CN.to_owned();
    }
    CONTENT_DISCIPLINE_V2_CN.replacen(
        R14_TAIL_ANCHOR,
        &format!("{R14_TAIL_ANCHOR}{R14_BATCH_SENTENCE}"),
        1,
    )
}

/// 完备性三面的规划面（y06，learnhub #202）：节点规划时过三面并查覆盖缺口
/// ——这是已教边界内的覆盖下限，不是超纲许可；「只教本节任务点 / 只考正文
/// 教过的」上限条款照旧（显式并置防互相加码）。
const COMPLETENESS_PLANNING_CN: &str = "\n- 完备性三面（R20，规划面）：每个节点规划时过三面——认得出什么（名称 + 边界）、推得出什么（结论 + 成立条件）、怎么推出来（中间步骤）；在已教边界内查覆盖缺口，缺哪个补哪个，不虚构前提之外的内容。这是边界内的覆盖下限，不是超纲许可：「只教本节任务点」「只考正文教过的」上限条款照旧。";

/// 分段例组的规划半（y06，learnhub #203）：分界/切换关系在规划时就排成
/// 两侧例 + 压线例 + 切换条件；线性关系不硬凑分段，例题总数照既有密度与
/// 「规划即雄心」，不借本条加量。
const SEGMENTED_EXAMPLES_PLANNING_CN: &str = "\n- 分段例组（R18，规划面）：遇分界或切换关系，在分界两侧各排一例、再排一个压线例与切换条件；线性关系不硬凑分段，例题总数照既有密度与「规划即雄心」，不借本条加量。";

/// 干扰项五型菜单的图线孪生（y06，learnhub #201；CN 版进活动契约，长菜单
/// 不进工具描述）：每型只在正文材料足以支撑自然负例时使用；fill_in_blank
/// 既有近义干扰与必填 distractors 不动，ordering / matching / numeric 不适用。
const DISTRACTOR_MENU_CN: &str = "\n- 干扰项五型（R21，选择题面）：single_choice / multi_choice / true_false 的诱惑项按五型设计——近邻负例（只破一个条件）、部分条件满足（满足部分条件却写成全部满足）、判联错配（结论对但对象或关系错）、表述转换陷阱（量词/方向/范围被同义改写悄悄翻转）、集合关系检查（子集/超集/相交/相离混淆）。每型只在正文材料足以支撑一个自然负例时使用，正文没有该对照就不写。fill_in_blank 的近义干扰与必填 distractors 规则不变；ordering / matching / numeric 不适用。适用时优先自然负例，无自然负例不得硬造。";

/// 题库两考向（y06，learnhub #202）：认一认与推一推都要有份量——取向不是
/// 题量配额（9 种题型 + 多样性 + reflection/open 配额已定，绝不加数）。
const QUESTION_COVERAGE_CN: &str = "\n- 两个考向都要有份量（R20，题库面）：认一认（名称与边界）与推一推（结论与成立条件）两类问题在题组里都要有份量；这是取向不是题量配额，不为凑数加题。";

/// 时长预算面（k03，开关 `NOMI_LEARNING_EST_BUDGET` 的语义臂，learnhub
/// #178 换血版）：预算行照读之外，意图画像四角（预算 × teaches 档位 ×
/// 概念首现 × assumes）定内容雄心；超预算先砍雄心再砍内容，收敛不住 =
/// 节点意图超载，capacity_note 必写指名原因（上游 est 复议信号）。
const EST_INTENT_CLAUSE_CN: &str = "- 时长预算：用户消息里有「时长预算」行时，规划节清单与逐节写作都对照它——节容量按预算铺。
- 意图画像与容量预算（节清单定稿前读，定稿时核）：先读「时长预算 × teaches 档位 × 概念首现 × assumes」的组合——短预算×首现×知道档×无前提 = 轻觘认课（认得、会辨认，少而准）；长预算×多前提×会用档 = 操练整合课；短预算×重访概念 = 巩固变式；长预算×能教档 = 完整推导。节数、题量与形态与画像相称，时长预算是画像的落地预算。定稿后逐节估容量，超预算**先砍雄心再砍内容**——减题、并节、降示例密度，teaches 的覆盖不可砍；收敛不住（保覆盖就必超）= 节点意图超载，ls_set_section_manifest 的 capacity_note 必写、指名超载原因（那是上游 est 复议的信号，不是可写可不写的自报）；预算内收敛 = 整键省略。时长预算缺席的节点不虚构预算，只按画像其余几角定性相称。
";

/// 时长预算面的旧语义臂（k03 #154 原文，开关 off 时整段回退——同二进制
/// 双臂对照的 off 臂必须逐字复现旧世界）。
const EST_LEGACY_CLAUSE_CN: &str = "- 时长预算：用户消息里有「时长预算」行时，规划节清单与逐节写作都对照它——节容量按预算铺；节清单定稿后对照预算粗估一遍，明显超容才在 ls_set_section_manifest 的 capacity_note 里如实写一句（超在哪个节、大概多多少；不超容不写），超容自报只留痕、不改安排、不压缩讲解深度。
";

/// Generation-loop system prompt. The sectioned contract (ADR-0002) drives
/// the rounds: plan the manifest once, then one tool call per section body
/// (the model's attention stays on one learnable unit), then the activities
/// bound to section keys. The deterministic audit enforces the same rules.
/// 契约数值（节配比 / visual 菜单 / 文字预算）由 nomifun-learning 的渲染
/// 函数从 owner 表格生成（ADR-0002 追加决策：唯一事实源，禁止手写字面量）。
static GENERATE_LESSON_AGENT_SYSTEM: LazyLock<String> = LazyLock::new(|| {
    let section_ranges = section_range_rules();
    let content_discipline_v2 = nomifun_learning::content_discipline_v2_enabled();
    // u05：写作成色块的装配走纯函数（R14 行按批量开关追加批量句；off 臂
    // 逐字回基线常量——discipline_v2_block 注记）。
    let discipline_v2 = if content_discipline_v2 {
        discipline_v2_block(nomifun_learning::verify_batch_enabled())
    } else {
        String::new()
    };
    // u07：档位优先序条款（占位符在「档位决定内容形态」行之后；off 臂
    // 空串 = 模板逐字节回基线）。
    let tier_priority_clause = tier_priority_clause(nomifun_learning::tier_priority_enabled());
    // y06（#201/#202/#203）：条款批与写作成色块同命——开关 off 时整块摘除
    // （规划三面 / 分段例组规划半 / 选择题面五型 / 题库两考向）。
    let completeness_planning = if content_discipline_v2 {
        COMPLETENESS_PLANNING_CN
    } else {
        ""
    };
    let segmented_examples_planning = if content_discipline_v2 {
        SEGMENTED_EXAMPLES_PLANNING_CN
    } else {
        ""
    };
    let distractor_menu = if content_discipline_v2 {
        DISTRACTOR_MENU_CN
    } else {
        ""
    };
    let question_coverage = if content_discipline_v2 {
        QUESTION_COVERAGE_CN
    } else {
        ""
    };
    // 时长预算面（k03/#178）：开关 off 臂整段回退旧语义（同二进制双臂）。
    let budget_clause = if nomifun_learning::est_budget_enabled() {
        EST_INTENT_CLAUSE_CN
    } else {
        EST_LEGACY_CLAUSE_CN
    };
    let visual_menu = visual_menu_text();
    let prose_budgets = prose_budget_rules();
    format!(
        r#"你是一名课时内容设计代理：把给定的一个课时规划为若干「节」，逐节撰写正文并设计检索题目，通过工具逐步构建，最终通过确定性审计门禁发布。

【分节契约（每节一次工具调用）】
- 先 ls_set_section_manifest 规划节清单：每节带 section_key（s1、s2……）、kind（concept 概念 / example 例题 / demo 演示 / summary 小结 / practice 练习）、title（带类型前缀，如「概念：…」）、points（一句话要点）。节数按课时复杂度自定：{section_ranges}；相邻节要有学习递进；最后一节必须是练习节（恰好 1 个）——学习者读完即进入统一练习轮。
- 每节带 visual 字段（该节承载核心讲解的可视化形态：{visual_menu} 之一，内容确实非视觉才可用 无）。
- 可视化为主、文字为辅是硬规则：概念/例题节的正文必须以至少一个可视化块（$$公式$$、```svg、```jsxgraph、```mermaid、对比表格）承载核心讲解，文字只作旁注；短段落、枚举用列表/表格、不写过渡废话。纯文字的概念/例题节会被质检门拒绝。
- 再逐节调用 ls_set_section_body 写正文：一次调用只写一节，body 直接以该节 `## ` 标题行开头（标题照抄清单），不要 JSON、不要包裹围栏、节内禁止 ### 子标题、不要自设练习环节（题目由题库承载）。
- 篇幅：{prose_budgets}；演示节由可视化块（```svg / ```jsxgraph / ```mermaid / $$数学$$）承载主要信息、旁注同预算；summary 是要点清单；practice 只写能力目标与作答引导（≤{PRACTICE_BODY_TARGET_CHARS} 字，不写题）。
{budget_clause}- 构件词边界：节标题、要点、例题与练习只把 用户消息「课程已知概念」名单 ∪ 本节点 teaches 的概念当工作词；名单外的领域名词不立名——不进节标题、不进要点、不作讲解构件（那是后续节点的内容，不预教），需要该意思时用日常词表述（范围、找不满足的例子、至少有一个）。本节点 teaches 的概念与教学必需的记号读法除外。
- 练习形态按档位：照示范复制本节点行为的练习（照例题格式改写、套格式、判断对错）各档都合法；练习引入本节点未教的方法论或推理链（归纳原理、多步推理证明一类）即越档——teaches 标「知道」的节点尤其如此：练习止于辨认与复制，方法论归「会用/能教」档。节清单定稿后逐节核对：每道练习考的是「示范过的行为」还是「没教过的方法」。
- 取材池（考察的概念域）：例题与练习考察的概念优先取自「课程已知概念」名单——用已教/已声明的概念出题，兼旧概念复习；情境载体另跟受众条款走，两不相扰。名单内多样轮换：不在单一概念上连出多题，不同题目轮换概念域与情境载体，避免同模子例句连排。{completeness_planning}{segmented_examples_planning}
- 声明优先（创建意图/教学范围）：用户消息带「创建意图：」「教学范围：」两行时，这两行是本节点意图与边界的**第一真相**——节安排不越出声明的教学范围，与意图画像冲突处照声明（「节点档位」行与开头「程度声明」照旧管档位与深度，分工不替代）；范围内讲透、范围外不无痕展开——范围外名词不立名、不展开、不作讲解构件；在小结或引入处点名边界（「本节点不教 X」一类）是对边界的诚实说明，不构成对 X 的引用，更不构成引入豁免。两行缺席（旧档节点）= 无声明面，照既有纪律走。
- 可视化优先：内容真正需要图示时才画，每个图必须自足完整（viewBox、命名点、坐标刻度、说明文字，svg 文本 ≥12px、无脚本无外链）；图形元素与文字必须完整落在 viewBox 内——画出画布边缘的文本/形状 = 图形缺陷（渲染端按声明尺寸裁剪，超界内容会被切掉）。图形块不计入篇幅。
- 与前一节已写正文自然衔接：不重复它讲过的内容；只写本节任务命中的范围，不越界讲后续节/课时。

【写作成色（域中立纪律——写作取向，不是格式模板）】
- 先推后练：方法与公式型结论先从已教前提推出，再进例题；不要先甩断言再补证明。
- 档位决定内容形态：标到「知道」的概念做认识性介绍，标到「会用」以应用示范为主，标到「能教」才给完整推导与反例；没有档位声明的课时按节任务命名的内容形态把握深浅。
{tier_priority_clause}- 汇合渐隐：前置众多（≥4 条前置声明或前置节）的节点是汇合处——例题逐级添加新要素，不一次性堆满全部要素。
- 对照要真对照：比较必须落在可核查的维度（适用范围、代价、失败形态），不写「X 更好」式的空对照。
- 例题密度：一节课的正文配 3-6 个递进例题（由易到难，最后一题触及边界情形）；节型已拆分例题节时按节承担，不重复堆叠。
- 规划即雄心：节数、题量与练习形态照节清单填充，不自行加码——规划收敛过的容量不再膨胀；练习不引入清单未安排的方法论或推理链（那是后续节点的事）；题目考察的概念域照节清单（已知概念名单优先），情境载体按受众取向。{discipline_v2}

【活动契约】
- 3-10 个活动：至少 2 个客观题（single_choice / true_false / fill_in_blank / multi_choice / numeric / ordering / matching）+ 至少 1 个 AI 批改题（reflection 反思；open_question 开放综合题至多 1 道），反思+开放合计 ≤3。
- 题型答案形状：single_choice 3-5 选项且 answer 恰等于其一；true_false 是布尔；fill_in_blank 恰含一个 "___"、answer 是 1-3 个等价答案数组、必须带 distractors 近义干扰；multi_choice 是 2+ 选项的数组（顺序无关）；numeric 是数字（可带 tol 容差）；ordering 的 options 是打乱条目、answer 是正确顺序；matching 的 options 是左列、answer 是一一对应的右列数组；reflection 与 open_question 的 answer 必须是 null。{question_coverage}{distractor_menu}
- 每题必须带 section_key 绑定到教它的那一节（清单里的 key）；至多 1 道跨节综合题用 "general"。
- estimated_minutes：5-60 的小整数，反映整体课时长度。

【工具使用纪律】
1. 第一步必须调用 ls_start 创建草稿——它返回 draft_id 与课时上下文。
2. ls_set_section_manifest 规划节清单 → 逐节 ls_set_section_body（每节一次调用，这是本轮循环的主要轮次）→ ls_patch_activities 分批提交题目（每批 1-5 个操作）。
3. ls_inspect 随时掌握草稿状态；全部构建完成后 ls_audit 自查，确认没有 danger 级问题才调用 ls_finish。

【结束条件】
- 只有 ls_audit 报告无 danger 时才调用 ls_finish；被门禁拒绝时按报告继续修复。"#
    )
});

/// Repair-loop system prompt: the audit report is the ONLY repair basis;
/// the model patches locally and never rewrites the lesson wholesale.
const REPAIR_LESSON_AGENT_SYSTEM: &str = r#"你是一名课时内容修复代理：基于确定性审计报告，精确修复学习文档与活动中被指出的问题。

【修复原则】
1. 审计报告是主要的修复依据：逐条处理 danger 级 findings，按报告给出的证据（文档章节、活动位置）精确操作。
2. 不推翻已通过的部分：文档缺章节/长度不足就 ls_set_document 重写补齐，活动形状不对就 ls_patch_activities 按位置更新。
3. 活动位置从 0 开始编号，以 ls_inspect 返回的活动清单为准；update_activity / remove_activity 引用的 position 必须与清单一致（注意先执行的操作会改变后续位置）。
4. 每批修复后用 ls_audit 复查对应 finding 是否消除；全部 danger 消除后调用 ls_finish 发布。

【常见修复动作对照】
- sections_missing / section_body_missing / section_invalid：ls_set_section_manifest 补规划，或 ls_set_section_body 按节重写正文（标题照抄清单、篇幅按节型、demo 节必须带可视化块）。
- document_missing / document_invalid：ls_set_document 重写完整文档（旧单篇契约，≥800 字符、三必需章节按序）——新草稿请优先走分节。
- activities_too_few / objective_activities_too_few：add_activity 补客观题。
- reflections_too_many：remove_activity 删多余的反思题。
- reflections_multiple（warning）：可保留——只有 danger 才阻断发布。
- activity_shape_invalid：update_activity 按位置重写该活动（选项数/答案形状/___ 空格/干扰项/容差/顺序与对应关系）。
- section_invalid（缺可视化）：按清单里该节的 visual 重写正文——用 $$公式$$ / ```svg / ```jsxgraph / ```mermaid / 表格承载核心讲解，文字作旁注。
- viz_block_lint（mermaid 括号不平衡/引号不配平/节点未定义）：修复该围栏块语法，或改用表格/公式承载该节讲解（终检已系统剔除的 viz_block_lint_dropped 按降级声明保留，无需重画）。
- math_verify_failed（warning 留痕）：本课生成期 verify_math 核验失败未必已全部修正——报告点名具体数值时按 R14 重算并用 verify_math 复核后改正文；无点名则无需动作（warning 不挡发布）。

【结束条件】
- 审计无 danger 时调用 ls_finish；若 ls_finish 被拒绝，认真阅读返回的阻塞报告并继续修复。
- 禁止空手结束：每一轮都必须调用工具（ls_set_document / ls_patch_activities / ls_audit / ls_finish）；只输出文字而不调用任何工具，会被判定为拒绝修复，整个生成以失败告终。
- 回复使用中文。"#;

/// Provider-backed engine for the two-loop lesson content pipeline.
pub struct LiveLessonContentAgentEngine {
    pub service: Arc<LearningService>,
    pub deps: OneShotDeps,
    /// 中断会话的轮次日志（draft_id → 每轮意图行）。resume 时取出注入开
    /// 场消息恢复认知；发布成功后清除。内存态，与草稿同生命周期（重启即
    /// 失）。与学习图循环的同名字段同构（断点续跑的另一半）。
    pub round_logs: Arc<Mutex<HashMap<String, Vec<String>>>>,
    /// 可选首轮观测钩子（冒烟存档面，k03/k05 读数）：每次生成先落一轮
    /// `{method:"lesson_agent_turn", system, user}`，发布成功再落
    /// `{method:"lesson_agent_publish", capacity_note, estimated_minutes,`
    /// viz_lint_events, scope_findings, math_verify}`。
    /// 生产接线恒 None（引擎自带 provider，不经 ArchivingCompleter）。
    pub prompt_observer: Option<Arc<dyn Fn(serde_json::Value) + Send + Sync>>,
    /// 可选 provider 装饰钩子（e4 冒烟瞬断注入面）：引擎解析出真实 provider
    /// 后经它包装再进计数层（`None` = 直通，生产接线恒 None）。smoke content
    /// 档注入「先掐断两次再放行」的 flaky 包装，钉「provider 层单次重试吸收
    /// 第一断、门面重试环吸收第二断，生成仍成功」的判据——与 prompt_observer
    /// 同一先例：测试接缝住结构体、生产零装配。
    pub provider_decorator: Option<
        Arc<dyn Fn(Arc<dyn LlmProvider>) -> Arc<dyn LlmProvider> + Send + Sync>,
    >,
}

#[async_trait::async_trait]
impl LessonContentAgentEngine for LiveLessonContentAgentEngine {
    async fn generate(
        &self,
        // Lesson content is a shared course asset: the caller identity is
        // accepted for trait uniformity but not used by the loop itself.
        _user_id: &UserId,
        context: &LessonGenerationContext,
        model_override: Option<(&str, &str)>,
    ) -> Result<LessonOutput, AppError> {
        self.run_generation(context, model_override, None).await
    }

    async fn resume(
        &self,
        _user_id: &UserId,
        draft_id: &str,
        context: &LessonGenerationContext,
        model_override: Option<(&str, &str)>,
    ) -> Result<LessonOutput, AppError> {
        self.run_generation(context, model_override, Some(draft_id.to_owned()))
            .await
    }
}

impl LiveLessonContentAgentEngine {
    /// Shared generation body: fresh runs start with an empty draft slot
    /// (`ls_start` creates the draft); resumes preset the slot with the
    /// surviving draft and inject its state plus the archived round logs
    /// into the opening user turn. Everything else — the timeout shell and
    /// the failure diagnostics — is identical for both entry points.
    async fn run_generation(
        &self,
        context: &LessonGenerationContext,
        model_override: Option<(&str, &str)>,
        resume_draft: Option<String>,
    ) -> Result<LessonOutput, AppError> {
        let (provider_id, model) = match model_override {
            Some((provider_id, model)) => (provider_id.to_owned(), model.to_owned()),
            None => resolve_default_model(&self.deps.provider_repo, &self.deps.provider_model_repo)
                .await
                .ok_or_else(|| {
                    AppError::Conflict(
                        "lesson content generation unavailable: no enabled provider/model is configured"
                            .into(),
                    )
                })?,
        };
        let provider_id: ProviderId = ProviderId::parse(provider_id)
            .map_err(|error| AppError::BadRequest(format!("invalid provider id: {error}")))?;
        let cfg = resolve_provider_config(
            &self.deps.provider_repo,
            &self.deps.provider_model_repo,
            &self.deps.encryption_key,
            provider_id.as_str(),
            &model,
            &self.deps.workspace,
        )
        .await?;
        let provider: Arc<dyn LlmProvider> = create_provider(&cfg);
        // 冒烟瞬断注入面（e4）：装饰钩子先包（flaky 故障在计数层的内圈——
        // 被掐断的调用也如实计入调用数，与 CountingCompleter 的 Err 口径一致）。
        let provider: Arc<dyn LlmProvider> = match self.provider_decorator.as_ref() {
            Some(decorator) => decorator(provider),
            None => provider,
        };
        // 相位耗时读数（e1）：上下文携带任务注册表的用量句柄时，经计数包装
        // 上报每次 LLM 调用（调用数 + 输入/输出字符量），finish 行聚合落档。
        let provider: Arc<dyn LlmProvider> = match context.usage.as_ref() {
            Some(usage) => Arc::new(crate::learning_loop::CountingProvider {
                inner: provider,
                usage: usage.clone(),
            }),
            None => provider,
        };
        tracing::info!(
            course = %context.course_title,
            lesson = %context.lesson_title,
            provider = provider_id.as_str(),
            model = %model,
            resumed = resume_draft.is_some(),
            "lesson content generation start"
        );

        // The two slots are shared with the tool handlers: the draft the
        // model opened and the lesson output `ls_finish` published. On
        // resume the draft slot presets with the surviving draft — the
        // model never spends a round re-bootstrapping.
        let (user_text, draft_slot) = match resume_draft.as_ref() {
            Some(draft_id) => {
                let view = self.service.inspect_lesson_draft(draft_id)?;
                let previous_rounds = self
                    .round_logs
                    .lock()
                    .ok()
                    .and_then(|mut logs| logs.remove(draft_id))
                    .unwrap_or_default();
                (
                    compose_resume_user_text(context, &json_compact(&view), &previous_rounds),
                    Some(draft_id.clone()),
                )
            }
            None => (lesson_user_text(context), None),
        };
        // 首轮观测（冒烟读数面）：system/user 在模型调用前落档；生产恒 None。
        if let Some(observer) = self.prompt_observer.as_ref() {
            observer(serde_json::json!({
                "ts": nomifun_common::now_ms(),
                "method": "lesson_agent_turn",
                "ok": true,
                "system": GENERATE_LESSON_AGENT_SYSTEM.as_str(),
                "user": user_text,
            }));
        }
        let ctx = Arc::new(LoopContext {
            service: Arc::clone(&self.service),
            context: context.clone(),
            draft_slot: Arc::new(Mutex::new(draft_slot)),
            published_slot: Arc::new(Mutex::new(None)),
            channel: LoopChannel::new(WIRE, BUDGETS.generate_max_rounds),
            math_verify: Arc::new(MathVerifyCounters::default()),
            stored_writes: Arc::new(Mutex::new(StoredWrites::default())),
        });

        match tokio::time::timeout(
            std::time::Duration::from_secs(BUDGETS.timeout_secs),
            run_loops(
                self,
                provider,
                &model,
                &user_text,
                Arc::clone(&ctx),
                BUDGETS.round_tokens,
            ),
        )
        .await
        {
            Ok(Ok(output)) => {
                // 发布成功：会话结束，清掉可能残留的轮次日志（run_loops 的
                // 发布点已清过，这里是幂等兜底；草稿与续跑映射由
                // finish_lesson_draft / 落库路径负责清理）。
                if let Some(draft_id) = ctx.draft_slot.lock().ok().and_then(|slot| slot.clone()) {
                    if let Ok(mut logs) = self.round_logs.lock() {
                        logs.remove(&draft_id);
                    }
                }
                ctx.log("session_end", serde_json::json!({
                    "ok": true,
                    "activities": output.activities.len(),
                    "estimated_minutes": output.estimated_minutes,
                }));
                // 发布观测（冒烟读数面）：超容自报与 est 的实际产物。
                if let Some(observer) = self.prompt_observer.as_ref() {
                    observer(lesson_publish_observation_frame(&output, &ctx.math_verify));
                }
                Ok(output)
            }
            Ok(Err(error)) => {
                // 失败保留轮次日志：草稿仍在 TTL 内时重试即续跑。
                if let Some(draft_id) = ctx.draft_slot.lock().ok().and_then(|slot| slot.clone()) {
                    if let Ok(mut logs) = self.round_logs.lock() {
                        logs.insert(draft_id, ctx.channel.round_log_snapshot());
                    }
                }
                ctx.log("session_end", serde_json::json!({
                    "ok": false,
                    "error": error.to_string(),
                }));
                Err(error)
            }
            Err(_) => {
                let mut message =
                    format!("lesson content agent timed out after {}s", BUDGETS.timeout_secs);
                if let Some(draft_id) =
                    ctx.draft_slot.lock().ok().and_then(|slot| slot.clone())
                {
                    if let Ok(mut logs) = self.round_logs.lock() {
                        logs.insert(draft_id.clone(), ctx.channel.round_log_snapshot());
                    }
                    match self.service.audit_lesson_draft(&draft_id) {
                        Ok(audit) => {
                            message.push_str(&format!(
                                "\ndraft {draft_id} survives（可续跑，重试即接续本进度）; its audit state:\n{audit}"
                            ));
                        }
                        Err(_) => {
                            message.push_str(&format!(
                                "\ndraft {draft_id} survives（可续跑，重试即接续本进度）; audit unavailable"
                            ));
                        }
                    }
                }
                ctx.log("session_end", serde_json::json!({
                    "ok": false,
                    "error": "timeout",
                }));
                // 显式超时 = 可重试失败（e4 错误两桶：judge 90s 形态的移植）。
                // tokio 超时在 await 点丢弃管线 future——在途流连接随之断开，
                // 「线程分离不可撤销，只弃其果」的 async 原生等价物；草稿与
                // 轮次日志已留档，门面重试环把本错误归可重试桶接续。
                Err(AppError::Timeout(message))
            }
        }
    }
}

impl LiveLessonContentAgentEngine {
    /// 发布成功后的会话收尾：清掉该草稿可能残留的轮次日志（发布后草稿
    /// 已被 finish 门移除，续跑映射也随之清除——日志没有存在意义）。
    fn clear_round_logs(&self, ctx: &LoopContext) {
        if let Some(draft_id) = ctx.draft_slot.lock().ok().and_then(|slot| slot.clone()) {
            if let Ok(mut logs) = self.round_logs.lock() {
                logs.remove(&draft_id);
            }
        }
    }
}

/// 统一外壳（[`FlowCycle`]）的课时内容插头：分节契约（ADR-0002）由提示词
/// 与 ls_* 工具面承载（旧单篇文档契约同为合法工具）；发布在任一轮完成时
/// 清轮次日志；错误透传、错误优先于发布——历史顺序，显式保留（ADR-0004）。
#[async_trait::async_trait]
impl FlowCycle for LiveLessonContentAgentEngine {
    type Ctx = LoopContext;
    type Output = LessonOutput;

    fn name(&self) -> &'static str {
        "lesson content"
    }

    fn budgets(&self) -> LoopBudgets {
        BUDGETS
    }

    fn wire(&self) -> WireConfig {
        WIRE
    }

    fn generate_system(&self) -> &'static str {
        GENERATE_LESSON_AGENT_SYSTEM.as_str()
    }

    fn repair_system(&self) -> &'static str {
        REPAIR_LESSON_AGENT_SYSTEM
    }

    fn tools(&self, ctx: Arc<LoopContext>, repair_face: bool) -> Vec<OneShotTool> {
        lesson_content_tools(ctx, repair_face)
    }

    fn history_compaction(&self, ctx: &Arc<LoopContext>) -> Option<Arc<HistoryCompaction>> {
        // u06：仅开关 on 时装配压缩钩子（缺省 off = None = 历史逐字节全量
        // 重发）。语义契约（钩子构造处集中说明，见 `lesson_history_compaction`）。
        if nomifun_learning::lesson_history_compact_enabled() {
            Some(lesson_history_compaction(Arc::clone(&ctx.stored_writes)))
        } else {
            None
        }
    }

    fn repair_opening_context(&self, ctx: &Arc<LoopContext>) -> Option<String> {
        // u07 机械（修复轮带课时上下文，不挂开关——票面正确性/上下文补
        // 全）：修复轮是全新对话，开场注入 lesson_user_text 同源上下文
        // （概念面/档位/教学范围在审计报告里不可达），审计报告随后
        // （run_loops 的 [`compose_repair_user`] 组装顺序）。
        Some(lesson_user_text(&ctx.context))
    }

    async fn finish(&self, ctx: &LoopContext, draft_id: &str) -> Result<LessonOutput, AppError> {
        // w1：verify_math 留痕随发布面穿出（finish 门后 warning，不挡发布）。
        // w1 评审修复口径：传未复核修正数（Fail 后无复核 Pass）——自修正
        // 会话零 warning（Q1 失败语义：自修正不是失败），累计 failed 只进
        // 观测帧读数。
        self.service
            .finish_lesson_draft(draft_id, ctx.math_verify.unresolved_u32())
    }

    fn revision(&self, _ctx: &LoopContext, draft_id: &str) -> Result<u64, AppError> {
        Ok(self.service.inspect_lesson_draft(draft_id)?.revision as u64)
    }

    fn repair_audit(
        &self,
        ctx: &LoopContext,
        draft_id: &str,
        round: usize,
    ) -> Result<String, AppError> {
        audit_report(ctx, draft_id, nomifun_learning::phase::REPAIR, round)
    }

    fn exhausted_audit(&self, _ctx: &LoopContext, draft_id: &str) -> Result<String, AppError> {
        self.service.audit_lesson_draft(draft_id)
    }

    fn publish_frame(
        output: &LessonOutput,
        loop_label: &str,
        round: Option<usize>,
    ) -> serde_json::Value {
        // visual 声明分布随发布帧披露（ADR-0008）：整课「无」独占即可疑的
        // 纯文字课时，事后定位同事反馈这类问题时有据可查。
        let visuals = nomifun_learning::visual_distribution(&output.sections);
        let mut frame = serde_json::json!({
            "phase": loop_label,
            "activities": output.activities.len(),
            "visuals": visuals,
            // f3 viz lint 事件计数（op 打回 + 终检剔除累计）：smoke
            // viz_lint_events 读数的数据源；0 = 生成期零 lint 介入。
            "viz_lint_events": output.viz_lint_events,
        });
        if let Some(round) = round {
            frame["round"] = serde_json::json!(round);
        }
        frame
    }

    fn on_published(&self, ctx: &LoopContext) {
        self.clear_round_logs(ctx);
    }

    fn idle_nudge_actions(&self) -> &'static str {
        "ls_set_document / ls_patch_activities 执行修复动作，或以 ls_finish 尝试发布"
    }

    fn map_loop_error(&self, _ctx: &LoopContext, error: AppError) -> AppError {
        error
    }

    fn publish_before_error(&self) -> bool {
        false
    }

    fn missing_draft_texts(&self) -> (&'static str, &'static str) {
        (
            "no draft created (ls_start never called)",
            "lesson content agent finished without creating a draft (ls_start was never called)",
        )
    }

    fn slot_poisoned(&self) -> AppError {
        AppError::Internal("lesson draft slot poisoned".into())
    }

    fn draft_slot(ctx: &LoopContext) -> &Arc<Mutex<Option<String>>> {
        &ctx.draft_slot
    }

    fn published_slot(ctx: &LoopContext) -> &Arc<Mutex<Option<LessonOutput>>> {
        &ctx.published_slot
    }
}

// ── Shared loop state ──────────────────────────────────────────────────────

/// u06 落库写记录的种类（压缩钩子的状态面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoredWriteKind {
    /// 生成/修复面的 `ls_set_section_body`（key = section_key）。
    SectionBody,
    /// 修复面的 `ls_set_document`（key = "document"）。
    Document,
}

/// 已成功落库的写入流水（u06 压缩钩子的判定面）：按写入顺序记录 (种类,
/// key, 非空字符数)。`ls_inspect` 的读数与钩子占位符共用这份字符数——
/// 认知与执法单源（handler 落库后从 inspect 视图回读，不自行复算）。
#[derive(Debug, Default)]
pub(crate) struct StoredWrites {
    entries: Vec<(StoredWriteKind, String, usize)>,
}

impl StoredWrites {
    fn record(&mut self, kind: StoredWriteKind, key: impl Into<String>, chars: usize) {
        self.entries.push((kind, key.into(), chars));
    }

    /// 某 (种类, key) 最近一次落库的字符数；未落库 = None（钩子不改写）。
    fn stored_chars(&self, kind: StoredWriteKind, key: &str) -> Option<usize> {
        self.entries
            .iter()
            .rev()
            .find(|(entry_kind, entry_key, _)| *entry_kind == kind && entry_key == key)
            .map(|(_, _, chars)| *chars)
    }

    /// 最近一次写入是否为该 (种类, key)——那一节保留原文（紧邻的修改仍
    /// 有上下文，票面语义）。
    fn is_latest(&self, kind: StoredWriteKind, key: &str) -> bool {
        self.entries.last().is_some_and(|(entry_kind, entry_key, _)| {
            *entry_kind == kind && entry_key == key
        })
    }
}

/// Everything the tool handlers need, captured once per generation. The two
/// slots are the only mutable cross-round state: which draft is active and
/// which lesson output (if any) was published by `ls_finish`. 线上翻译与轮
/// 次日志由统一外壳的 [`LoopChannel`] 承载（差异表见 [`WIRE`]）。
pub(crate) struct LoopContext {
    service: Arc<LearningService>,
    context: LessonGenerationContext,
    draft_slot: Arc<Mutex<Option<String>>>,
    published_slot: Arc<Mutex<Option<LessonOutput>>>,
    channel: LoopChannel,
    /// verify_math 调用计数槽（w1/A5）：两面工具共享（构造一次、闭包捕获
    /// Arc）——发布观测帧为唯一汇点。
    math_verify: Arc<MathVerifyCounters>,
    /// u06 已落库写记录（压缩实验）：写侧工具在落库成功后记账、压缩钩子
    /// 读账。开关 off 时记录照记（几个字节的开销）、钩子不装配。
    stored_writes: Arc<Mutex<StoredWrites>>,
}

/// verify_math 调用计数（w1/A5）：calls/passed/failed/unresolved 四个原子
/// 槽。观测面而已——不进 WS publish_ok 帧（`publish_frame` 是拿不到
/// LoopContext 的 trait 关联函数，不加签名）、不走 LessonDraft（verify_math
/// 不落库，无载体）；任何计数丢失不影响生成正确性。
///
/// `unresolved`（w1 评审修复）：未复核修正的失败数——Fail 置增、任一 Pass
/// 清零（R14 自修改协议的机械形状：模型按 verdict 改正后重新提交核验 =
/// 复核闭环）。它是 ls_finish 传给发布面留痕的计数：自修正会话（核验失败
/// →打回→复核过→过闸，map.md 验证总纲第 1 条）零 warning；无视 Fail 硬写
/// 错数的会话才有 `math_verify_failed` warning。v0 粗粒度：任一 Pass 视为
/// 闭环在走，按量精确追踪是后置议题。累计 `failed` 只进观测帧读数（smoke
/// read-out 行），不进任何硬门（Q1 失败语义：自修正不是失败）。
#[derive(Default)]
pub(crate) struct MathVerifyCounters {
    calls: AtomicUsize,
    passed: AtomicUsize,
    failed: AtomicUsize,
    unresolved: AtomicUsize,
    /// u05 批量调用次数（claims 数组形式的 verify_math 调用数；单 claim
    /// 不计）。纯读数面——A/B 观测点（on 臂期望 >0，calls/batches 比即
    /// 每批 claim 数）；off 臂恒 0。
    batches: AtomicUsize,
    /// u07 失败条目留痕（发布前数学失败可见的回给面）：每次 Fail 的
    /// verdict 原文（reason/computed/counterexample）。软上限防病态会话
    /// 无界增长——超出后新失败只计数不逐条留痕（打回条目截断声明）。
    failures: Mutex<Vec<ClaimVerdict>>,
    /// u07 修正机会闸（机械，防死循环）：`claim_fix_nudge` 首次 true 后
    /// 恒 false——发布前打回至多一次。
    fix_nudge_used: AtomicBool,
}

/// u07 失败条目留痕的软上限（打回条目数）。
const MATH_FAILURE_ENTRIES_CAP: usize = 32;

impl MathVerifyCounters {
    /// 按核验结论记一笔（op handler 每次 verify_claim 后调用）。
    fn record(&self, verdict: &ClaimVerdict) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        match verdict {
            ClaimVerdict::Pass { .. } => {
                self.passed.fetch_add(1, Ordering::Relaxed);
                // 复核闭环：任一 Pass 即清空未修正失败（见 struct 文档）。
                self.unresolved.store(0, Ordering::Relaxed);
            }
            ClaimVerdict::Fail { .. } => {
                self.failed.fetch_add(1, Ordering::Relaxed);
                self.unresolved.fetch_add(1, Ordering::Relaxed);
                // u07 失败条目留痕（软上限内逐条；超额只计数）。
                if let Ok(mut failures) = self.failures.lock() {
                    if failures.len() < MATH_FAILURE_ENTRIES_CAP {
                        failures.push(verdict.clone());
                    }
                }
            }
        }
    }

    /// u07：领取「发布前修正机会」——首次调用返回 true 并置闸（此后恒
    /// false）。机械防死循环：打回至多一次。
    fn claim_fix_nudge(&self) -> bool {
        !self.fix_nudge_used.swap(true, Ordering::SeqCst)
    }

    /// u07：失败条目原文列表（verdict 序列化——reason/computed/
    /// counterexample 随打回可见）。空 = 无失败可回。
    fn failures_json(&self) -> Vec<serde_json::Value> {
        self.failures
            .lock()
            .map(|failures| {
                failures
                    .iter()
                    .map(|verdict| serde_json::to_value(verdict).unwrap_or(serde_json::Value::Null))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// u05：一次批量调用记一笔（claims 数组通过形状校验后；退稿不记）。
    fn record_batch(&self) {
        self.batches.fetch_add(1, Ordering::Relaxed);
    }

    /// 发布观测帧的计数形状（smoke `math_verify read-out` 与判据⑧的取数
    /// 面）：failed = 会话累计（读数面）；unresolved = 未复核修正（finish
    /// findings 口径，判据⑧硬门）；batches = u05 批量调用次数（读数面，
    /// off 臂恒 0）。
    fn snapshot_json(&self) -> serde_json::Value {
        serde_json::json!({
            "calls": self.calls.load(Ordering::Relaxed),
            "passed": self.passed.load(Ordering::Relaxed),
            "failed": self.failed.load(Ordering::Relaxed),
            "unresolved": self.unresolved.load(Ordering::Relaxed),
            "batches": self.batches.load(Ordering::Relaxed),
        })
    }

    /// 未复核修正失败数穿出发布面（`ls_finish` → finish 门后 warning 留痕）。
    /// usize 饱和到 u32::MAX——计数溢出只影响读数形状，不 panic。
    fn unresolved_u32(&self) -> u32 {
        u32::try_from(self.unresolved.load(Ordering::Relaxed)).unwrap_or(u32::MAX)
    }
}

impl LoopContext {
    /// Mirror loop events onto the WebSocket progress stream (the shared
    /// progress channel — no session files). Best-effort and never fails
    /// the caller.
    fn log(&self, event: &str, fields: serde_json::Value) {
        self.channel.emit(&self.service, event, &fields);
    }

    fn require_draft(&self) -> Result<String, String> {
        self.draft_slot
            .lock()
            .map_err(|_| "internal draft slot lock failed".to_owned())?
            .clone()
            .ok_or_else(|| "没有活动的草稿——请先调用 ls_start".to_owned())
    }
}

impl LoopEventSink for LoopContext {
    fn log(&self, event: &str, fields: serde_json::Value) {
        LoopContext::log(self, event, fields);
    }
}

/// 发布观测帧（冒烟读数面，`lesson_agent_publish` 行）——smoke 实际读的
/// 汇点。f3 读数修复（票 w1/Resolution 2）：viz_lint_events 此前只进 WS
/// `publish_ok` 帧（[`FlowCycle::publish_frame`]），calls.jsonl 的这一行
/// 从未带上它——读数结构性恒 0；本帧补上该字段，`output` 在构造点
/// （`run_generation` 的 `Ok(Ok(output))` 臂）已在作用域内。w1 起同帧带
/// math_verify 计数（A5：该帧有 ctx，是计数唯一可穿出的观测汇点）。
fn lesson_publish_observation_frame(
    output: &LessonOutput,
    math_verify: &MathVerifyCounters,
) -> serde_json::Value {
    serde_json::json!({
        "ts": nomifun_common::now_ms(),
        "method": "lesson_agent_publish",
        "ok": true,
        "capacity_note": output.capacity_note,
        "estimated_minutes": output.estimated_minutes,
        "viz_lint_events": output.viz_lint_events,
        // scope 边界词面 findings 计数（absorb-8 z03，报告-only）：smoke
        // `scope_findings read-out` 的数据源；0 = 零命中或旧档零扫描。
        "scope_findings": output.scope_findings,
        "math_verify": math_verify.snapshot_json(),
    })
}

/// Live audit snapshot for the repair loop: fetches the full findings text
/// (the model's repair basis) and logs the `audit` progress frame with the
/// severity counts and up to five danger messages (the UI's audit badges).
fn audit_report(
    ctx: &LoopContext,
    draft_id: &str,
    loop_label: &str,
    round: usize,
) -> Result<String, AppError> {
    let report = ctx.service.audit_lesson_draft(draft_id)?;
    let findings = ctx.service.inspect_lesson_draft(draft_id)?.findings;
    let count = |severity: &str| {
        findings
            .iter()
            .filter(|finding| finding.severity == severity)
            .count()
    };
    let top: Vec<String> = findings
        .iter()
        .filter(|finding| finding.severity == "danger")
        .map(|finding| finding.message.clone())
        .take(5)
        .collect();
    ctx.log(
        "audit",
        serde_json::json!({
            "phase": nomifun_learning::phase::AUDIT,
            "loop": loop_label,
            "round": round,
            "danger": count("danger"),
            "warning": count("warning"),
            "info": count("info"),
            "top": top,
        }),
    );
    Ok(report)
}

/// The generation loop's user turn: the lesson coordinates, purpose,
/// bridging target, and the grounding — the cited source file read in full
/// (主引用) plus the retrieval supplements (补充片段) for the kb flow, the
/// course brief for the description flow. Learning-graph nodes
/// (`context.graph` = Some) swap the outline/brief sections for
/// graph-scoped sections: goal, the degree statement, the concept-web
/// prerequisite summary, and the downstream nodes (ADR-0009).
/// 图课时的「时长预算」行（k03）：开关关闭或无 est 时不注入。抽成纯函数
/// 让开/关两臂在同一进程内可测（env 是进程全局态，单测不碰）。
fn graph_budget_line(enabled: bool, est_minutes: Option<i64>) -> Option<String> {
    if !enabled {
        return None;
    }
    est_minutes.map(nomifun_learning::render_est_budget_line)
}

/// 图线是否挂 teach 概念（y03 的 `search_concepts` 挂载条件）：传统线
/// （graph = None）与零 teaches 节点都是 false——无概念体系就没有自查对象，
/// 不挂幽灵工具、也不出提示句。
fn graph_teaches_concepts(context: &LessonGenerationContext) -> bool {
    context
        .graph
        .as_ref()
        .is_some_and(|graph| !graph.teaches_tiers.is_empty())
}

/// 登记表自查提示（y03）：与工具挂载同一条件、同一句只出现一次；只读
/// 声明明示零写侧，避免模型把它当铸名/改别名入口。
const SEARCH_CONCEPTS_HINT: &str =
    "可用 `search_concepts` 自查概念是否在册、别名与近名；只读，不改登记表。\n";

fn lesson_user_text(context: &LessonGenerationContext) -> String {
    let mut text = String::new();
    text.push_str(&format!("课程：{}\n", context.course_title.trim()));
    let graph = context.graph.as_ref();
    match graph {
        Some(graph) => {
            text.push_str(
                "学习图节点（「本节点」是你要写的课时——只写它的范围，不越界讲后续节点，不重复前置节点已覆盖的内容）：\n",
            );
            // 头两行（p02 C 空值行省略）：程度声明空值省行（真机 L4 空值
            // 行噪声）；学习目标同款缺席省略——known_names 省行先例同形。
            text.push_str(&nomifun_learning::render_graph_goal_degree_lines(
                &graph.goal,
                &graph.degree_statement,
            ));
            // 时长预算（k03，learnhub #154）：结构阶段估的节点尺寸锚——
            // 规划节容量与写作都对照它；allo 图节点列恒有值。
            if let Some(line) =
                graph_budget_line(nomifun_learning::est_budget_enabled(), graph.est_minutes)
            {
                text.push_str(&line);
            }
        }
        None if !context.outline_tree.is_empty() => {
            text.push_str(&format!(
                "课程完整目录（「本课时」是你要写的课时——只写它的范围，不越界讲后续课时，不重复相邻课时）：\n{}\n",
                context.outline_tree
            ));
        }
        None => {}
    }
    match graph {
        Some(graph) => {
            text.push_str(&format!(
                "节点（{}/{}）：{}\n节点定位：{}\n",
                context.lesson_index + 1,
                context.total_lessons,
                context.lesson_title.trim(),
                context.purpose.trim()
            ));
            // 声明面骨架行（absorb-8 z03，learnhub #205 契约 v2.17）：节点
            // 定位行后插「创建意图/教学范围」两行——声明优先条款的输入面；
            // 任一字段缺席（旧档）省该行、两字段皆缺席整段省略（旧行为零
            // 变化）。行格式逐字固定（真机冒烟机械 grep 面）。
            if let Some(line) = nomifun_learning::render_intent_scope_lines(
                graph.intent.as_deref(),
                graph.scope.as_deref(),
            ) {
                text.push_str(&line);
            }
            // teaches 档位行（D4，意图画像深度角）：此前写作纪律「档位决定
            // 内容形态」引用模型读不到的值——本行补上输入面；空 teaches 整行
            // 省略。
            if let Some(line) = nomifun_learning::render_teaches_tier_line(&graph.teaches_tiers) {
                text.push_str(&line);
            }
            if graph.prerequisite_path.is_empty() {
                text.push_str("本节点没有前置——它是学习图的起点，从零讲起。\n");
            } else if let Some(block) =
                nomifun_learning::render_prerequisite_path_block(&graph.prerequisite_path)
            {
                // p02 B 双头合并：单一块头吸收「前置契约」句（真机 L11-12
                // 两头并一），档位标尺括注由前置段保留一次。
                text.push_str(&block);
                text.push('\n');
            }
            if graph.upcoming_nodes.is_empty() {
                text.push_str("本节点没有后续节点——结尾句做整个学习图的收束。\n");
            } else {
                text.push_str(&format!(
                    "后续节点（结尾句要为它们做衔接铺垫，但不要展开其内容）：\n{}\n",
                    graph.upcoming_nodes
                ));
            }
            // 课程已知概念（Known Names，#177 输入面）：工作词边界的指称面
            // ——构件词边界/取材池条款的名单行；空名单（图空且无声明）整行
            // 省略（受众行省行先例同形）。
            if let Some(names) = &graph.known_names {
                if let Some(line) = nomifun_learning::render_known_names_line(names) {
                    text.push_str(&line);
                }
            }
            // 先例摘要（#180）：已建立语境（概念 ← 先前教学点 + 先行节标题）
            // 的生成侧视野；无先例整块省略。
            if let Some(block) = nomifun_learning::render_precedent_block(&graph.precedents) {
                text.push_str(&block);
            }
            // 概念面（j07 读侧投影，图节点路径专属）：首现/非首现 + 引用名
            // + 活跃别名池 + 概念登场纪律（R2/R5 条款只落此处）。
            if let Some(faces) = &graph.concept_faces {
                text.push_str(&format!("\n{}\n", faces.render_text()));
            }
            // 登记表自查提示（y03）：已知概念/档位行之后补一句——工具挂载
            // 条件的同一判定，传统线与零 teaches 节点不出现。
            if graph_teaches_concepts(context) {
                text.push_str(SEARCH_CONCEPTS_HINT);
            }
        }
        None => {
            // 目标（t1）：传统课时的目标字段——有目标时先立方向锚点，让每
            // 一节都服务于同一门课的意图；旧课程无目标，不出现此段。
            if let Some(goal) = context.goal.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
                text.push_str(&format!("课程目标（学习者要达成的整体目标）：{goal}\n"));
            }
            text.push_str(&format!(
                "模块：{}\n课时（{}/{}）：{}\n课时目标：{}\n",
                context.module_title.trim(),
                context.lesson_index + 1,
                context.total_lessons,
                context.lesson_title.trim(),
                context.purpose.trim()
            ));
            if let Some(next) = context.next_lesson_title.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
                text.push_str(&format!("下一课：「{next}」——结尾句要衔接到它。\n"));
            } else {
                text.push_str("这是本模块最后一课——结尾句做本模块的收束。\n");
            }
        }
    }
    if !context.forbidden_concepts.trim().is_empty() {
        // 防超纲黑名单：传统课时已随概念契约下线（恒空）；学习图节点 =
        // 概念网反查的下游节点标题（ADR-0009）。
        text.push_str(&format!("\n{}\n", context.forbidden_concepts.trim()));
    }
    if !context.adjacent_context.is_empty() {
        text.push_str(&format!("\n{}\n", context.adjacent_context));
    }
    let grounding = context.grounding();
    if !grounding.is_empty() {
        // 多片段 grounding（t3）：主引用 = 大纲引用的 source 文件全文，补充
        // 片段 = 知识库相关文件扩读——逐片段给角色与出处（库/文件/章节），
        // 补充片段可溯源；锚定纪律：主引用是写作主线。
        text.push_str(
            "\n引用片段（文档与活动必须忠于它们——主引用是本课时引用的源文件全文，\
             补充片段是知识库中相关文件的扩读，引用其事实时注明该片段的库/文件/章节）：\n",
        );
        let total = grounding.len();
        for (index, excerpt) in grounding.iter().enumerate() {
            text.push_str(&format!(
                "\n--- {}（{}/{}）{} ---\n{}\n",
                excerpt.role_label(),
                index + 1,
                total,
                excerpt.provenance_label(),
                excerpt.text
            ));
        }
    } else if let Some(graph) = graph {
        // 无简报尾注（p02 C，真机 L33）：列项动态拼装，空值项省略——
        // 不再指向空白程度声明。
        text.push_str(&nomifun_learning::render_graph_no_brief_note(
            &graph.goal,
            &graph.degree_statement,
        ));
    } else {
        let brief = context.course_description.trim();
        if brief.is_empty() {
            text.push_str("\n课程简报为空：以课时标题与课时目标为准展开。\n");
        } else {
            text.push_str(&format!("\n课程简报（文档与活动必须忠于它）：\n{brief}\n"));
        }
    }
    text
}

/// The resume opening: the fresh-run user turn (lesson coordinates and
/// grounding) plus the surviving draft's live state and the previous
/// session's round logs — the model continues where it stopped instead of
/// rebuilding from scratch (learnhub「断点续跑跳过已 ready 节」的循环形
/// 式等价物：草稿状态注入开场，已写的节就在草稿里，重写即浪费)。
fn compose_resume_user_text(
    context: &LessonGenerationContext,
    draft_state_json: &str,
    previous_rounds: &[String],
) -> String {
    let mut text = lesson_user_text(context);
    text.push_str(&format!(
        "\n【上次会话进度（{} 轮后中断；接着此进度继续，不要重复已写好的节与题目——草稿状态里的 section_bodies 就是已落库的节，对它们再调用 ls_set_section_body 会被幂等跳过）】\n当前草稿状态：{}\n",
        previous_rounds.len(),
        draft_state_json
    ));
    // 门面重试环的反馈槽（e4）：上次尝试因瞬断中断时重试环写入——模型先
    // 知道发生了什么、该从哪继续，而不是对着无声的进度猜。
    if let Some(feedback) = context.retry_feedback.as_deref().filter(|f| !f.trim().is_empty()) {
        text.push_str(&format!("\n【重试反馈】{feedback}\n"));
    }
    for line in previous_rounds {
        text.push_str("\n- ");
        text.push_str(line);
    }
    text.push_str(
        "\n\n建议：先 ls_inspect 通读当前草稿核对进度，再继续未完成的节（每节一次 \
         ls_set_section_body），最后 ls_audit 自查、ls_finish 发布。",
    );
    text
}

// ── Tool set ───────────────────────────────────────────────────────────────

/// u06 占位符前缀（幂等判定锚）：已压缩的 body 都以它开头，钩子重复经过
/// 不再改写。
const STORED_BODY_PLACEHOLDER_PREFIX: &str = "（已落库";

/// u06 历史压缩钩子（`NOMI_LEARNING_LESSON_HISTORY_COMPACT` on 臂专用，
/// 由 [`FlowCycle::history_compaction`] 在开关 on 时装配）：每轮工具结果
/// 回灌后原地改写消息历史——**已成功落库**的 `ls_set_section_body` /
/// `ls_set_document` 的 body/document 参数替换为落库摘要占位符（节 key +
/// 非空字符数 + ls_inspect 取回指引），**最近一次写入**保留原文（紧邻的
/// 修改仍有上下文，票面语义）。
///
/// 语义契约（u06 高风险项的三道安全垫）：
/// - 只改历史视图，不改草稿——发布产物与 off 臂完全一致（A/B 只测历史
///   形状对生成质量的影响）；
/// - 只压记账面（[`StoredWrites`]）在案的 key；被拒 op 的调用体若其 key
///   已由后续成功写入在案，同样换占位符（历史以「该节最新落库状态」为
///   准，拒绝明细留在 ToolResult 里）；
/// - 幂等：占位符以「（已落库」开头，钩子重复经过不再改写。
/// 正文按需取回走 ls_inspect 的可选 section_key（同一开关的另一臂面）。
fn lesson_history_compaction(
    stored_writes: Arc<Mutex<StoredWrites>>,
) -> Arc<HistoryCompaction> {
    Arc::new(move |messages: &mut [nomi_types::message::Message]| {
        let Ok(writes) = stored_writes.lock() else {
            return;
        };
        for message in messages.iter_mut() {
            for block in message.content.iter_mut() {
                let nomi_types::message::ContentBlock::ToolUse { name, input, .. } = block
                else {
                    continue;
                };
                match name.as_str() {
                    "ls_set_section_body" => {
                        // key 转持有副本：占位符插值与 input 改写借用不交叠。
                        let Some(key) = input
                            .get("section_key")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                        else {
                            continue;
                        };
                        let Some(body) =
                            input.get("body").and_then(serde_json::Value::as_str)
                        else {
                            continue;
                        };
                        if body.starts_with(STORED_BODY_PLACEHOLDER_PREFIX)
                            || writes.is_latest(StoredWriteKind::SectionBody, &key)
                        {
                            continue;
                        }
                        let Some(chars) =
                            writes.stored_chars(StoredWriteKind::SectionBody, &key)
                        else {
                            continue;
                        };
                        if let Some(object) = input.as_object_mut() {
                            object.insert(
                                "body".to_owned(),
                                serde_json::json!(format!(
                                    "（已落库：{key}，{chars} 字。需要原文时调用 ls_inspect 并传 section_key）"
                                )),
                            );
                        }
                    }
                    "ls_set_document" => {
                        let Some(body) =
                            input.get("document").and_then(serde_json::Value::as_str)
                        else {
                            continue;
                        };
                        if body.starts_with(STORED_BODY_PLACEHOLDER_PREFIX)
                            || writes.is_latest(StoredWriteKind::Document, "document")
                        {
                            continue;
                        }
                        let Some(chars) =
                            writes.stored_chars(StoredWriteKind::Document, "document")
                        else {
                            continue;
                        };
                        if let Some(object) = input.as_object_mut() {
                            object.insert(
                                "document".to_owned(),
                                serde_json::json!(format!(
                                    "（已落库：单篇文档全文，{chars} 字。需要概览时调用 ls_inspect；重写走 ls_set_document）"
                                )),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    })
}

/// The `ls_*` whitelist. `repair_face` selects the loop's tool face: the
/// generation face carries `ls_start` (the repair loop must never re-scope
/// the draft) while the repair face additionally carries `ls_set_document`
/// for legacy single-document drafts. Generation must plan sections — the
/// single-document contract carries no visual promise at all, so exposing it
/// to fresh runs is a zero-visualization escape hatch (ADR-0008). An
/// unlisted tool name fails closed at the loop level.
fn lesson_content_tools(ctx: Arc<LoopContext>, repair_face: bool) -> Vec<OneShotTool> {
    let mut tools = Vec::with_capacity(10);
    if !repair_face {
        tools.push(ls_start(Arc::clone(&ctx)));
    } else {
        tools.push(ls_set_document(Arc::clone(&ctx)));
    }
    tools.push(ls_inspect(Arc::clone(&ctx)));
    tools.push(ls_set_section_manifest(Arc::clone(&ctx)));
    tools.push(ls_set_section_body(Arc::clone(&ctx), repair_face));
    tools.push(ls_patch_activities(Arc::clone(&ctx)));
    tools.push(ls_audit(Arc::clone(&ctx)));
    tools.push(ls_finish(Arc::clone(&ctx)));
    // verify_math（w1）：两面同备的无草稿只读 op——生成与修复轮都可以把
    // 正文数学翻译成 claim 实算核验；不触库、不占草稿（A3/A5）。
    tools.push(verify_math_tool(Arc::clone(&ctx)));
    // search_concepts（y03）：登记表只读自查——仅图线 ∧ 本节点 teaches
    // 非空时挂（传统线与零 teaches 节点无概念体系，不挂幽灵工具）；生成
    // 面与修复面同备，模型在撞名门失败前就能自己核对在册/别名/近名。
    if graph_teaches_concepts(&ctx.context) {
        tools.push(search_concepts_tool(Arc::clone(&ctx)));
    }
    tools
}

fn ls_start(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "ls_start".into(),
        description: "启动课时内容草稿：返回 draft_id、课时上下文与当前审计状态。这是你的第一个工具调用；幂等——已有活动草稿时直接返回现有草稿。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                if let Some(existing) = ctx
                    .draft_slot
                    .lock()
                    .map_err(|_| "ls_start: 内部锁故障".to_owned())?
                    .clone()
                {
                    let view = ctx
                        .service
                        .inspect_lesson_draft(&existing)
                        .map_err(|error| error.to_string())?;
                    return Ok(format!("已有活动草稿（ls_start 幂等返回）：{}", json_compact(&view)));
                }
                let view = ctx
                    .service
                    .create_lesson_draft(ctx.context.clone())
                    .map_err(|error| error.to_string())?;
                *ctx.draft_slot
                    .lock()
                    .map_err(|_| "ls_start: 内部锁故障".to_owned())? = Some(view.draft_id.clone());
                Ok(format!("草稿已创建：{}", json_compact(&view)))
            }
        }),
    }
}

/// ls_inspect 的基线描述（u06 off 臂 = 本串逐字回退；on 臂尾部追加取回
/// 通道句）。改动基线文案必须同步更新 off 臂回归测试里的逐字副本。
const LS_INSPECT_BASELINE_DESCRIPTION: &str = "当前草稿概览：文档字符数与章节清单、活动清单（position/kind/prompt 摘要）、estimated_minutes、审计 findings。每次修改前后调用，保持全局认知；update/remove 活动的 position 以这里为准。";

/// u06 取回通道句（on 臂追加在 ls_inspect 描述尾部；off 臂零出现）。
const LS_INSPECT_RETRIEVE_NOTE: &str = "传 section_key 时返回该节已落库的完整正文（历史被压缩后按需取回原文）。";

fn ls_inspect(ctx: Arc<LoopContext>) -> OneShotTool {
    // u06：压缩实验臂给 ls_inspect 加原文取回面（可选 section_key）；
    // off 臂描述/schema/行为逐字回基线（概览视图，多余字段照旧宽容）。
    let compact = nomifun_learning::lesson_history_compact_enabled();
    let description = if compact {
        format!("{LS_INSPECT_BASELINE_DESCRIPTION}{LS_INSPECT_RETRIEVE_NOTE}")
    } else {
        LS_INSPECT_BASELINE_DESCRIPTION.to_owned()
    };
    let mut input_schema = serde_json::json!({ "type": "object", "properties": {} });
    if compact {
        input_schema["properties"]["section_key"] = serde_json::json!({
            "type": "string",
            "description": "可选：节 key（清单里的 s1、s2……）。传入时返回该节的完整正文"
        });
    }
    OneShotTool {
        name: "ls_inspect".into(),
        description,
        input_schema,
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                // u06 取回通道（on 臂）：section_key 非空传入 → 返回该节
                // 完整正文（历史压缩后的按需取回面）。off 臂不进此分支。
                if compact {
                    if let Some(section_key) = input
                        .get("section_key")
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|key| !key.is_empty())
                    {
                        let body = ctx
                            .service
                            .lesson_draft_section_body(&draft_id, section_key)
                            .map_err(|error| error.to_string())?;
                        let chars =
                            body.chars().filter(|c| !c.is_whitespace()).count();
                        return Ok(json_compact(&serde_json::json!({
                            "section_key": section_key,
                            "chars": chars,
                            "body": body,
                        })));
                    }
                }
                let view = ctx
                    .service
                    .inspect_lesson_draft(&draft_id)
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&view))
            }
        }),
    }
}

fn ls_set_section_manifest(ctx: Arc<LoopContext>) -> OneShotTool {
    // 契约数值从 owner 表格渲染（ADR-0002 追加决策：禁止手写字面量）。
    let section_ranges = section_range_rules();
    let visual_menu = visual_menu_text();
    let visual_enum: Vec<&str> = VISUAL_OPTIONS.to_vec();
    // capacity_note 语义面（k03/#178）：on 臂 = 意图超载信号（先砍雄心再
    // 砍内容，收敛不住必写指名原因）；off 臂 = 旧超容自报措辞（双臂回退）。
    let (capacity_tool_note, capacity_schema_note) = if nomifun_learning::est_budget_enabled() {
        (
            "可选 capacity_note：节点意图超载信号——先砍雄心再砍内容（减题、并节、降示例密度，teaches 覆盖不可砍）仍保不住「时长预算」时必写、指名超载原因（上游 est 复议的信号）；预算内收敛则整键省略。",
            "节点意图超载原因（先砍雄心再砍内容仍超预算时必写；预算内收敛省略；缺席合法，永不影响发布）",
        )
    } else {
        (
            "可选 capacity_note：节清单定稿后对照「时长预算」粗估明显超容时如实写一句（不超容不写）。",
            "可选超容自报：对照「时长预算」粗估明显超容时如实写一句；不超容留空（缺席合法，永不影响发布）",
        )
    };
    OneShotTool {
        name: "ls_set_section_manifest".into(),
        description: format!(
            "规划课时的分节清单（整组替换）：sections 数组，每节带 section_key（s1、s2……）、kind（concept/example/demo/summary/practice）、title（带类型前缀）、points（一句话要点）、visual（可视化形态：{visual_menu}）。节数{section_ranges}；最后一节必须是练习节（恰好 1 个）；重规划时已写正文按 key 保留。{capacity_tool_note}"
        ),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "sections": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 8,
                    "description": "节清单（按学习顺序）",
                    "items": {
                        "type": "object",
                        "properties": {
                            "section_key": { "type": "string" },
                            "kind": { "type": "string", "enum": ["concept", "example", "demo", "summary", "practice"] },
                            "title": { "type": "string" },
                            "points": { "type": "string" },
                            "visual": { "type": "string", "enum": visual_enum }
                        },
                        "required": ["section_key", "kind", "title", "visual"]
                    }
                },
                "capacity_note": {
                    "type": "string",
                    "description": capacity_schema_note
                }
            },
            "required": ["sections"]
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let sections = input
                    .get("sections")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let sections: Vec<nomifun_learning::SectionPack> = serde_json::from_value(sections)
                    .map_err(|error| format!("ls_set_section_manifest: sections 必须是节清单数组——{error}"))?;
                // 超容自报（k03）：形状条款只查非空——空白/非字符串按缺席
                // 裁成 None，恶劣值不退稿。
                let capacity_note = input
                    .get("capacity_note")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                let report = ctx
                    .service
                    .patch_lesson_draft(
                        &draft_id,
                        vec![nomifun_learning::LessonOp::SetSectionManifest {
                            sections,
                            capacity_note,
                        }],
                    )
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&report))
            }
        }),
    }
}

/// 段级幂等跳过（e1 D1 第一步原语，生成面专属）：已落库正文的节再收到
/// `ls_set_section_body` 即短路返回「已在库」——不落 op、不 bump revision、
/// 模型写好的正文原样保留。这是 learnhub 备料「内容在→只补题库」的循环
/// 形式等价物：整课重试/续跑时已成功落库的节不重写，天然断点续跑。语义
/// 面向并发安全（e4 段写并行依赖此原语）：不同 section_key 落不同的草稿
/// 表项互不依赖；同 key 竞争时先写者赢、后到者拿「已在库」——跳过判定
/// 与落库虽非同一临界区，但草稿 op 本身按 key 整体替换，两种交错都收敛
/// 到「恰一份正文在库」，无半写状态。修复面**不带**跳过：审计打回的节
/// 必须能按报告重写（repair loop 的正当改稿通道）。
///
/// 跳过判定读 inspect 视图的 `section_bodies`（清单序已落库节 + 字符数）
/// ——模型在 ls_inspect 与续跑开场看到的是同一份读数，认知与执法单源。
fn ls_set_section_body(ctx: Arc<LoopContext>, repair_face: bool) -> OneShotTool {
    let prose_budgets = prose_budget_rules();
    let rewrite_clause = if repair_face {
        "每节可多次调用，重复调用即整节重写（修复面按审计报告改稿）。"
    } else {
        "每节调用一次；已写好正文的节再调用即幂等跳过（已落库的节不重写——断点续跑纪律），该节需要改动时走审计报告驱动的修复轮。"
    };
    OneShotTool {
        name: "ls_set_section_body".into(),
        description: format!(
            "写入一节正文：body 是该节的完整 Markdown，直接以 `## ` 标题行开头（照抄清单标题），不要 JSON、不要包裹围栏、节内禁止 ### 子标题。篇幅按节型（{prose_budgets}；demo 可视化为主；summary 要点清单；practice ≤{PRACTICE_BODY_TARGET_CHARS} 字不写题）。{rewrite_clause}"
        ),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "section_key": { "type": "string", "description": "节 key（清单中的 s1、s2……）" },
                "body": { "type": "string", "description": "该节正文（Markdown 全文，一次写入）" }
            },
            "required": ["section_key", "body"]
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let section_key = input
                    .get("section_key")
                    .and_then(|value| value.as_str())
                    .filter(|key| !key.trim().is_empty())
                    .ok_or_else(|| "ls_set_section_body: section_key 必须是非空字符串".to_owned())?
                    .to_owned();
                let body = input
                    .get("body")
                    .and_then(|value| value.as_str())
                    .filter(|body| !body.trim().is_empty())
                    .ok_or_else(|| "ls_set_section_body: body 必须是非空 Markdown 文本".to_owned())?
                    .to_owned();
                // 段级幂等跳过（生成面）：已落库的节原样保留，短路返回。
                if !repair_face {
                    let view = ctx
                        .service
                        .inspect_lesson_draft(&draft_id)
                        .map_err(|error| error.to_string())?;
                    if let Some(existing) = view
                        .section_bodies
                        .iter()
                        .find(|body| body.section_key == section_key)
                    {
                        return Ok(format!(
                            "section {section_key} 正文已在库（{} 非空字符）——幂等跳过，未重写（已落库的节不重写）。\
                             继续写未完成的节，或 ls_audit 自查后 ls_finish。",
                            existing.chars
                        ));
                    }
                }
                let last_section_key = section_key.clone();
                let report = ctx
                    .service
                    .patch_lesson_draft(
                        &draft_id,
                        vec![nomifun_learning::LessonOp::SetSectionBody { section_key, body }],
                    )
                    .map_err(|error| error.to_string())?;
                // u06 落库记账：op 被接纳时从 inspect 视图回读该节字符数
                // （与 ls_inspect/续跑开场同一读数源），压缩钩子据此换占位
                // 符。op 被拒（lint 打回等）不记账——历史原样留给模型看。
                if report.rejected.is_empty() {
                    let view = ctx
                        .service
                        .inspect_lesson_draft(&draft_id)
                        .map_err(|error| error.to_string())?;
                    if let Some(entry) = view
                        .section_bodies
                        .iter()
                        .find(|entry| entry.section_key == last_section_key)
                    {
                        if let Ok(mut writes) = ctx.stored_writes.lock() {
                            writes.record(
                                StoredWriteKind::SectionBody,
                                entry.section_key.clone(),
                                entry.chars,
                            );
                        }
                    }
                }
                Ok(json_compact(&report))
            }
        }),
    }
}

fn ls_set_document(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "ls_set_document".into(),
        description: "写入（或整体重写）学习文档（旧单篇契约，仅修复轮提供——新草稿请走分节）：一次给全文的纯 Markdown——直接以第一个 `## ` 标题行开头，以衔接下一课的收尾句结束；不要 JSON、不要包裹围栏。返回应用结果与最新审计 findings。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "document": { "type": "string", "description": "完整学习文档（Markdown 全文，一次写入）" }
            },
            "required": ["document"]
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let document = input
                    .get("document")
                    .and_then(|value| value.as_str())
                    .filter(|document| !document.trim().is_empty())
                    .ok_or_else(|| "ls_set_document: document 必须是非空 Markdown 文本".to_owned())?;
                let report = ctx
                    .service
                    .patch_lesson_draft(
                        &draft_id,
                        vec![LessonOp::SetDocument {
                            document: document.to_owned(),
                        }],
                    )
                    .map_err(|error| error.to_string())?;
                // u06 落库记账（修复面单篇契约）：与节写同款——接纳才记、
                // 字符数从 inspect 视图回读（单源）。
                if report.rejected.is_empty() {
                    let view = ctx
                        .service
                        .inspect_lesson_draft(&draft_id)
                        .map_err(|error| error.to_string())?;
                    if let Ok(mut writes) = ctx.stored_writes.lock() {
                        writes.record(StoredWriteKind::Document, "document", view.document_chars);
                    }
                }
                Ok(json_compact(&report))
            }
        }),
    }
}

/// `ls_patch_activities` 的基础描述（y06 前原文：off 臂逐字回退旧文本）。
const PATCH_ACTIVITIES_DESCRIPTION: &str = "批量应用活动操作（add_activity / update_activity / remove_activity / set_estimated_minutes），一次调用就是一个批次；操作按数组顺序执行，先执行的操作会改变后续操作的 position。返回每个操作的成功/拒绝明细 + 最新审计 findings。\n\n调用示例：\n{\"operations\": [\n  {\"op\": \"add_activity\", \"activity\": {\"kind\": \"single_choice\", \"prompt\": \"期权的本质是什么？\", \"options\": [\"权利\", \"义务\", \"债务\"], \"answer\": \"权利\", \"explanation\": \"买方持有的是权利\"}},\n  {\"op\": \"set_estimated_minutes\", \"minutes\": 15}\n]}\n\n字段规则（9 种题型）：single_choice 3-5 选项且 answer 恰等于其一；multi_choice 是 2+ 选项数组（顺序无关）；numeric 是数字（可带 tol 容差）；ordering 的 options 是打乱条目、answer 是正确顺序数组；matching 的 options 是左列、answer 是一一对应右列数组；open_question 与 reflection 的 answer 必须是 null；每题带 section_key 绑定来源节（跨节综合题用 \"general\"）。原四种：single_choice 3-5 个选项且 answer 恰等于其一；true_false 的 answer 是布尔；fill_in_blank 的 prompt 恰含一个 \"___\"、answer 是 1-3 个等价答案的数组且必须带 distractors；reflection 的 answer 必须是 null。每批 ≤10 个操作；写正文请用 ls_set_section_body。";

/// y06（learnhub #201）：工具面只补一句负例总闸——长菜单进系统提示词的
/// 活动契约，不塞工具描述（避免工具面注意力被五型菜单挤占）。
const PATCH_ACTIVITIES_DISTRACTOR_NOTE_CN: &str = "\n\n负例优先自然，无自然负例不硬造：选择题的诱惑项先找正文里现成的错误对照，正文没有该对照就不强行塑造。";

fn ls_patch_activities(ctx: Arc<LoopContext>) -> OneShotTool {
    let description = if nomifun_learning::content_discipline_v2_enabled() {
        format!("{PATCH_ACTIVITIES_DESCRIPTION}{PATCH_ACTIVITIES_DISTRACTOR_NOTE_CN}")
    } else {
        PATCH_ACTIVITIES_DESCRIPTION.to_owned()
    };
    OneShotTool {
        name: "ls_patch_activities".into(),
        description,
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "operations": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 10,
                    "description": "活动操作数组（按顺序执行）；每个元素是带 op 标签的对象，见工具描述",
                    "items": { "type": "object" }
                }
            },
            "required": ["operations"]
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                let operations =
                    input.get("operations").cloned().unwrap_or(serde_json::Value::Null);
                let ops: Vec<LessonOp> = serde_json::from_value(operations)
                    .map_err(|error| format!("ls_patch_activities: operations 必须是活动操作数组——{error}"))?;
                if ops.is_empty() {
                    return Err("ls_patch_activities: operations 不能为空".into());
                }
                let report = ctx
                    .service
                    .patch_lesson_draft(&draft_id, ops)
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&report))
            }
        }),
    }
}

fn ls_audit(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "ls_audit".into(),
        description: "完整确定性审计报告：每条 finding 的级别、类型与证据（文档长度/章节、活动数量与形状、概念绑定）。发布前的最终自查也用它；存在 danger 时 ls_finish 会被门禁拒绝。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                ctx.service
                    .audit_lesson_draft(&draft_id)
                    .map_err(|error| error.to_string())
            }
        }),
    }
}

fn ls_finish(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "ls_finish".into(),
        description: "发布草稿为最终课时内容。确定性审计门禁有最终决定权：存在 danger 级 findings 时发布被阻塞，返回完整阻塞报告（草稿保留，可继续修复）。只有 ls_audit 确认无 danger 时才调用。".into(),
        input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        handler: one_shot_handler(move |_input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let draft_id = ctx.require_draft()?;
                // u07 机械（发布前数学失败可见，不挂开关——票面正确性补
                // 全）：会话存在未复核修正失败时，**首次** ls_finish 不进
                // 门禁，把失败条目（verdict 原文：reason/computed/
                // counterexample）与修正指引回给模型；修正机会仅一次
                // （`claim_fix_nudge` 防死循环闸——模型无视打回时第二次
                // ls_finish 照常走门禁，未复核失败以 warning 留痕，基线
                // 语义不变）。干净会话零变化（unresolved = 0 不进此分支）。
                if ctx.math_verify.unresolved_u32() > 0
                    && ctx.math_verify.claim_fix_nudge()
                {
                    let failures = ctx.math_verify.failures_json();
                    return Err(format!(
                        "发布前数学失败可见：本会话 verify_math 有 {} 次失败未复核修正——\
                         请按失败条目以实算值修正正文数字（R14），必要时用 verify_math 复核闭环后重新 ls_finish。\
                         失败条目（verdict 原文，含 reason/computed/counterexample）：{}",
                        ctx.math_verify.unresolved_u32(),
                        json_compact(&failures),
                    ));
                }
                // w1：verify_math 留痕随发布面穿出（finish 门后 warning，
                // A3——不挡发布、不重放核验）。w1 评审修复口径：传未复核
                // 修正数（unresolved），自修正会话零 warning。
                match ctx
                    .service
                    .finish_lesson_draft(&draft_id, ctx.math_verify.unresolved_u32())
                {
                    Ok(output) => {
                        let chars =
                            output.summary.chars().filter(|c| !c.is_whitespace()).count();
                        *ctx.published_slot
                            .lock()
                            .map_err(|_| "ls_finish: 内部锁故障".to_owned())? =
                            Some(output.clone());
                        Ok(format!(
                            "课时内容已通过门禁：文档 {chars} 字符 / {} 个活动 / 预计 {} 分钟。",
                            output.activities.len(),
                            output.estimated_minutes
                        ))
                    }
                    Err(error) => Err(error.to_string()),
                }
            }
        }),
    }
}

/// verify_math 工具 op（w1/蓝图 4.2/A1/A2）：模型把正文数学翻译成 claim
/// JSON，核验器（nomifun-learning 单源）确定性执行。失败语义（A3）：
/// claim JSON 损坏 → Err（模型下一轮改 claim）；核验 Fail → **Ok**（
/// verdict JSON 内含 reason/computed/counterexample——核验失败是正常结论
/// 不是工具故障）。无任何路径触库；每次调用记入计数槽（观测面）。
///
/// u05 批量面（开关 `NOMI_LEARNING_VERIFY_BATCH`，显式 on 臂；开发期缺省
/// off 待 A/B）：入参在
/// 原单 claim 形状之外接受 `claims` 数组（≤[`VERIFY_MATH_BATCH_CAP`] 条/次，
/// 超限可读退稿），逐条核验、逐条返回 verdict 列表（沿用单条文案）。读数
/// 按 **claim** 计数口径不变（calls=条数），批量调用次数另记 batches。off
/// 臂（票面正优化协议）：描述/schema/条款逐字节回退基线，claims 形状走基线
/// 单 claim 解析路径。
///
/// 同步注记（w1 评审，记录不改）：本 description 与 input_schema 的字段
/// 说明三处复述了求值器规则（表达式文法/函数表/容差缺省）——求值器单源
/// 在 `nomifun_learning::verify_math`（`eval_expr` 文法与常量），同步靠人
/// 盯，改动文法时逐字核对这三处文案。
///
/// 基线描述单源（u05 off 臂 = 本串逐字回退；on 臂尾部追加批量句）。改动
/// 基线文案必须同步更新 off 臂回归测试里的逐字副本（那是独立事实源）。
const VERIFY_MATH_BASELINE_DESCRIPTION: &str = "数学断言核验器（确定性实算，可复现）：把正文里的数学翻译成 claim 提交核验。两类 claim——eval = 求值表达式并与 expected 比对（expected 缺席 = 只回读实算值）；identity = 等式两边在变量域采样数值比对（缺省 7 点，相对容差 1e-9）。返回 verdict JSON：pass / fail（fail 带 reason、computed、counterexample——据此修正正文数字，再提交复核）。凡可算量（和、均值、平方和、单位换算、开方）写入正文前先在这里实算。表达式规则：显式 *（不支持隐式乘法与 %）、+ - * / ^ 括号、sqrt/sin/cos/tan/ln/log/exp/abs、pi/e。";

/// u05 批量核验句（on 臂追加在工具描述尾部；off 臂零出现）。
const VERIFY_MATH_BATCH_NOTE: &str = "一节写完后，把本节全部可算量放进 claims 数组一次性提交核验（单次至多 12 条，逐条返回 verdict），不必逐条多次调用。";

/// 单次批量核验的 claim 上限（u05 票面：≤12 条/次，超限可读退稿）。
const VERIFY_MATH_BATCH_CAP: usize = 12;

fn verify_math_tool(ctx: Arc<LoopContext>) -> OneShotTool {
    // 契约与 handler 读同一份开关值（构造时捕获——工具面与行为面一致）。
    let batch = nomifun_learning::verify_batch_enabled();
    let description = if batch {
        format!("{VERIFY_MATH_BASELINE_DESCRIPTION}{VERIFY_MATH_BATCH_NOTE}")
    } else {
        VERIFY_MATH_BASELINE_DESCRIPTION.to_owned()
    };
    let mut input_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "kind": { "type": "string", "enum": ["eval", "identity"], "description": "claim 类型" },
            "expr": { "type": "string", "description": "eval：待求值表达式" },
            "expected": { "type": "number", "description": "eval：期望值（缺席 = 只回读实算值）" },
            "lhs": { "type": "string", "description": "identity：等式左边" },
            "rhs": { "type": "string", "description": "identity：等式右边" },
            "variables": { "type": "array", "items": { "type": "string" }, "description": "identity：采样变量名清单" },
            "tolerance": { "type": "number", "description": "相对容差（缺省 1e-9，绝对地板 1e-12）" },
            "samples": { "type": "integer", "description": "identity：采样点数（缺省 7）" }
        },
        "required": ["kind"]
    });
    if batch {
        input_schema["properties"]["claims"] = serde_json::json!({
            "type": "array",
            "maxItems": VERIFY_MATH_BATCH_CAP,
            "description": "批量核验：claim 对象数组（每项同单 claim 形状），逐条返回 verdict 数组",
            "items": { "type": "object" }
        });
    }
    OneShotTool {
        name: "verify_math".into(),
        description,
        input_schema,
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                // u05 批量路径（on 臂且入参带 claims）：逐条核验逐条计数
                // （calls 按 claim 计，覆盖口径不变），批量调用另记 batches。
                // 空数组与超上限可读退稿（不产结论、不记批量）。
                if batch {
                    if let Some(claims_value) = input.get("claims") {
                        let claims: Vec<MathClaim> =
                            serde_json::from_value(claims_value.clone()).map_err(|error| {
                                format!(
                                    "verify_math: claims 必须是 claim 数组（每项 {{kind:eval|identity,...}}）——{error}"
                                )
                            })?;
                        if claims.is_empty() {
                            return Err(
                                "verify_math: claims 是空数组——提交至少一条 claim，或改用单 claim 形状（kind/expr/... 在顶层）"
                                    .into(),
                            );
                        }
                        if claims.len() > VERIFY_MATH_BATCH_CAP {
                            return Err(format!(
                                "verify_math: 单次至多 {VERIFY_MATH_BATCH_CAP} 条 claim（收到 {} 条）——拆成多次提交，或只核验本节的可算量",
                                claims.len()
                            ));
                        }
                        ctx.math_verify.record_batch();
                        let verdicts: Vec<ClaimVerdict> =
                            claims.iter().map(verify_claim).collect();
                        for verdict in &verdicts {
                            ctx.math_verify.record(verdict);
                        }
                        return Ok(crate::loop_core::json_compact(&verdicts));
                    }
                }
                let claim: MathClaim = serde_json::from_value(input).map_err(|error| {
                    format!("verify_math: claim 必须是 {{kind:eval|identity,...}} 形状——{error}")
                })?;
                let verdict = verify_claim(&claim);
                ctx.math_verify.record(&verdict);
                Ok(crate::loop_core::json_compact(&verdict))
            }
        }),
    }
}

/// search_concepts 工具 op（y03）：概念登记表的只读自查通道——模型可按需
/// 核对某个名词/说法是否在册、有哪些活跃别名、有没有近名，不必等铸名门
/// 失败后才看到反馈。查询域 = 全局登记表（既有 ADR-0010 边界不变），
/// 四通道 exact / alias / nearname / fulltext 与排序去重单源在
/// `nomifun_learning` 服务层（本工具只解析入参并回包）。
///
/// 失败语义：零命中是正常结论（`Ok`，`not_found: true`）不是工具故障；
/// 入参错误（缺 query / detail 缺 concept_id / 非法 mode）与登记表错误转
/// 模型可读 Err 文本。**零写侧**：handler 只读，不铸名、不改别名、不建边。
fn search_concepts_tool(ctx: Arc<LoopContext>) -> OneShotTool {
    OneShotTool {
        name: "search_concepts".into(),
        description: "概念登记表的只读自查：查某个名词/说法是否在册、有哪些活跃别名、有没有近名，或按 concept_id 取完整定义与全部别名。只读，不改登记表（不铸名、不改别名、不建边）。册内未见不代表学科中不存在。".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "检索词（名词或说法）" },
                "mode": { "type": "string", "enum": ["search", "detail"], "default": "search", "description": "search=按查询词检索（默认）；detail=按 concept_id 取完整定义与全部活跃别名" },
                "concept_id": { "type": "string", "description": "detail 模式必填：取 search 命中条目里的 concept_id" }
            },
            "required": ["query"]
        }),
        handler: one_shot_handler(move |input| {
            let ctx = Arc::clone(&ctx);
            async move {
                let query = input
                    .get("query")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|query| !query.is_empty())
                    .ok_or_else(|| "search_concepts: query 必填且非空".to_owned())?;
                let mode = match input
                    .get("mode")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                {
                    None | Some("") | Some("search") => ConceptSearchMode::Search,
                    Some("detail") => ConceptSearchMode::Detail,
                    Some(other) => {
                        return Err(format!(
                            "search_concepts: mode 只支持 search|detail，得到「{other}」"
                        ));
                    }
                };
                let concept_id = input
                    .get("concept_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|concept_id| !concept_id.is_empty());
                let results = ctx
                    .service
                    .search_concepts(query, mode, concept_id)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(json_compact(&results))
            }
        }),
    }
}

// ── The tool loop (one-shot core, parameterized) ───────────────────────────
//
// `run_agent_loop` lives in `loop_core` — shared verbatim by the
// learning-graph, course-outline and lesson-content loops.

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_types::llm::{LlmEvent, ThinkingConfig};
    use nomi_types::message::{ContentBlock, StopReason};

    use nomifun_learning::{GraphLessonContext, LessonExcerpt};

    use crate::learning_loop::test_support::{
        ScriptedProvider, done, test_deps, test_service, tool_use,
    };
    use crate::loop_core::{GENERATE_REASONING_EFFORT, run_agent_loop};

    /// ≥800 non-whitespace characters across the three required sections —
    /// clears the document half of the audit gate.
    fn long_document() -> String {
        let body = "这是一个用于测试的完整段落，覆盖课时要求的知识点并且足够长。".repeat(10);
        format!("## 描述\n{body}\n## 例子\n{body}\n## 验证\n{body}\n")
    }

    /// Three valid activities (2 objective + 1 reflection) — clears the
    /// activity half of the audit gate.
    fn valid_activity_ops() -> serde_json::Value {
        serde_json::json!({ "operations": [
            { "op": "add_activity", "activity": { "kind": "single_choice", "prompt": "期权的本质是什么？", "options": ["权利", "义务", "债务"], "answer": "权利", "explanation": "买方持有的是权利而非义务。" } },
            { "op": "add_activity", "activity": { "kind": "true_false", "prompt": "期权卖方没有履约义务。", "answer": false, "explanation": "卖方承担履约义务。" } },
            { "op": "add_activity", "activity": { "kind": "reflection", "prompt": "结合一个场景说明权利与义务的不对称。", "answer": null, "explanation": "用身边合同或规则的例子说明：权利可以放弃，义务不履行会有后果，两者不对等。" } },
            { "op": "set_estimated_minutes", "minutes": 15 }
        ] })
    }

    /// Two-section manifest: one concept section (visual promise 表格) plus
    /// the mandatory closing practice section.
    fn section_manifest_ops() -> serde_json::Value {
        serde_json::json!({ "sections": [
            { "section_key": "s1", "kind": "concept", "title": "概念：期权的定义", "points": "权利义务不对称", "visual": "表格" },
            { "section_key": "s2", "kind": "practice", "title": "练习：期权判断", "points": "判断题自测", "visual": "无" }
        ] })
    }

    /// A concept body that clears the section gate: Markdown table delivers
    /// the declared 表格 promise, prose clears the length floor.
    fn concept_body() -> String {
        format!(
            "## 概念：期权的定义\n\n| 头寸 | 权利 |\n| --- | --- |\n| 买方 | 有 |\n\n{}\n",
            "这是一个足够长的正文段落，用于通过节级质检门的长度下限要求。".repeat(12)
        )
    }

    /// A practice body that clears the section gate (capability goal +
    /// answering guidance only, under the 250-character cap).
    fn practice_body() -> String {
        "## 练习：期权判断\n\n完成以下判断题，检验你对权利义务不对称与四类基本头寸的理解；\
         作答后阅读解析，回顾买方权利与卖方义务的关键区别。"
            .into()
    }

    fn lesson_context() -> LessonGenerationContext {
        LessonGenerationContext {
            lesson_id: "lesson-1".into(),
            course_title: "测试课程".into(),
            course_description: "零基础期权入门：从权利义务讲到期权策略".into(),
            module_title: "模块一".into(),
            module_index: 0,
            lesson_title: "课时一".into(),
            lesson_index: 0,
            total_lessons: 2,
            next_lesson_title: Some("课时二".into()),
            purpose: "理解期权的定义".into(),
            excerpt: Some(LessonExcerpt::text_only("docs/basics.md", "期权的定义……")),
            excerpts: Vec::new(),
            outline_tree: String::new(),
            adjacent_context: String::new(),
            graph: None,
            goal: None,
            forbidden_concepts: String::new(),
            usage: None,
            retry_feedback: None,
        }
    }

    /// 学习图节点的用户回合：图语义段落（目标/范围/前置路径/后续节点）取代
    /// 课程目录/模块/下一课段；无摘录时以学习图专用「无简报」句兜底。
    #[test]
    fn lesson_user_text_renders_graph_sections_instead_of_outline() {
        let mut context = lesson_context();
        context.clear_grounding();
        context.graph = Some(GraphLessonContext {
            goal: "通盘认识期权交易".into(),
            degree_statement: "聚焦场内标准期权，不含结构性产品".into(),
            prerequisite_path: "1. 什么是衍生品 — 先建立衍生品框架\n".into(),
            upcoming_nodes: "- 期权定价基础（为理解希腊字母铺垫）\n".into(),
            concept_faces: None,
            est_minutes: Some(20),
            known_names: None,
            teaches_tiers: Vec::new(),
            precedents: Vec::new(),
            intent: None,
            scope: None,
        });
        let text = lesson_user_text(&context);
        assert!(text.contains("学习图节点"));
        assert!(text.contains("学习目标：通盘认识期权交易"));
        assert!(text.contains("程度声明：聚焦场内标准期权"));
        assert!(text.contains("前置路径"));
        assert!(text.contains("1. 什么是衍生品"));
        // p02 B 双头合并：单一块头（吸收「前置契约」句），不再出现独立
        // 契约行（真机 L11-12 两头并一）。
        assert!(
            text.contains(
                "前置路径（学习者到达本节点前应已掌握，按学习顺序；概念已由其他节点教到标注档位——不要重新讲授，只在必要时按该档位复述或引用。条目下的要点是各前置实际教过的内容——不要重复讲授）："
            ),
            "{text}"
        );
        assert!(!text.contains("前置契约："), "{text}");
        assert!(text.contains("后续节点"));
        assert!(text.contains("期权定价基础"));
        assert!(text.contains("学习图节点没有课程简报"));
        // 传统段落不再出现。
        assert!(!text.contains("课程完整目录"));
        assert!(!text.contains("下一课"));
        assert!(!text.contains("课程简报为空"));
        assert!(!text.contains("课程简报（文档与活动必须忠于它）"));

        // 起点/终点节点：无前置与无后续的措辞分支。
        context.graph = Some(GraphLessonContext {
            goal: "通盘认识期权交易".into(),
            degree_statement: "聚焦场内标准期权".into(),
            prerequisite_path: String::new(),
            upcoming_nodes: String::new(),
            concept_faces: None,
            est_minutes: Some(15),
            known_names: None,
            teaches_tiers: Vec::new(),
            precedents: Vec::new(),
            intent: None,
            scope: None,
        });
        let text = lesson_user_text(&context);
        assert!(text.contains("从零讲起"));
        assert!(text.contains("整个学习图的收束"));
    }

    /// 空值行省略（p02 C，真机 L4/L33）：程度声明空值省行 + 无简报尾注
    /// 省其列项；goal/degree 全空时头部只剩「学习图节点」框架行、尾注只剩
    /// 前置/后续段落列项。
    #[test]
    fn lesson_user_text_omits_empty_degree_statement_and_tail_note_item() {
        let mut context = lesson_context();
        context.clear_grounding();
        context.graph = Some(GraphLessonContext {
            goal: "理解衍生品的基础计量工具".into(),
            degree_statement: String::new(),
            prerequisite_path: String::new(),
            upcoming_nodes: String::new(),
            concept_faces: None,
            est_minutes: Some(20),
            known_names: None,
            teaches_tiers: Vec::new(),
            precedents: Vec::new(),
            intent: None,
            scope: None,
        });
        let text = lesson_user_text(&context);
        assert!(text.contains("学习目标：理解衍生品的基础计量工具\n"), "{text}");
        assert!(!text.contains("程度声明"), "{text}");
        // 尾注：不再指向空白程度声明。
        assert!(
            text.contains(
                "\n学习图节点没有课程简报：内容忠于学习目标与前置/后续节点段落。\n"
            ),
            "{text}"
        );

        // 全空：头部只剩框架行，尾注只剩前置/后续段落列项。
        context.graph = Some(GraphLessonContext {
            goal: "  ".into(),
            degree_statement: " ".into(),
            prerequisite_path: String::new(),
            upcoming_nodes: String::new(),
            concept_faces: None,
            est_minutes: Some(20),
            known_names: None,
            teaches_tiers: Vec::new(),
            precedents: Vec::new(),
            intent: None,
            scope: None,
        });
        let text = lesson_user_text(&context);
        assert!(text.contains("学习图节点（"), "{text}");
        assert!(!text.contains("学习目标："), "{text}");
        assert!(text.contains("内容忠于前置/后续节点段落。"), "{text}");
    }

    /// 概念面投影（j07）：图节点路径的用户回合渲染概念面段（首现/非首现/
    /// 引用名/旧叫法）；传统路径（graph = None）不出现该段。
    #[test]
    fn lesson_user_text_renders_concept_faces_for_graph_nodes_only() {
        let mut context = lesson_context();
        context.clear_grounding();
        context.graph = Some(GraphLessonContext {
            goal: "g".into(),
            degree_statement: "d".into(),
            prerequisite_path: String::new(),
            upcoming_nodes: String::new(),
            concept_faces: Some(nomifun_learning::GraphConceptFaces {
                first_seen: vec!["希腊字母".into()],
                returning: vec!["衍生品".into()],
                ref_names: vec![("Delta".into(), "德尔塔".into())],
                aliases: vec![("衍生品".into(), vec!["衍生工具".into()])],
            }),
            est_minutes: Some(20),
            known_names: None,
            teaches_tiers: Vec::new(),
            precedents: Vec::new(),
            intent: None,
            scope: None,
        });
        let text = lesson_user_text(&context);
        assert!(text.contains("概念面"));
        assert!(text.contains("首现"));
        assert!(text.contains("非首现"));
        assert!(text.contains("引用名"));
        // 旧叫法行方向化（p02 D）：「本课现用名（旧称：…）」新措辞在场，
        // 「X 又叫 Y」旧方向措辞退场。
        assert!(text.contains("本课现用名"), "{text}");
        assert!(text.contains("（旧称：衍生工具）"), "{text}");
        assert!(!text.contains("又叫"), "{text}");
        assert!(text.contains("用名纪律"));

        // 传统路径：graph = None → 概念面段不出现。
        let mut plain = lesson_context();
        plain.clear_grounding();
        let text = lesson_user_text(&plain);
        assert!(!text.contains("概念面"));
    }

    /// k03：图路径注入「时长预算」行（est 值在场）；开关关闭或无 est 时
    /// 整行省略；传统路径不出现该行。
    #[test]
    fn graph_budget_line_renders_est_and_respects_the_switch() {
        let enabled = graph_budget_line(true, Some(20)).expect("on 臂注入");
        assert!(enabled.contains("时长预算：20 分钟"), "{enabled}");
        assert!(graph_budget_line(false, Some(20)).is_none(), "off 臂省略整行");
        assert!(graph_budget_line(true, None).is_none(), "无 est 省略整行");

        let mut context = lesson_context();
        context.clear_grounding();
        context.graph = Some(GraphLessonContext {
            goal: "g".into(),
            degree_statement: "d".into(),
            prerequisite_path: String::new(),
            upcoming_nodes: String::new(),
            concept_faces: None,
            est_minutes: Some(20),
            known_names: None,
            teaches_tiers: Vec::new(),
            precedents: Vec::new(),
            intent: None,
            scope: None,
        });
        let text = lesson_user_text(&context);
        assert!(text.contains("时长预算：20 分钟"), "{text}");

        let mut plain = lesson_context();
        plain.clear_grounding();
        assert!(!lesson_user_text(&plain).contains("时长预算"), "传统路径无预算行");
    }

    /// n3 输入面三行（#177 Known Names / D4 档位行 / #180 先例）：图路径
    /// 用户轮在概念面段前注入已知概念行与先例块、节点块注入档位行；空
    /// 名单/空档位/空先例各自整行（块）省略；传统路径三行全不出现。
    #[test]
    fn lesson_user_text_renders_known_names_tiers_and_precedents_for_graph_nodes() {
        let mut context = lesson_context();
        context.clear_grounding();
        context.graph = Some(GraphLessonContext {
            goal: "g".into(),
            degree_statement: "d".into(),
            prerequisite_path: String::new(),
            upcoming_nodes: String::new(),
            concept_faces: None,
            est_minutes: Some(20),
            known_names: Some(vec!["对数收益率".into(), "历史波动率".into()]),
            teaches_tiers: vec![("历史波动率".into(), "知道".into())],
            precedents: vec![nomifun_learning::LessonPrecedent {
                concept: "对数收益率".into(),
                node_name: "收益率与对数收益率".into(),
            }],
            intent: None,
            scope: None,
        });
        let text = lesson_user_text(&context);
        // Known Names 行：名单 + 边界措辞（冒烟判据①机械 grep 面）。
        assert!(text.contains("课程已知概念"), "{text}");
        assert!(text.contains("对数收益率、历史波动率"), "{text}");
        assert!(text.contains("名单外的领域名词不引入"), "{text}");
        // 档位行：深度角进节点块。
        assert!(text.contains("节点档位"), "{text}");
        assert!(text.contains("历史波动率（知道）"), "{text}");
        // 先例块：概念 ← 节点映射（冒烟判据⑤机械 grep 面）；p02 双传收敛
        // ——节清单不再重复进先例块（前置路径承载节要点）。
        assert!(text.contains("已建立先例"), "{text}");
        assert!(text.contains("概念「对数收益率」← 节点「收益率与对数收益率」"), "{text}");
        assert!(!text.contains("概念：对数收益；例题：两日收益换算"), "{text}");

        // 全空三行：名单 None、档位空、先例空 → 全部省略。
        let mut bare = lesson_context();
        bare.clear_grounding();
        bare.graph = Some(GraphLessonContext {
            goal: "g".into(),
            degree_statement: "d".into(),
            prerequisite_path: String::new(),
            upcoming_nodes: String::new(),
            concept_faces: None,
            est_minutes: Some(20),
            known_names: None,
            teaches_tiers: Vec::new(),
            precedents: Vec::new(),
            intent: None,
            scope: None,
        });
        let bare_text = lesson_user_text(&bare);
        assert!(!bare_text.contains("课程已知概念"), "{bare_text}");
        assert!(!bare_text.contains("节点档位"), "{bare_text}");
        assert!(!bare_text.contains("已建立先例"), "{bare_text}");

        // 传统路径：三行全不出现。
        let plain = lesson_user_text(&lesson_context());
        assert!(!plain.contains("课程已知概念"), "{plain}");
        assert!(!plain.contains("节点档位"), "{plain}");
        assert!(!plain.contains("已建立先例"), "{plain}");
    }

    /// 声明面骨架行（absorb-8 z03，learnhub #205 契约 v2.17）：图臂节点
    /// 定位行后插「创建意图/教学范围」两行——两字段齐两行在场；单缺席只
    /// 出在场行；全缺席（旧档）与传统路径零两行（旧行为零变化）。
    #[test]
    fn lesson_user_text_renders_declaration_lines_for_declared_graph_nodes() {
        let graph_ctx = |intent: Option<&str>, scope: Option<&str>| {
            let mut context = lesson_context();
            context.clear_grounding();
            context.graph = Some(GraphLessonContext {
                goal: "g".into(),
                degree_statement: "d".into(),
                prerequisite_path: String::new(),
                upcoming_nodes: String::new(),
                concept_faces: None,
                est_minutes: Some(20),
                known_names: None,
                teaches_tiers: Vec::new(),
                precedents: Vec::new(),
                intent: intent.map(str::to_owned),
                scope: scope.map(str::to_owned),
            });
            context
        };
        // 两字段齐：两行在场，且插在节点定位行之后（骨架行现有风格）。
        let text = lesson_user_text(&graph_ctx(
            Some("在收益率之后解释波动率，承接换算例题"),
            Some("教波动率的直观刻画；不引入 GARCH 模型"),
        ));
        assert!(text.contains("创建意图：在收益率之后解释波动率，承接换算例题\n"), "{text}");
        assert!(
            text.contains(
                "教学范围：教波动率的直观刻画；不引入 GARCH 模型（范围外内容不教，越界点名不构成引用）\n"
            ),
            "{text}"
        );
        let node_position = text.find("节点定位：").expect("节点定位行在场");
        let intent_line = text.find("创建意图：").expect("意图行在场");
        assert!(
            intent_line > node_position,
            "声明两行插在节点定位行之后：{text}"
        );
        // 单缺席：只出在场行。
        let scope_only = lesson_user_text(&graph_ctx(None, Some("教基本运算；不展开对数")));
        assert!(scope_only.contains("教学范围：教基本运算；不展开对数"), "{scope_only}");
        assert!(!scope_only.contains("创建意图"), "{scope_only}");
        // 全缺席（旧档）= 零两行；空白串同折叠。
        let legacy = lesson_user_text(&graph_ctx(None, None));
        assert!(!legacy.contains("创建意图"), "{legacy}");
        assert!(!legacy.contains("教学范围"), "{legacy}");
        let blank = lesson_user_text(&graph_ctx(Some("  "), Some("")));
        assert!(!blank.contains("创建意图"), "{blank}");
        assert!(!blank.contains("教学范围"), "{blank}");
        // 传统路径：恒零两行。
        let plain = lesson_user_text(&lesson_context());
        assert!(!plain.contains("创建意图"), "{plain}");
        assert!(!plain.contains("教学范围"), "{plain}");
    }

    /// 声明优先条款（absorb-8 z03，learnhub content-plan「声明优先于推断」
    /// + content-write「以教学范围声明为界」对偶句的 allo 合成）：系统提示
    /// 词进图臂写作纪律区——两字段在场为意图/边界第一真相；档位/深度照旧
    /// 归档位行与程度声明；点名边界不构成引用不构成引入豁免。
    #[test]
    fn lesson_agent_system_prompt_carries_the_declaration_first_clause() {
        let system = GENERATE_LESSON_AGENT_SYSTEM.as_str();
        for phrase in [
            "声明优先（创建意图/教学范围）",
            "**第一真相**",
            "节安排不越出声明的教学范围",
            "照旧管档位与深度",
            "点名边界",
            "不构成对 X 的引用，更不构成引入豁免",
            "两行缺席（旧档节点）= 无声明面",
        ] {
            assert!(system.contains(phrase), "system prompt missing「{phrase}」");
        }
    }

    /// k05：写作成色四条新纪律（R10 具象替身 / R11 受众语域 / R12 表述工具
    /// 在场 / R13 断言-例子一致）+ w1 的 R14（可算量必须实算，绑 verify_math
    /// 工具）进图管线系统提示词（默认开；off 臂由 nomifun-learning 的开关
    /// 测试锁装配函数）。R12 为 #177 收窄版条款覆盖断言：逃生门只许记号与
    /// 读法，旧「正文先行引入」全量门必须消失。
    #[test]
    fn lesson_agent_system_prompt_carries_content_discipline_v2() {
        let system = GENERATE_LESSON_AGENT_SYSTEM.as_str();
        for phrase in [
            "具象替身先（R10）",
            "受众语域（R11）",
            "表述工具在场（R12）",
            "断言-例子一致（R13）",
            "可算量必须实算（R14）",
            "先调 verify_math 实算再写入正文",
        ] {
            assert!(system.contains(phrase), "system prompt missing「{phrase}」");
        }
        // R12 收窄（#177）：只适用于记号与读法；领域概念不适用；旧全量
        // 逃生门措辞不得残留（活性超纲门，learnhub 病历同款）。
        assert!(system.contains("只适用于**记号与读法**"), "{system}");
        assert!(system.contains("领域概念不适用"), "{system}");
        assert!(!system.contains("或本节正文已先行正式引入"), "{system}");
        assert!(!system.contains("或在正文内先正式引入再用"), "{system}");
        // j07 基线条款仍在（回归护栏）。
        assert!(system.contains("先推后练"));
        assert!(system.contains("对照要真对照"));
    }

    /// R14 跨宿互钉（w1 评审修复，轻量漂移钉）：非对称孪生的两宿各自携带
    /// R14 标记——EN 块（nomifun-learning `CONTENT_DISCIPLINE_V2_BLOCK`，传统
    /// 线纪律版）与中文块（本图线 `CONTENT_DISCIPLINE_V2_CN`，绑 verify_math
    /// 工具版）。本测试是唯一能同时看到两宿的位置（依赖方向 ai-agent →
    /// learning）：任一侧删条目/换编号/漏改时这里即红，防孪生漂移成单宿
    /// 条款。
    #[test]
    fn r14_marker_is_pinned_on_both_host_blocks() {
        assert!(
            CONTENT_DISCIPLINE_V2_CN.contains("R14"),
            "中文块（图线宿主）缺 R14 标记——单侧漏改"
        );
        assert!(
            nomifun_learning::CONTENT_DISCIPLINE_V2_BLOCK.contains("R14"),
            "EN 块（传统线宿主）缺 R14 标记——单侧漏改"
        );
        // y06（#201/#202/#203）：R15–R20 在两侧纪律块同批落位，R21 在两侧
        // 题库面菜单同批落位——任一宿删条目/换编号即红。
        for marker in ["R15", "R16", "R17", "R18", "R19", "R20"] {
            assert!(
                CONTENT_DISCIPLINE_V2_CN.contains(marker),
                "中文块（图线宿主）缺 {marker} 标记——单侧漏改"
            );
            assert!(
                nomifun_learning::CONTENT_DISCIPLINE_V2_BLOCK.contains(marker),
                "EN 块（传统线宿主）缺 {marker} 标记——单侧漏改"
            );
        }
        assert!(
            DISTRACTOR_MENU_CN.contains("R21"),
            "中文五型菜单缺 R21 标记"
        );
        assert!(
            nomifun_learning::DISTRACTOR_MENU_BLOCK.contains("R21"),
            "EN 五型菜单缺 R21 标记"
        );
    }

    /// y06（learnhub #201/#202/#203）：条款批的图线孪生——材料学五条 +
    /// 完备性写作面进 CN 纪律块；规划三面 + 分段例组进规划段；选择题面五型
    /// + 题库两考向进活动契约。两宿形状互钉由上面的标记测试承担。
    #[test]
    fn lesson_agent_system_prompt_carries_the_y06_clauses() {
        let system = GENERATE_LESSON_AGENT_SYSTEM.as_str();
        for phrase in [
            "例子四件齐（R15",
            "符号—对象—条件落点（R16",
            "类比带边界（R17",
            "分段例组（R18",
            "关系句可还原（R19",
            "完备面不缩水（R20，写作面）",
            "完备性三面（R20，规划面）",
            "分段例组（R18，规划面）",
            "干扰项五型（R21",
            "两个考向都要有份量（R20，题库面）",
        ] {
            assert!(system.contains(phrase), "system prompt missing「{phrase}」");
        }
        // 并置条款：不新增术语 / 无自然负例不硬造 / 只在已教边界内补缺口。
        assert!(system.contains("不新增术语"), "{system}");
        assert!(system.contains("无自然负例不得硬造"), "{system}");
        assert!(system.contains("只教本节任务点"), "{system}");
        assert!(system.contains("不是超纲许可"), "{system}");
    }

    /// y06 off 臂形状：新条款由 V2 开关整块摘除；`ls_patch_activities`
    /// 描述只在 on 臂追加总闸句（off 臂 = 原描述逐字，不长菜单塞工具面）。
    #[test]
    fn y06_clause_arms_swap_wholesale() {
        for marker in ["R15", "R16", "R17", "R18", "R19", "R20"] {
            assert!(
                CONTENT_DISCIPLINE_V2_CN.contains(marker),
                "中文纪律块缺 {marker}"
            );
        }
        assert!(COMPLETENESS_PLANNING_CN.contains("完备性三面（R20，规划面）"));
        assert!(SEGMENTED_EXAMPLES_PLANNING_CN.contains("分段例组（R18，规划面）"));
        assert!(DISTRACTOR_MENU_CN.contains("干扰项五型（R21"));
        assert!(QUESTION_COVERAGE_CN.contains("两个考向都要有份量（R20，题库面）"));
        assert!(
            PATCH_ACTIVITIES_DISTRACTOR_NOTE_CN.contains("负例优先自然，无自然负例不硬造")
        );
        assert!(!PATCH_ACTIVITIES_DESCRIPTION.contains("无自然负例不得硬造"));
    }

    /// n3（#177/#178 条款移植）：规划面四条款（构件词边界 / 练习形态按
    /// 档位 / 取材池 / 意图画像与容量预算）+ 写作面「规划即雄心」进图管线
    /// 系统提示词——条款覆盖短语断言（三闸之短语闸；金样闸不适用：图线
    /// 系统提示词无金样资产，围栏面由条款短语在场性承担）。
    #[test]
    fn lesson_agent_system_prompt_carries_the_boundary_and_planning_clauses() {
        let system = GENERATE_LESSON_AGENT_SYSTEM.as_str();
        for phrase in [
            "构件词边界",
            "练习形态按档位",
            "取材池",
            "多样轮换",
            "意图画像与容量预算",
            "先砍雄心再砍内容",
            "节点意图超载",
            "规划即雄心",
            "课程已知概念",
        ] {
            assert!(system.contains(phrase), "system prompt missing「{phrase}」");
        }
    }

    /// k03/#178：时长预算条款语义换血——on 臂（默认）= 意图画像 + 超载必写
    /// + teaches 覆盖不可砍；旧「超容才写、只留痕不改安排」措辞不得残留。
    #[test]
    fn lesson_agent_system_prompt_carries_the_capacity_note_clause() {
        let system = GENERATE_LESSON_AGENT_SYSTEM.as_str();
        assert!(system.contains("时长预算"), "{system}");
        assert!(system.contains("capacity_note"), "{system}");
        assert!(system.contains("先砍雄心再砍内容"), "{system}");
        assert!(system.contains("teaches 的覆盖不可砍"), "{system}");
        assert!(system.contains("必写、指名超载原因"), "{system}");
        assert!(system.contains("预算内收敛 = 整键省略"), "{system}");
        // 旧 #154 语义（活性自相矛盾源）不得残留。
        assert!(!system.contains("明显超容才"), "{system}");
        assert!(!system.contains("不改安排、不压缩讲解深度"), "{system}");
    }

    /// k03/#178 off 臂：NOMI_LEARNING_EST_BUDGET=off 时时长预算条款整段
    /// 回退旧语义（同二进制双臂的 off 臂逐字复现旧世界）。env 是进程全局
    /// 态：LazyLock 的 system prompt 已在上一个测试固化，这里用枚举臂的
    /// 常量对照锁「off 臂 = 旧文本、on 臂 = 新文本」的形状。
    #[test]
    fn est_budget_clause_arms_swap_wholesale() {
        assert!(EST_INTENT_CLAUSE_CN.contains("意图画像与容量预算"));
        assert!(EST_INTENT_CLAUSE_CN.contains("先砍雄心再砍内容"));
        assert!(EST_LEGACY_CLAUSE_CN.contains("明显超容才"));
        assert!(EST_LEGACY_CLAUSE_CN.contains("不改安排、不压缩讲解深度"));
        assert!(!EST_LEGACY_CLAUSE_CN.contains("意图画像"));
    }

    /// The user turn embeds the outline tree and the adjacent-lesson
    /// reference when present, and stays clean when both are empty.
    #[test]
    fn lesson_user_text_embeds_outline_and_adjacent_sections() {
        let mut context = lesson_context();
        context.outline_tree = "模块 1/1：模块一\n  1. 课时〇 — 铺垫\n  2. 课时一 — 理解期权的定义（本课时）".into();
        context.adjacent_context = "相邻课时参考：\n- 上一课时「课时〇」— 铺垫".into();
        let text = lesson_user_text(&context);
        assert!(text.contains("课程完整目录"));
        assert!(text.contains("（本课时）"));
        assert!(text.contains("相邻课时参考"));
        assert!(text.contains("课时〇"));

        let plain = lesson_user_text(&lesson_context());
        assert!(!plain.contains("课程完整目录"));
        assert!(!plain.contains("相邻课时参考"));
    }

    /// 防超纲黑名单注入：`forbidden_concepts` 非空时渲染为独立段（传统课
    /// 时与学习图节点共用该注入点）；空串不产生任何文本。
    #[test]
    fn lesson_user_text_embeds_the_forbidden_blacklist_when_present() {
        let mut context = lesson_context();
        context.forbidden_concepts =
            "禁止使用的概念（尚未讲授——正文不得出现这些名称，也不得引用其结论）：\n- 希腊字母（期权 sensitivities）".into();
        let text = lesson_user_text(&context);
        assert!(text.contains("禁止使用的概念"), "{text}");
        assert!(text.contains("希腊字母"), "{text}");

        let plain = lesson_user_text(&lesson_context());
        assert!(!plain.contains("禁止使用的概念"), "{plain}");
    }

    /// 目标注入（t1）：传统课时带目标时，用户轮先立"课程目标"锚点；旧课程
    /// （无目标）不出现该段——与旧路径逐字一致。
    #[test]
    fn lesson_user_text_embeds_the_course_goal_when_present() {
        let mut context = lesson_context();
        assert!(!lesson_user_text(&context).contains("课程目标"));
        context.goal = Some("零基础学 Python 数据分析".into());
        let text = lesson_user_text(&context);
        assert!(text.contains("课程目标（学习者要达成的整体目标）：零基础学 Python 数据分析"), "{text}");
    }

    fn engine(service: Arc<LearningService>) -> LiveLessonContentAgentEngine {
        LiveLessonContentAgentEngine {
            service,
            deps: test_deps(),
            round_logs: Arc::new(Mutex::new(HashMap::new())),
            prompt_observer: None,
            provider_decorator: None,
        }
    }

    fn context(
        service: Arc<LearningService>,
    ) -> (
        Arc<LoopContext>,
        Arc<Mutex<Option<String>>>,
        Arc<Mutex<Option<LessonOutput>>>,
    ) {
        context_with(service, lesson_context())
    }

    fn context_with(
        service: Arc<LearningService>,
        context: LessonGenerationContext,
    ) -> (
        Arc<LoopContext>,
        Arc<Mutex<Option<String>>>,
        Arc<Mutex<Option<LessonOutput>>>,
    ) {
        let draft_slot = Arc::new(Mutex::new(None));
        let published_slot = Arc::new(Mutex::new(None));
        let ctx = Arc::new(LoopContext {
            service,
            context,
            draft_slot: Arc::clone(&draft_slot),
            published_slot: Arc::clone(&published_slot),
            channel: LoopChannel::new(WIRE, BUDGETS.generate_max_rounds),
            stored_writes: Arc::new(Mutex::new(StoredWrites::default())),
            math_verify: Arc::new(MathVerifyCounters::default()),
        });
        (ctx, draft_slot, published_slot)
    }

    /// 学习图节点的生成上下文（teaches 档位可配）；传统线 = `lesson_context`。
    fn graph_context(teaches_tiers: Vec<(String, String)>) -> LessonGenerationContext {
        let mut context = lesson_context();
        context.clear_grounding();
        context.graph = Some(GraphLessonContext {
            goal: "通盘认识极限".into(),
            degree_statement: "能独立判断极限是否存在".into(),
            prerequisite_path: String::new(),
            upcoming_nodes: String::new(),
            concept_faces: None,
            est_minutes: Some(20),
            known_names: None,
            teaches_tiers,
            precedents: Vec::new(),
            intent: None,
            scope: None,
        });
        context
    }

    fn graph_teaches_context() -> LessonGenerationContext {
        graph_context(vec![("极限".to_owned(), "会用".to_owned())])
    }

    const REGISTRY_CONCEPT: &str = "0190f5fe-7c00-7a00-8abc-111111111111";

    /// 带登记表种子的服务（六条活跃别名：detail 全别名与 top-5 无关的
    /// 证据面）。返回 pool 句柄供只读自证快照。
    async fn registry_service() -> (Arc<LearningService>, sqlx::SqlitePool) {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let pool = database.pool().clone();
        sqlx::query(
            "INSERT INTO learning_concept_registry \
             (concept_id, canonical, aliases_json, definition, deprecated, doc_version, created_at, updated_at) \
             VALUES (?, '极限', ?, '无限逼近的值：函数值趋近于某个定值', 0, 1, 1, 1)",
        )
        .bind(REGISTRY_CONCEPT)
        .bind(
            r#"[{"name":"Limits","status":"active"},{"name":"别名2"},{"name":"别名3"},{"name":"别名4"},{"name":"别名5"},{"name":"别名6"}]"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        (Arc::new(LearningService::new(pool.clone())), pool)
    }

    async fn registry_snapshot(
        pool: &sqlx::SqlitePool,
    ) -> Vec<(String, String, String, String, i64, i64)> {
        sqlx::query_as(
            "SELECT concept_id, canonical, aliases_json, definition, deprecated, updated_at \
             FROM learning_concept_registry ORDER BY concept_id",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    fn mounted_search_tool(ctx: &Arc<LoopContext>, repair_face: bool) -> OneShotTool {
        lesson_content_tools(Arc::clone(ctx), repair_face)
            .into_iter()
            .find(|tool| tool.name == "search_concepts")
            .expect("search_concepts must be mounted for a teaches-bearing graph node")
    }

    /// 安全不变量：发给模型的工具注册面恰等于构造的工具集——每一轮都如此；
    /// 生成 loop 携带 8 个工具（含 ls_start 与 verify_math，不含
    /// ls_set_document——单篇契约无可视化承诺，不给新草稿绕过分节的路径）；
    /// 修复 loop 反过来：省略 ls_start，保留 ls_set_document 以修复旧单篇
    /// 草稿。verify_math 是两面同备的无草稿只读 op（w1）。
    #[tokio::test]
    async fn lesson_loop_exposes_exactly_the_whitelist() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(service.clone());
        let generation: Vec<String> = lesson_content_tools(Arc::clone(&ctx), false)
            .iter()
            .map(|tool| tool.name.clone())
            .collect();
        assert_eq!(
            generation,
            vec![
                "ls_start", "ls_inspect", "ls_set_section_manifest", "ls_set_section_body",
                "ls_patch_activities", "ls_audit", "ls_finish", "verify_math"
            ]
        );
        // Repair face: no ls_start, but ls_set_document for legacy drafts.
        let repair_names: Vec<String> = lesson_content_tools(Arc::clone(&ctx), true)
            .iter()
            .map(|tool| tool.name.clone())
            .collect();
        assert_eq!(
            repair_names,
            vec![
                "ls_set_document", "ls_inspect", "ls_set_section_manifest", "ls_set_section_body",
                "ls_patch_activities", "ls_audit", "ls_finish", "verify_math"
            ]
        );

        // Round 1: the model asks ls_inspect before any draft exists — the
        // handler must answer with a guidance error, never panic.
        let provider = ScriptedProvider::new(vec![
            vec![
                tool_use("ls_inspect", serde_json::json!({})),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("understood".into()), done(StopReason::EndTurn)],
        ]);
        run_agent_loop(
            provider.clone(),
            "test-model",
            GENERATE_LESSON_AGENT_SYSTEM.as_str(),
            &lesson_user_text(&ctx.context),
            &lesson_content_tools(Arc::clone(&ctx), false),
            BUDGETS.generate_max_rounds,
            BUDGETS.round_tokens,
            ThinkingConfig::Disabled,
            GENERATE_REASONING_EFFORT,
            nomifun_learning::phase::GENERATE,
            Some(ctx.as_ref()),
            None,
        )
        .await
        .unwrap();

        let seen = provider.seen_tool_names.lock().unwrap();
        assert_eq!(seen.len(), 2, "two model rounds");
        for round in seen.iter() {
            assert_eq!(round, &generation, "tool registry must be exactly the whitelist on every round");
        }
        let thinking = provider.seen_thinking.lock().unwrap();
        assert!(
            thinking.iter().all(|t| matches!(t, Some(ThinkingConfig::Disabled))),
            "every round must explicitly disable thinking: {thinking:?}"
        );
        let rounds = provider.seen_messages.lock().unwrap();
        let followup = &rounds[1];
        let last = followup.last().unwrap();
        assert!(matches!(
            &last.content[0],
            ContentBlock::ToolResult { is_error: true, content, .. }
                if content.contains("ls_start")
        ));
    }

    /// e1 D1 段级幂等跳过（红绿样）：生成面对已落库正文的节再调用
    /// ls_set_section_body = 短路返回「已在库」，正文原样保留（revision 不
    /// 动）——整课重试/续跑时已成功落库的节不重写；修复面同款调用照旧整
    /// 节重写（审计打回的正当改稿通道不受跳过约束）。
    #[tokio::test]
    async fn generation_face_skips_written_sections_while_repair_face_rewrites() {
        let (service, _dir) = test_service().await;
        let (ctx, draft_slot, _published) = context(Arc::clone(&service));
        // 预置草稿 = 「上次会话」形态：清单 + s1 正文已落库。
        let view = service.create_lesson_draft(ctx.context.clone()).unwrap();
        *draft_slot.lock().unwrap() = Some(view.draft_id.clone());
        service
            .patch_lesson_draft(
                &view.draft_id,
                vec![
                    nomifun_learning::LessonOp::SetSectionManifest {
                        sections: serde_json::from_value(section_manifest_ops()["sections"].clone())
                            .unwrap(),
                        capacity_note: None,
                    },
                    nomifun_learning::LessonOp::SetSectionBody {
                        section_key: "s1".into(),
                        body: concept_body(),
                    },
                ],
            )
            .unwrap();
        let before = service.inspect_lesson_draft(&view.draft_id).unwrap();
        assert_eq!(before.section_bodies.len(), 1, "s1 已落库");
        let original_chars = before.section_bodies[0].chars;

        // 生成面：同 key 交一份**不同**正文 → 幂等跳过，原稿保留。
        let generation_face = lesson_content_tools(Arc::clone(&ctx), false)
            .into_iter()
            .find(|tool| tool.name == "ls_set_section_body")
            .unwrap();
        let reply = (generation_face.handler)(serde_json::json!({
            "section_key": "s1",
            "body": format!("## 概念：期权的定义\n\n被续跑重写尝试污染的正文。{}", "凑长度。".repeat(40)),
        }))
        .await
        .unwrap();
        assert!(reply.contains("幂等跳过"), "{reply}");
        assert!(reply.contains("已在库"), "{reply}");
        let after_skip = service.inspect_lesson_draft(&view.draft_id).unwrap();
        assert_eq!(
            after_skip.revision, before.revision,
            "skipped op must not bump the revision"
        );
        assert_eq!(
            after_skip.section_bodies[0].chars, original_chars,
            "the original body must survive the rewrite attempt verbatim"
        );

        // 修复面：同 key 同正文 → 照旧整节重写（replaced 报告 + 正文更新）。
        // 正文带表格 = 过概念节的 visual 承诺门（改稿必须能落库）。
        let repair_face = lesson_content_tools(Arc::clone(&ctx), true)
            .into_iter()
            .find(|tool| tool.name == "ls_set_section_body")
            .unwrap();
        let rewritten = format!(
            "## 概念：期权的定义\n\n| 头寸 | 义务 |\n| --- | --- |\n| 卖方 | 有 |\n\n{}\n",
            "修复轮按审计报告重写的正文段落。".repeat(20)
        );
        let reply = (repair_face.handler)(serde_json::json!({
            "section_key": "s1",
            "body": rewritten,
        }))
        .await
        .unwrap();
        assert!(reply.contains("replaced"), "{reply}");
        let after_repair = service.inspect_lesson_draft(&view.draft_id).unwrap();
        assert!(after_repair.revision > after_skip.revision, "repair op bumps the revision");
        assert_ne!(
            after_repair.section_bodies[0].chars, original_chars,
            "the repair face must be able to rewrite a flagged section"
        );
    }

    /// verify_math 工具 op（w1/蓝图 4.2）handler 直调样：解析边界是 Err
    /// （claim JSON 损坏，带形状指引前缀）、核验 Fail 是 Ok（verdict JSON
    /// 内含 reason/computed/counterexample——核验失败是正常结论不是工具
    /// 故障，A3）、计数槽随每次调用如实累计（A5 观测面的数据源）。
    #[tokio::test]
    async fn verify_math_handler_direct_call_semantics() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(service);
        let tool = lesson_content_tools(Arc::clone(&ctx), false)
            .into_iter()
            .find(|tool| tool.name == "verify_math")
            .expect("verify_math on the generation face");

        // 解析边界：kind 缺失 / 未支持 kind / 字段类型错 → Err（形状指引前缀）。
        for bad in [
            serde_json::json!({}),
            serde_json::json!({ "kind": "equation", "expr": "1" }),
            serde_json::json!({ "kind": "eval", "expr": 12 }),
        ] {
            let error = (tool.handler)(bad).await.expect_err("corrupt claim is Err");
            assert!(
                error.starts_with("verify_math: claim 必须是"),
                "pinned prefix: {error}"
            );
            assert!(error.contains("{kind:eval|identity,...}"), "{error}");
        }

        // 多余字段宽容（w1 评审修复：deny_unknown_fields 移除，全仓工具 op
        // 参数均容多余字段——模型多带一字段不吃 Err 多耗一轮自纠）。
        let reply = (tool.handler)(serde_json::json!({
            "kind": "eval", "expr": "1+1", "expected": 2.0, "wat": 1
        }))
        .await
        .unwrap();
        assert!(reply.contains("\"kind\":\"pass\""), "{reply}");

        // 核验结论一律 Ok：pass / fail（fail 带 reason 与 computed）/ 解析错
        // verdict（定位死因）/ identity 反例。
        let reply = (tool.handler)(serde_json::json!({
            "kind": "eval", "expr": "1+1", "expected": 2.0
        }))
        .await
        .unwrap();
        assert!(reply.contains("\"kind\":\"pass\""), "{reply}");
        assert!(reply.contains("\"value\":2.0"), "{reply}");

        let reply = (tool.handler)(serde_json::json!({
            "kind": "eval", "expr": "1+1", "expected": 2.2
        }))
        .await
        .unwrap();
        assert!(reply.contains("\"kind\":\"fail\""), "{reply}");
        assert!(reply.contains("\"computed\":2.0"), "{reply}");
        assert!(reply.contains("超出容差"), "{reply}");

        let reply = (tool.handler)(serde_json::json!({
            "kind": "eval", "expr": "2x"
        }))
        .await
        .unwrap();
        assert!(reply.contains("\"kind\":\"fail\""), "{reply}");
        assert!(reply.contains("不支持隐式乘法"), "{reply}");

        let reply = (tool.handler)(serde_json::json!({
            "kind": "identity", "lhs": "x^2", "rhs": "(x+1)^2", "variables": ["x"]
        }))
        .await
        .unwrap();
        assert!(reply.contains("\"kind\":\"fail\""), "{reply}");
        assert!(reply.contains("counterexample"), "{reply}");

        let reply = (tool.handler)(serde_json::json!({
            "kind": "identity", "lhs": "x^2", "rhs": "x*x", "variables": ["x"]
        }))
        .await
        .unwrap();
        assert!(reply.contains("\"kind\":\"pass\""), "{reply}");

        // 计数槽：只计产出 verdict 的调用（Err 的损坏 claim 在解析层即拒，
        // 不产结论；多余字段样是合法调用照计——calls=6，pass=3 / fail=3，
        // 且每个 Fail 后都有复核 Pass → 未复核修正数清零）。
        let counters = ctx.math_verify.snapshot_json();
        assert_eq!(counters["calls"], 6, "{counters}");
        assert_eq!(counters["passed"], 3, "{counters}");
        assert_eq!(counters["failed"], 3, "{counters}");
        assert_eq!(counters["unresolved"], 0, "{counters}");

        // 修复面同备 verify_math，且两面共享同一计数槽（同一次会话累计）。
        let repair_tool = lesson_content_tools(Arc::clone(&ctx), true)
            .into_iter()
            .find(|tool| tool.name == "verify_math")
            .expect("verify_math on the repair face");
        (repair_tool.handler)(serde_json::json!({
            "kind": "eval", "expr": "3.0^2+1.0^2+2.0^2+4.0^2", "expected": 30.0
        }))
        .await
        .unwrap();
        let counters = ctx.math_verify.snapshot_json();
        assert_eq!(counters["calls"], 7, "{counters}");
        assert_eq!(counters["passed"], 4, "{counters}");
    }

    /// 未复核修正数语义（w1 评审修复）：Fail 置增、任一 Pass 清零——自修正
    /// 会话（核验失败→打回→复核过）的 finish findings 零
    /// `math_verify_failed` warning（Q1 口径：自修正不是失败）；累计 failed
    /// 不清（观测读数面保留全量）。
    #[test]
    fn math_verify_unresolved_resets_on_recheck_pass() {
        let counters = MathVerifyCounters::default();
        let fail = ClaimVerdict::Fail {
            reason: "boom".into(),
            computed: None,
            counterexample: None,
        };
        counters.record(&fail);
        counters.record(&fail);
        let snapshot = counters.snapshot_json();
        assert_eq!(snapshot["failed"], 2, "{snapshot}");
        assert_eq!(snapshot["unresolved"], 2, "{snapshot}");
        assert_eq!(counters.unresolved_u32(), 2);

        // 模型按 verdict 改正并重新提交核验 → Pass 清零未修正数。
        counters.record(&ClaimVerdict::Pass { value: Some(2.0) });
        let snapshot = counters.snapshot_json();
        assert_eq!(snapshot["failed"], 2, "累计 failed 保留（读数面）: {snapshot}");
        assert_eq!(snapshot["unresolved"], 0, "{snapshot}");
        assert_eq!(counters.unresolved_u32(), 0, "finish findings 零留痕");
    }

    // ── u05：verify_math 批量核验（NOMI_LEARNING_VERIFY_BATCH） ──────────

    /// NOMI_LEARNING_* 开关的进程 env 窗口（串行写 env——非串行 save/restore
    /// 会泄漏进并行测试，上一波 z02 竞态教训）：单源在
    /// `nomifun_learning::switch_env_guard`（四份脚手架收拢），本面便捷形。
    fn learning_switch_env_guard(env: &'static str, value: &str) -> impl Drop {
        nomifun_learning::switch_env_guard(env, Some(value))
    }

    /// u05 off 臂守卫（`learning_switch_env_guard` 的 verify_batch 便捷形）。
    fn verify_batch_env_guard(value: &'static str) -> impl Drop {
        learning_switch_env_guard(nomifun_learning::VERIFY_BATCH_SWITCH_ENV, value)
    }

    /// u06 off/on 臂守卫（`learning_switch_env_guard` 的历史压缩便捷形）。
    fn lesson_compact_env_guard(value: &'static str) -> impl Drop {
        learning_switch_env_guard(nomifun_learning::LESSON_HISTORY_COMPACT_SWITCH_ENV, value)
    }

    /// u05 off 臂逐字节回归（正优化验收协议②）：`NOMI_LEARNING_VERIFY_BATCH`
    /// = off 时——工具描述、input_schema 与基线逐字一致（测试内逐字副本为
    /// 独立事实源，基线文案被改动会先在这里红）；claims 数组入参走基线单
    /// claim 解析路径（同一个 Err 前缀），批量读数恒 0。
    #[tokio::test]
    #[serial_test::serial]
    async fn verify_math_off_arm_contract_is_byte_identical_baseline() {
        let _env = verify_batch_env_guard("off");
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(service);
        let tool = lesson_content_tools(Arc::clone(&ctx), false)
            .into_iter()
            .find(|tool| tool.name == "verify_math")
            .expect("verify_math on the generation face");

        // 工具描述逐字 = 基线（独立副本，改动基线文案会在这里红）。
        assert_eq!(tool.description, "数学断言核验器（确定性实算，可复现）：把正文里的数学翻译成 claim 提交核验。两类 claim——eval = 求值表达式并与 expected 比对（expected 缺席 = 只回读实算值）；identity = 等式两边在变量域采样数值比对（缺省 7 点，相对容差 1e-9）。返回 verdict JSON：pass / fail（fail 带 reason、computed、counterexample——据此修正正文数字，再提交复核）。凡可算量（和、均值、平方和、单位换算、开方）写入正文前先在这里实算。表达式规则：显式 *（不支持隐式乘法与 %）、+ - * / ^ 括号、sqrt/sin/cos/tan/ln/log/exp/abs、pi/e。");
        // schema 逐字 = 基线：无 claims 键，required 恰为 ["kind"]。
        assert_eq!(
            tool.input_schema,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["eval", "identity"], "description": "claim 类型" },
                    "expr": { "type": "string", "description": "eval：待求值表达式" },
                    "expected": { "type": "number", "description": "eval：期望值（缺席 = 只回读实算值）" },
                    "lhs": { "type": "string", "description": "identity：等式左边" },
                    "rhs": { "type": "string", "description": "identity：等式右边" },
                    "variables": { "type": "array", "items": { "type": "string" }, "description": "identity：采样变量名清单" },
                    "tolerance": { "type": "number", "description": "相对容差（缺省 1e-9，绝对地板 1e-12）" },
                    "samples": { "type": "integer", "description": "identity：采样点数（缺省 7）" }
                },
                "required": ["kind"]
            }),
            "off 臂 schema 逐字 = 基线"
        );

        // claims 数组入参 → 基线单 claim 解析路径（同一个 Err 前缀——基线
        // 对该形状本就报 claim 形状错，off 臂如实复现）。
        let error = (tool.handler)(serde_json::json!({
            "claims": [
                { "kind": "eval", "expr": "1+1", "expected": 2.0 },
                { "kind": "eval", "expr": "2+2", "expected": 4.0 }
            ]
        }))
        .await
        .expect_err("off 臂 claims 形状按基线解析失败");
        assert!(
            error.starts_with("verify_math: claim 必须是"),
            "基线同款前缀: {error}"
        );
        // 单 claim 形状照旧可用（向后兼容本来就是基线行为）。
        let reply = (tool.handler)(serde_json::json!({
            "kind": "eval", "expr": "1+1", "expected": 2.0
        }))
        .await
        .unwrap();
        assert!(reply.contains("\"kind\":\"pass\""), "{reply}");
        let counters = ctx.math_verify.snapshot_json();
        assert_eq!(counters["calls"], 1, "{counters}");
        assert_eq!(counters["batches"], 0, "off 臂零批量: {counters}");
    }

    /// u05 on 臂批量语义：claims 数组逐条核验、返回逐条 verdict 列表（沿用
    /// 单条文案）；计数按 **claim** 口径不变（calls=条数），另加 batches 读数
    /// （批量调用次数）；单 claim 形状向后兼容（返回单 verdict、不记批量）；
    /// 空数组与超上限（>12）给可读 Err。
    #[tokio::test]
    #[serial_test::serial]
    async fn verify_math_batch_handler_semantics() {
        let _env = verify_batch_env_guard("on");
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(service);
        let tool = lesson_content_tools(Arc::clone(&ctx), false)
            .into_iter()
            .find(|tool| tool.name == "verify_math")
            .expect("verify_math on the generation face");

        // 工具面 on 臂：描述带批量句、schema 带 claims 属性（maxItems 12）。
        assert!(
            tool.description.contains("一次性提交核验"),
            "描述带批量指引: {}",
            tool.description
        );
        assert!(
            tool.input_schema["properties"]["claims"]["maxItems"]
                == serde_json::json!(VERIFY_MATH_BATCH_CAP),
            "schema 带 claims 上限: {}",
            tool.input_schema
        );

        // 批量核验：pass/fail 混批逐条返回，顺序与入参一致。
        let reply = (tool.handler)(serde_json::json!({
            "claims": [
                { "kind": "eval", "expr": "1+1", "expected": 2.0 },
                { "kind": "eval", "expr": "1+1", "expected": 2.2 },
                { "kind": "identity", "lhs": "x^2", "rhs": "x*x", "variables": ["x"] }
            ]
        }))
        .await
        .expect("批量核验是 Ok（Fail 是正常结论不是工具故障）");
        let verdicts: Vec<serde_json::Value> = serde_json::from_str(&reply).expect("verdict 数组");
        assert_eq!(verdicts.len(), 3, "逐条列表: {reply}");
        assert_eq!(verdicts[0]["kind"], "pass");
        assert_eq!(verdicts[1]["kind"], "fail", "fail 条带实算值与修正指引: {verdicts:?}");
        assert!(verdicts[1]["computed"] == serde_json::json!(2.0), "{verdicts:?}");
        assert_eq!(verdicts[2]["kind"], "pass");

        // 读数：按 claim 计数口径不变（calls=3），批量调用另记（batches=1）。
        let counters = ctx.math_verify.snapshot_json();
        assert_eq!(counters["calls"], 3, "{counters}");
        assert_eq!(counters["passed"], 2, "{counters}");
        assert_eq!(counters["failed"], 1, "{counters}");
        assert_eq!(counters["batches"], 1, "{counters}");

        // 向后兼容：单 claim 形状照旧返回单 verdict 对象（不是数组），不计批量。
        let reply = (tool.handler)(serde_json::json!({
            "kind": "eval", "expr": "2+2", "expected": 4.0
        }))
        .await
        .unwrap();
        let verdict: serde_json::Value = serde_json::from_str(&reply).expect("单 verdict 对象");
        assert_eq!(verdict["kind"], "pass", "{reply}");
        let counters = ctx.math_verify.snapshot_json();
        assert_eq!(counters["calls"], 4, "calls 按 claim 计: {counters}");
        assert_eq!(counters["batches"], 1, "单 claim 不记批量: {counters}");

        // 空数组：可读 Err（不产结论、不记批量）。
        let error = (tool.handler)(serde_json::json!({ "claims": [] }))
            .await
            .expect_err("空数组退稿");
        assert!(error.contains("claims"), "{error}");
        // 超上限（13 > 12）：可读 Err 点名上限。
        let mut many = Vec::new();
        for index in 0..13 {
            many.push(serde_json::json!({ "kind": "eval", "expr": format!("{index}+0") }));
        }
        let error = (tool.handler)(serde_json::json!({ "claims": many }))
            .await
            .expect_err("超上限退稿");
        assert!(error.contains("12"), "点名上限: {error}");
        let counters = ctx.math_verify.snapshot_json();
        assert_eq!(counters["calls"], 4, "退稿不产结论: {counters}");
        assert_eq!(counters["batches"], 1, "退稿不记批量: {counters}");
    }

    /// u05 写作成色块双臂：批量句只进 R14 条款尾部（on 臂），off 臂整块
    /// 逐字等于基线常量；其余条款两臂逐字一致（纯函数面——env 不进单测）。
    #[test]
    fn discipline_v2_block_batch_sentence_only_on_batch_arm() {
        let off = discipline_v2_block(false);
        assert_eq!(
            off, CONTENT_DISCIPLINE_V2_CN,
            "off 臂 = 基线块逐字（结构保证：直接回基线常量）"
        );
        assert!(!off.contains("claims"), "基线块不提批量: {off}");

        let on = discipline_v2_block(true);
        // 其余条款逐字一致：把 R14 行剥掉后两臂全等。
        let strip_r14 = |block: &str| {
            block
                .lines()
                .filter(|line| !line.starts_with("- 可算量必须实算（R14）"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(strip_r14(&on), strip_r14(&off), "仅 R14 行变化");
        // R14 行 = 基线行 + 批量句（行尾追加，句中不带换行）。
        let on_r14 = on
            .lines()
            .find(|line| line.starts_with("- 可算量必须实算（R14）"))
            .expect("R14 行在场");
        let off_r14 = off
            .lines()
            .find(|line| line.starts_with("- 可算量必须实算（R14）"))
            .expect("R14 行在场");
        assert!(
            on_r14.strip_prefix(off_r14)
                .is_some_and(|rest| rest.contains("claims 数组")),
            "批量句追加在 R14 行尾: {on_r14}"
        );
        assert_eq!(on.lines().count(), off.lines().count(), "不增行");
    }

    /// u05 读数面：batches 计数进快照（calls.jsonl 的 math_verify 读数源），
    /// 批量调用记 1、单 claim 不记。
    #[test]
    fn math_verify_batches_counter_shapes_snapshot() {
        let counters = MathVerifyCounters::default();
        assert_eq!(counters.snapshot_json()["batches"], 0);
        counters.record_batch();
        counters.record(&ClaimVerdict::Pass { value: Some(2.0) });
        counters.record_batch();
        counters.record(&ClaimVerdict::Fail {
            reason: "boom".into(),
            computed: None,
            counterexample: None,
        });
        let snapshot = counters.snapshot_json();
        assert_eq!(snapshot["batches"], 2, "{snapshot}");
        assert_eq!(snapshot["calls"], 2, "calls 仍按 claim 计: {snapshot}");
    }

    // ── u06：历史压缩实验（NOMI_LEARNING_LESSON_HISTORY_COMPACT，缺省 off）──

    /// 压缩实验的标准脚本：开局 → 清单 → 写 s1 → 写 s2 → 活动 → finish →
    /// 收尾文本。两节正文先后落库——压缩钩子「最近一次写入保留原文」语义
    /// 的最小可辨形态（s1 被压、s2 在场）。
    fn two_section_bodies_script() -> Vec<Vec<LlmEvent>> {
        vec![
            vec![tool_use("ls_start", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                tool_use("ls_set_section_manifest", section_manifest_ops()),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use(
                    "ls_set_section_body",
                    serde_json::json!({ "section_key": "s1", "body": concept_body() }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use(
                    "ls_set_section_body",
                    serde_json::json!({ "section_key": "s2", "body": practice_body() }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![tool_use("ls_patch_activities", valid_activity_ops()), done(StopReason::ToolUse)],
            vec![tool_use("ls_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![LlmEvent::TextDelta("已发布".into()), done(StopReason::EndTurn)],
        ]
    }

    /// 从一轮请求的完整历史里按序抽 ls_set_section_body 的 (key, body)。
    fn section_bodies_in_history(messages: &[nomi_types::message::Message]) -> Vec<(String, String)> {
        let mut pairs = Vec::new();
        for message in messages {
            for block in &message.content {
                if let ContentBlock::ToolUse { name, input, .. } = block {
                    if name == "ls_set_section_body" {
                        pairs.push((
                            input["section_key"].as_str().unwrap_or("").to_owned(),
                            input["body"].as_str().unwrap_or("").to_owned(),
                        ));
                    }
                }
            }
        }
        pairs
    }

    /// u06 off 臂逐字节回归（缺省 off = 基线）：历史全量重发，两节正文在
    /// 每一轮历史里逐字在场（同二进制对照臂——正文参数永不被改写）。
    #[tokio::test]
    #[serial_test::serial]
    async fn lesson_history_compact_off_arm_keeps_full_bodies_in_history() {
        let _env = lesson_compact_env_guard("off");
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service));
        let provider = ScriptedProvider::new(two_section_bodies_script());
        run_loops(
            &engine(Arc::clone(&service)),
            provider.clone(),
            "test-model",
            &lesson_user_text(&ctx.context),
            Arc::clone(&ctx),
            BUDGETS.round_tokens,
        )
        .await
        .expect("off 臂脚本照常发布");
        let rounds = provider.seen_messages.lock().unwrap();
        // 期望序列（写入序）；任意一轮历史里的正文必须是它的**逐字前缀**
        // （早期轮次只有 s1，s2 落库后才齐全——但每一处正文都逐字在场，
        // 从不被压缩）。
        let expected = [("s1", concept_body()), ("s2", practice_body())];
        for (index, round) in rounds.iter().enumerate() {
            let bodies = section_bodies_in_history(round);
            if bodies.is_empty() {
                continue; // 正文写入轮之前的历史没有该工具调用
            }
            assert!(
                bodies.len() <= expected.len(),
                "第 {} 轮正文调用数超纲: {bodies:?}",
                index + 1
            );
            for (actual, (key, body)) in bodies.iter().zip(expected.iter()) {
                assert_eq!(actual.0, *key, "第 {} 轮 key 序", index + 1);
                assert_eq!(
                    actual.1, *body,
                    "off 臂第 {} 轮历史必须逐字保留正文（基线行为）",
                    index + 1
                );
            }
        }
    }

    /// u06 on 臂：已落库正文在历史中换摘要——非最近写入的 s1 换占位符
    /// （落库读数 + ls_inspect 取回指引，原正文退场），最近一次写入的 s2
    /// 保留原文（紧邻修改仍有上下文）。发布结果与 off 臂完全一致（压缩只
    /// 改历史视图，不改草稿）。
    #[tokio::test]
    #[serial_test::serial]
    async fn lesson_history_compact_on_arm_replaces_stored_bodies_but_keeps_latest() {
        let _env = lesson_compact_env_guard("on");
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service));
        let provider = ScriptedProvider::new(two_section_bodies_script());
        let output = run_loops(
            &engine(Arc::clone(&service)),
            provider.clone(),
            "test-model",
            &lesson_user_text(&ctx.context),
            Arc::clone(&ctx),
            BUDGETS.round_tokens,
        )
        .await
        .expect("on 臂脚本照常发布");
        assert_eq!(output.sections.len(), 2, "压缩不改变草稿本身");

        let rounds = provider.seen_messages.lock().unwrap();
        // 压缩收敛在最后一轮历史：s1 换占位符、s2 原文在场。
        let last = rounds.last().expect("最后一轮请求");
        let bodies = section_bodies_in_history(last);
        assert_eq!(bodies.len(), 2, "两节写入都在历史: {bodies:?}");
        assert_eq!(bodies[0].0, "s1");
        let placeholder = &bodies[0].1;
        assert!(placeholder.contains("已落库：s1"), "占位符带节 key: {placeholder}");
        assert!(
            placeholder.contains("ls_inspect") && placeholder.contains("section_key"),
            "占位符带取回指引: {placeholder}"
        );
        let s1_chars = concept_body().chars().filter(|c| !c.is_whitespace()).count();
        assert!(
            placeholder.contains(&s1_chars.to_string()),
            "占位符带落库字数（{s1_chars}）: {placeholder}"
        );
        assert!(
            !placeholder.contains("期权的定义"),
            "原正文必须退场: {placeholder}"
        );
        assert_eq!(bodies[1].1, practice_body(), "最近一次写入保留原文");

        // 早期轮次（s1 刚写入时它就是最近写入）：正文原样在场——保留规则
        // 按写入时点判定，不回溯改写当时的保留件。
        let s1_write_round = rounds
            .iter()
            .position(|round| !section_bodies_in_history(round).is_empty())
            .expect("正文写入轮在场");
        let mid = section_bodies_in_history(&rounds[s1_write_round]);
        assert!(
            mid.iter().any(|(key, body)| key == "s1" && *body == concept_body()),
            "s1 刚写入的轮次保留原文（当时它是最近写入）: {mid:?}"
        );
    }

    /// u06 on 臂的取回通道：ls_inspect 可选 section_key 返回该节完整正文
    /// （历史被压缩后按需取回原文）；未知 key 可读退稿；缺席 = 基线概览。
    #[tokio::test]
    #[serial_test::serial]
    async fn ls_inspect_section_key_returns_full_body_on_compact_arm() {
        let _env = lesson_compact_env_guard("on");
        let (service, _dir) = test_service().await;
        let (ctx, draft_slot, _published) = context(Arc::clone(&service));
        let view = service.create_lesson_draft(ctx.context.clone()).unwrap();
        *draft_slot.lock().unwrap() = Some(view.draft_id.clone());
        service
            .patch_lesson_draft(
                &view.draft_id,
                vec![
                    nomifun_learning::LessonOp::SetSectionManifest {
                        sections: serde_json::from_value(section_manifest_ops()["sections"].clone())
                            .unwrap(),
                        capacity_note: None,
                    },
                    nomifun_learning::LessonOp::SetSectionBody {
                        section_key: "s1".into(),
                        body: concept_body(),
                    },
                ],
            )
            .unwrap();
        let tool = lesson_content_tools(Arc::clone(&ctx), false)
            .into_iter()
            .find(|tool| tool.name == "ls_inspect")
            .unwrap();

        // 带 section_key：完整正文在场（压缩后按需取回）。回包是 JSON——
        // 按 JSON 解析比对（正文里的换行在 wire 上是转义序列）。
        let reply = (tool.handler)(serde_json::json!({ "section_key": "s1" }))
            .await
            .expect("取回通道");
        let parsed: serde_json::Value =
            serde_json::from_str(&reply).expect("取回通道回 JSON");
        assert_eq!(parsed["section_key"], "s1", "{reply}");
        assert_eq!(
            parsed["body"],
            serde_json::json!(concept_body().trim()),
            "返回完整正文（逐字；草稿存 trim 后正文）: {reply}"
        );
        assert!(
            parsed["chars"].as_u64().unwrap_or(0)
                == concept_body().chars().filter(|c| !c.is_whitespace()).count() as u64,
            "chars 用草稿同一口径: {reply}"
        );
        // 未知 key：可读 Err。
        let error = (tool.handler)(serde_json::json!({ "section_key": "s9" }))
            .await
            .expect_err("未知节退稿");
        assert!(error.contains("s9"), "{error}");
        // 缺席 = 基线概览视图。
        let overview = (tool.handler)(serde_json::json!({})).await.unwrap();
        assert!(overview.contains("section_bodies"), "概览视图照旧: {overview}");
    }

    /// u06 off 臂（缺省）：ls_inspect 描述与 schema 逐字回基线（独立逐字
    /// 副本为事实源），section_key 入参被无视（基线语义 = 概览视图）。
    #[tokio::test]
    #[serial_test::serial]
    async fn ls_inspect_off_arm_contract_is_byte_identical_baseline() {
        let _env = lesson_compact_env_guard("off");
        let (service, _dir) = test_service().await;
        let (ctx, draft_slot, _published) = context(Arc::clone(&service));
        // 活动草稿（概览 handler 需要 require_draft；空草稿即可）。
        let view = service.create_lesson_draft(ctx.context.clone()).unwrap();
        *draft_slot.lock().unwrap() = Some(view.draft_id.clone());
        let tool = lesson_content_tools(Arc::clone(&ctx), false)
            .into_iter()
            .find(|tool| tool.name == "ls_inspect")
            .unwrap();
        assert_eq!(
            tool.description,
            "当前草稿概览：文档字符数与章节清单、活动清单（position/kind/prompt 摘要）、estimated_minutes、审计 findings。每次修改前后调用，保持全局认知；update/remove 活动的 position 以这里为准。",
            "off 臂描述逐字 = 基线"
        );
        assert_eq!(
            tool.input_schema,
            serde_json::json!({ "type": "object", "properties": {} }),
            "off 臂 schema 逐字 = 基线（无 section_key 键）"
        );
        // 即使模型带了 section_key（旧 schema 不含此键，宽容多余字段），off
        // 臂行为 = 基线概览，绝不触取回通道。
        let reply = (tool.handler)(serde_json::json!({ "section_key": "s1" }))
            .await
            .unwrap();
        assert!(reply.contains("revision"), "off 臂返回概览视图: {reply}");
    }

    // ── u07：档位优先序条款 / 发布前数学失败可见 / 修复轮课时上下文 ─────

    /// u07 档位优先序条款双臂（纯函数面，env 不进单测）：off 臂 = 空串
    /// （模板逐字节回基线）；on 臂 = 单行条款，点名知道档的三条收敛
    /// （认识性讲解 / 不引入逐步实算推导 / 数值例至多 1 须 verify_math）
    /// 与密度/完备面条款的档位限定，且带「不空心化」护栏半句。
    #[test]
    fn tier_priority_clause_only_on_tier_arm() {
        let off = tier_priority_clause(false);
        assert_eq!(off, "", "off 臂 = 空串（模板逐字节回基线）");
        let on = tier_priority_clause(true);
        assert!(on.starts_with("- 档位优先序"), "{on}");
        assert!(on.contains("认识性讲解"), "{on}");
        assert!(on.contains("逐步实算"), "{on}");
        assert!(on.contains("数值例至多 1"), "{on}");
        assert!(on.contains("verify_math"), "{on}");
        assert!(on.contains("会用/能教"), "深度条款限定到会用/能教档: {on}");
        // 护栏半句（推翻条件的方向锚）：不为凑密度写推导课，也不删成空心。
        assert!(on.contains("只剩定义"), "{on}");
        // 单行条款（模板插位在「档位决定内容形态」行之后）：恰一个换行且在尾。
        assert_eq!(on.matches('\n').count(), 1, "{on}");
        assert!(on.ends_with('\n'), "{on}");
    }

    /// u07 机械（发布前数学失败可见，不挂开关）：生成会话存在未复核修正
    /// 失败时，**首次** ls_finish 被可读 Err 打回——失败条目（verdict 原文：
    /// reason/computed/counterexample）+ 修正指引随打回可见；模型复核闭环
    /// 后重新 ls_finish 发布成功。打回恰一次（防死循环闸）。
    #[tokio::test]
    async fn math_failure_visible_before_publish_with_one_fix_chance() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service));
        let provider = ScriptedProvider::new(vec![
            vec![tool_use("ls_start", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                tool_use("ls_set_section_manifest", section_manifest_ops()),
                done(StopReason::ToolUse),
            ],
            // 恒错 identity → Fail verdict（此后无复核 Pass → unresolved=1）。
            vec![
                tool_use(
                    "verify_math",
                    serde_json::json!({
                        "kind": "identity", "lhs": "x^2", "rhs": "(x+1)^2", "variables": ["x"]
                    }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use(
                    "ls_set_section_body",
                    serde_json::json!({ "section_key": "s1", "body": concept_body() }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use(
                    "ls_set_section_body",
                    serde_json::json!({ "section_key": "s2", "body": practice_body() }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![tool_use("ls_patch_activities", valid_activity_ops()), done(StopReason::ToolUse)],
            // 首次 ls_finish：被 u07 打回（失败条目可见），草稿保留。
            vec![tool_use("ls_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            // 修正轮：按反例改正后复核 Pass（unresolved 清零）。
            vec![
                tool_use(
                    "verify_math",
                    serde_json::json!({
                        "kind": "identity", "lhs": "x^2", "rhs": "x*x", "variables": ["x"]
                    }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![tool_use("ls_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![LlmEvent::TextDelta("已发布".into()), done(StopReason::EndTurn)],
        ]);
        let output = run_loops(
            &engine(Arc::clone(&service)),
            provider.clone(),
            "test-model",
            &lesson_user_text(&ctx.context),
            Arc::clone(&ctx),
            BUDGETS.round_tokens,
        )
        .await
        .expect("复核闭环后发布成功");
        assert_eq!(output.sections.len(), 2);

        // 打回面：恰一次，失败条目（反例证据）与修正指引随打回可见。
        // 计数口径 = **最后一轮请求的完整历史**（seen_messages 每轮带全量
        // 历史，逐轮扫描会把同一条 ToolResult 重复计数）。
        let rounds = provider.seen_messages.lock().unwrap();
        let rejections: Vec<String> = rounds
            .last()
            .expect("最后一轮请求")
            .iter()
            .filter_map(|message| match &message.content[0] {
                ContentBlock::ToolResult { is_error: true, content, .. } => {
                    Some(content.clone())
                }
                _ => None,
            })
            .filter(|content| content.contains("发布前数学失败可见"))
            .collect();
        assert_eq!(
            rejections.len(),
            1,
            "打回恰一次（修正机会仅一次，防死循环）: {rejections:?}"
        );
        assert!(
            rejections[0].contains("counterexample"),
            "失败条目带 verdict 原文（反例证据）: {}",
            rejections[0]
        );
        assert!(
            rejections[0].contains("采样核验失败"),
            "失败条目带死因: {}",
            rejections[0]
        );
        assert!(
            rejections[0].contains("verify_math"),
            "带修正指引（R14 复核闭环）: {}",
            rejections[0]
        );
        // 计数收尾：累计 failed=1 保留（观测读数面），复核 Pass 清零未修正
        // 数——finish findings 零 warning。
        let counters = ctx.math_verify.snapshot_json();
        assert_eq!(counters["failed"], 1, "{counters}");
        assert_eq!(counters["unresolved"], 0, "{counters}");
    }

    /// u07 机械的另一半：模型无视打回（不做复核）直接再次 ls_finish——
    /// 第二次**不再打回**（修正机会已用尽，防死循环闸），发布照常走完
    /// （未复核失败以 `math_verify_failed` warning 留痕，不阻断——基线
    /// 语义）。
    #[tokio::test]
    async fn math_failure_nudge_fires_exactly_once_then_publishes() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service));
        let provider = ScriptedProvider::new(vec![
            vec![tool_use("ls_start", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                tool_use("ls_set_section_manifest", section_manifest_ops()),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use(
                    "verify_math",
                    serde_json::json!({
                        "kind": "identity", "lhs": "x^2", "rhs": "(x+1)^2", "variables": ["x"]
                    }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use(
                    "ls_set_section_body",
                    serde_json::json!({ "section_key": "s1", "body": concept_body() }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use(
                    "ls_set_section_body",
                    serde_json::json!({ "section_key": "s2", "body": practice_body() }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![tool_use("ls_patch_activities", valid_activity_ops()), done(StopReason::ToolUse)],
            // 首次 ls_finish：u07 打回。
            vec![tool_use("ls_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            // 无视打回：直接再次 ls_finish（unresolved 仍 1）→ 不再打回，
            // 发布成功（warning 留痕走服务面，不挡发布）。
            vec![tool_use("ls_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![LlmEvent::TextDelta("已发布".into()), done(StopReason::EndTurn)],
        ]);
        let output = run_loops(
            &engine(Arc::clone(&service)),
            provider.clone(),
            "test-model",
            &lesson_user_text(&ctx.context),
            Arc::clone(&ctx),
            BUDGETS.round_tokens,
        )
        .await
        .expect("第二次 ls_finish 不再打回，发布照常");
        assert_eq!(output.activities.len(), 3);

        let rejections: Vec<String> = provider
            .seen_messages
            .lock()
            .unwrap()
            .last()
            .expect("最后一轮请求")
            .iter()
            .filter_map(|message| match &message.content[0] {
                ContentBlock::ToolResult { is_error: true, content, .. } => {
                    Some(content.clone())
                }
                _ => None,
            })
            .filter(|content| content.contains("发布前数学失败可见"))
            .collect();
        assert_eq!(rejections.len(), 1, "恰一次: {rejections:?}");
        // 发布面留痕（基线语义不变）：未复核失败数随 finish 进 warning。
        let counters = ctx.math_verify.snapshot_json();
        assert_eq!(counters["unresolved"], 1, "{counters}");
    }

    /// u07 机械（修复轮带课时上下文，不挂开关）：修复轮是全新对话——开场
    /// user 轮注入 lesson_user_text 同源的课时上下文（课程/节点定位/档位
    /// 行/前置路径在案），审计报告随后。
    #[tokio::test]
    async fn repair_round_opens_with_lesson_context_then_audit() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service));
        let provider = ScriptedProvider::new(vec![
            // ── 生成 loop：规划 + 写一节 + 过早 finish（缺节 danger）──
            vec![tool_use("ls_start", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                tool_use("ls_set_section_manifest", section_manifest_ops()),
                done(StopReason::ToolUse),
            ],
            vec![tool_use("ls_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                LlmEvent::TextDelta("发布被拒，还有节没写".into()),
                done(StopReason::EndTurn),
            ],
            // ── 修复 loop 1：补写缺的两节 ──
            vec![
                tool_use(
                    "ls_set_section_body",
                    serde_json::json!({ "section_key": "s1", "body": concept_body() }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use(
                    "ls_set_section_body",
                    serde_json::json!({ "section_key": "s2", "body": practice_body() }),
                ),
                done(StopReason::ToolUse),
            ],
            // ── 修复 loop 2：补活动 ──
            vec![tool_use("ls_patch_activities", valid_activity_ops()), done(StopReason::ToolUse)],
            vec![LlmEvent::TextDelta("已补齐".into()), done(StopReason::EndTurn)],
        ]);
        run_loops(
            &engine(Arc::clone(&service)),
            provider.clone(),
            "test-model",
            &lesson_user_text(&ctx.context),
            Arc::clone(&ctx),
            BUDGETS.round_tokens,
        )
        .await
        .expect("修复后发布");

        // 修复轮请求 = 单消息全新对话，user 轮含「课时草稿审计」即修复轮。
        let rounds = provider.seen_messages.lock().unwrap();
        let repair_requests: Vec<&nomi_types::message::Message> = rounds
            .iter()
            .filter_map(|round| round.first())
            .filter(|message| {
                matches!(
                    &message.content[0],
                    ContentBlock::Text { text, .. } if text.contains("课时草稿审计")
                )
            })
            .collect();
        assert!(
            !repair_requests.is_empty(),
            "修复轮请求在场（finish 被拒后进入修复循环）"
        );
        for message in &repair_requests {
            let text = match &message.content[0] {
                ContentBlock::Text { text, .. } => text.as_str(),
                _ => unreachable!(),
            };
            assert!(
                text.starts_with("课程："),
                "修复轮开场 = 课时上下文先行（lesson_user_text 同源）: {text}"
            );
            let context_end = text.find("课时草稿审计").expect("审计报告随后");
            assert!(
                text.find("课程：").expect("上下文在先") < context_end,
                "审计报告在上下文之后: {text}"
            );
            // 上下文里档位/定位面在案（修复轮此前不可达的信息）。
            assert!(text.contains("课时（"), "课时坐标在案: {text}");
            assert!(text.contains("节点定位") || text.contains("课时目标"), "{text}");
        }
    }

    /// f3 读数修复 + w1 计数槽（蓝图 4.3/A5）+ w1 评审修复的 unresolved 面：
    /// 发布观测帧（calls.jsonl 的 `lesson_agent_publish` 行）必须带
    /// viz_lint_events（此前只进 WS publish_ok 帧——smoke 读数结构性恒 0 的
    /// 缺陷）、math_verify 累计计数（读数面）与未复核修正数（finish
    /// findings 口径，smoke 判据⑧硬门的取数面）。absorb-8 z03 追加
    /// scope_findings 计数（报告-only 读数源）。
    #[test]
    fn lesson_publish_observation_frame_carries_viz_lint_and_math_verify() {
        let output = LessonOutput {
            summary: String::new(),
            estimated_minutes: 15,
            activities: Vec::new(),
            sections: Vec::new(),
            degraded_keys: Vec::new(),
            capacity_note: None,
            viz_lint_events: 3,
            scope_findings: 1,
        };
        let counters = MathVerifyCounters::default();
        counters.record(&ClaimVerdict::Pass { value: Some(2.0) });
        counters.record(&ClaimVerdict::Fail {
            reason: "boom".into(),
            computed: None,
            counterexample: None,
        });
        let frame = lesson_publish_observation_frame(&output, &counters);
        assert_eq!(frame["method"], "lesson_agent_publish");
        assert_eq!(
            frame["viz_lint_events"], 3,
            "f3：viz lint 事件计数随观测帧披露（calls.jsonl 读数源）"
        );
        assert_eq!(
            frame["scope_findings"], 1,
            "z03：scope 边界词面 findings 计数随观测帧披露（报告-only 读数源）"
        );
        assert_eq!(frame["math_verify"]["calls"], 2);
        assert_eq!(frame["math_verify"]["passed"], 1);
        assert_eq!(frame["math_verify"]["failed"], 1);
        assert_eq!(
            frame["math_verify"]["unresolved"], 1,
            "未复核修正数随帧披露（判据⑧ finish findings 口径的取数面）"
        );
    }

    /// 全循环打回自修改样（w1 评审票面缺口，照
    /// `incomplete_sectioned_draft_is_blocked_until_repaired_and_published`
    /// 模子）：某轮调 verify_math（identity）得 Fail verdict——**Ok** 结果
    /// 携带 computed/counterexample（核验失败是正常结论不是工具故障，A3）
    /// ——模型下一轮按反例改正 claim 重新复核过（R14 自修改闭环），随后
    /// 正文落库、活动补齐、ls_finish 过闸发布。钉住 map.md 验证总纲第 1 条
    /// 「核验失败→打回→自修改→过闸」的循环内全链。
    #[tokio::test]
    async fn verify_math_fail_verdict_feeds_back_and_self_correction_publishes() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service));

        let provider = ScriptedProvider::new(vec![
            vec![tool_use("ls_start", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                tool_use("ls_set_section_manifest", section_manifest_ops()),
                done(StopReason::ToolUse),
            ],
            // 核验失败轮：恒错 identity → Fail verdict 以 Ok ToolResult 回模型。
            vec![
                tool_use(
                    "verify_math",
                    serde_json::json!({
                        "kind": "identity", "lhs": "x^2", "rhs": "(x+1)^2", "variables": ["x"]
                    }),
                ),
                done(StopReason::ToolUse),
            ],
            // 自修改轮：按反例改正后的恒等式重新提交复核 → Pass（闭环）。
            vec![
                tool_use(
                    "verify_math",
                    serde_json::json!({
                        "kind": "identity", "lhs": "x^2", "rhs": "x*x", "variables": ["x"]
                    }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use(
                    "ls_set_section_body",
                    serde_json::json!({ "section_key": "s1", "body": concept_body() }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use(
                    "ls_set_section_body",
                    serde_json::json!({ "section_key": "s2", "body": practice_body() }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![tool_use("ls_patch_activities", valid_activity_ops()), done(StopReason::ToolUse)],
            vec![tool_use("ls_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![LlmEvent::TextDelta("已发布".into()), done(StopReason::EndTurn)],
        ]);
        let output = run_loops(
            &engine(Arc::clone(&service)),
            provider.clone(),
            "test-model",
            &lesson_user_text(&ctx.context),
            Arc::clone(&ctx),
            BUDGETS.round_tokens,
        )
        .await
        .unwrap();
        assert_eq!(output.activities.len(), 3, "self-corrected lesson publishes");
        assert_eq!(output.sections.len(), 2);

        // Fail verdict 的回灌面：Ok ToolResult（is_error=false）携带 verdict
        // JSON 的 computed/counterexample 槽位与反例证据——模型据此自修改。
        let rounds = provider.seen_messages.lock().unwrap();
        let fail_result = rounds
            .iter()
            .flat_map(|round| round.iter())
            .find_map(|message| match &message.content[0] {
                ContentBlock::ToolResult { is_error: false, content, .. }
                    if content.contains("\"kind\":\"fail\"") && content.contains("counterexample") =>
                {
                    Some(content.clone())
                }
                _ => None,
            })
            .expect("identity Fail verdict must come back as a successful ToolResult");
        assert!(fail_result.contains("computed"), "{fail_result}");
        assert!(fail_result.contains("采样核验失败"), "{fail_result}");
        assert!(
            fail_result.contains("lhs=") && fail_result.contains("rhs="),
            "counterexample 带两侧实算值：{fail_result}"
        );
        // 复核轮的 Pass verdict 也在场（自修改闭环的另一半）。
        assert!(
            rounds
                .iter()
                .flat_map(|round| round.iter())
                .any(|message| matches!(
                    &message.content[0],
                    ContentBlock::ToolResult { is_error: false, content, .. }
                        if content.contains("\"kind\":\"pass\"")
                )),
            "the corrected claim must re-verify to a Pass verdict"
        );

        // 计数槽收尾：累计 failed=1（观测读数面保留），复核 Pass 把未复核
        // 修正数清零——finish findings 零 `math_verify_failed` warning（判据
        // ⑧ Q1 口径：自修正会话必须绿）。
        let counters = ctx.math_verify.snapshot_json();
        assert_eq!(counters["calls"], 2, "{counters}");
        assert_eq!(counters["passed"], 1, "{counters}");
        assert_eq!(counters["failed"], 1, "{counters}");
        assert_eq!(counters["unresolved"], 0, "{counters}");
    }

    /// 分节草稿缺节就过早 ls_finish（section_body_missing danger）→ 发布被
    /// 门禁拒绝 → 修复 loop 补齐剩余节与活动 → 下一轮门禁通过并发布。
    #[tokio::test]
    async fn incomplete_sectioned_draft_is_blocked_until_repaired_and_published() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service));

        let provider = ScriptedProvider::new(vec![
            // ── generation loop: plan two sections, write one, finish early ──
            vec![tool_use("ls_start", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                tool_use("ls_set_section_manifest", section_manifest_ops()),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use(
                    "ls_set_section_body",
                    serde_json::json!({ "section_key": "s1", "body": concept_body() }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![tool_use("ls_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                LlmEvent::TextDelta("发布被拒，还有节与题目没写".into()),
                done(StopReason::EndTurn),
            ],
            // ── repair loop 1: write the remaining section ──
            vec![
                tool_use(
                    "ls_set_section_body",
                    serde_json::json!({ "section_key": "s2", "body": practice_body() }),
                ),
                done(StopReason::ToolUse),
            ],
            // ── repair loop 2: add the activities ──
            vec![tool_use("ls_patch_activities", valid_activity_ops()), done(StopReason::ToolUse)],
            vec![LlmEvent::TextDelta("已补齐".into()), done(StopReason::EndTurn)],
        ]);
        let output = run_loops(
            &engine(Arc::clone(&service)),
            provider.clone(),
            "test-model",
            &lesson_user_text(&ctx.context),
            ctx,
            BUDGETS.round_tokens,
            )
            .await
            .unwrap();
        assert!(output.summary.contains("## 练习"));
        assert_eq!(output.sections.len(), 2, "both section bodies survived");
        assert_eq!(output.activities.len(), 3, "repaired lesson publishes");
        assert_eq!(output.estimated_minutes, 15);
        // The premature ls_finish was rejected with the blocking report.
        let rounds = provider.seen_messages.lock().unwrap();
        let rejected = rounds.iter().flat_map(|round| round.iter()).any(|message| {
            matches!(
                &message.content[0],
                ContentBlock::ToolResult { is_error: true, content, .. }
                    if content.contains("audit gate") && content.contains("section_body_missing")
            )
        });
        assert!(rejected, "an incomplete sectioned draft must be rejected by the audit gate");
    }

    /// 修复 loop：生成 loop 规划并写完全部节但没有题目就宣告完成——发布被
    /// 门禁阻塞；修复 loop 补齐活动后由门禁放行发布。修复 loop 的工具面
    /// 必须不含 ls_start。
    #[tokio::test]
    async fn repair_loop_fixes_danger_findings_before_publish() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(Arc::clone(&service));

        let provider = ScriptedProvider::new(vec![
            // ── generation loop: sections complete, activities missing ──
            vec![tool_use("ls_start", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                tool_use("ls_set_section_manifest", section_manifest_ops()),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use(
                    "ls_set_section_body",
                    serde_json::json!({ "section_key": "s1", "body": concept_body() }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use(
                    "ls_set_section_body",
                    serde_json::json!({ "section_key": "s2", "body": practice_body() }),
                ),
                done(StopReason::ToolUse),
            ],
            // The model declares the generation done WITHOUT activities and
            // WITHOUT ls_finish; the gate blocks the publish and the repair
            // loop starts.
            vec![LlmEvent::TextDelta("done".into()), done(StopReason::EndTurn)],
            // ── repair loop: add the activities and publish in-loop ──
            vec![tool_use("ls_patch_activities", valid_activity_ops()), done(StopReason::ToolUse)],
            vec![tool_use("ls_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            // The publish result comes back as a tool result; the model
            // wraps up with one final (text-only) round.
            vec![LlmEvent::TextDelta("已发布".into()), done(StopReason::EndTurn)],
        ]);

        let output = run_loops(
            &engine(Arc::clone(&service)),
            provider.clone(),
            "test-model",
            &lesson_user_text(&ctx.context),
            ctx,
            BUDGETS.round_tokens,
            )
            .await
            .unwrap();
        assert!(output.summary.contains("## 验证") || output.summary.contains("## 练习"));
        assert_eq!(output.activities.len(), 3);

        let seen = provider.seen_tool_names.lock().unwrap();
        assert_eq!(seen.len(), 8, "4 generation rounds + 3 repair rounds + 1 wrap-up");
        for round in &seen[5..] {
            assert!(
                !round.iter().any(|name| name == "ls_start"),
                "the repair loop must never expose ls_start: {round:?}"
            );
        }
    }

    /// 模型从未调用 ls_start 就宣告完成：无草稿可发布，明确报错。
    #[tokio::test]
    async fn finishing_without_a_draft_fails() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(service.clone());
        let provider = ScriptedProvider::new(vec![vec![
            LlmEvent::TextDelta("nothing to do".into()),
            done(StopReason::EndTurn),
        ]]);
        let error = run_loops(
            &engine(service),
            provider,
            "test-model",
            &lesson_user_text(&ctx.context),
            ctx,
            BUDGETS.round_tokens,
            )
            .await
            .unwrap_err();
        assert!(matches!(&error, AppError::Internal(message) if message.contains("without creating a draft")));
    }

    /// 修复预算耗尽：每次修复 loop 后审计仍有 danger，最终返回阻塞报告
    /// 而非静默失败。
    #[tokio::test]
    async fn repair_budget_exhaustion_reports_blocking_findings() {
        let (service, _dir) = test_service().await;
        let (ctx, _draft, _published) = context(service.clone());
        let provider = ScriptedProvider::new(vec![
            // generation loop: manifest only, never the bodies or activities
            vec![tool_use("ls_start", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![
                tool_use("ls_set_section_manifest", section_manifest_ops()),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("done".into()), done(StopReason::EndTurn)],
            // repair loops 1-3: refuse to touch anything, then stop
            vec![LlmEvent::TextDelta("cannot fix".into()), done(StopReason::EndTurn)],
            vec![LlmEvent::TextDelta("cannot fix".into()), done(StopReason::EndTurn)],
            vec![LlmEvent::TextDelta("cannot fix".into()), done(StopReason::EndTurn)],
        ]);
        let error = run_loops(
            &engine(service),
            provider.clone(),
            "test-model",
            &lesson_user_text(&ctx.context),
            ctx,
            BUDGETS.round_tokens,
            )
            .await
            .unwrap_err();
        assert!(matches!(&error, AppError::UnprocessableEntity(message) if message.contains("exhausted 3 repair loops")));
        assert!(
            error.to_string().contains("section_body_missing"),
            "the blocking report names the finding"
        );
        let seen = provider.seen_tool_names.lock().unwrap();
        assert_eq!(seen.len(), 6, "2 generation rounds + 1 idle round + 3 repair rounds");
    }

    /// 旧单篇草稿的修复路径仍然可用：ls_set_document 只保留在修复工具面，
    /// 续跑遗留的单篇草稿时，修复轮重写文档、补活动后照常过门发布。
    #[tokio::test]
    async fn repair_face_still_offers_ls_set_document_for_legacy_drafts() {
        let (service, _dir) = test_service().await;
        let engine = engine(Arc::clone(&service));

        // 预置遗留草稿：旧单篇契约（文档已过文档门，活动缺失）。
        let view = service.create_lesson_draft(lesson_context()).unwrap();
        let draft_id = view.draft_id.clone();
        service
            .patch_lesson_draft(
                &draft_id,
                vec![LessonOp::SetDocument {
                    document: long_document(),
                }],
            )
            .unwrap();

        let ctx = Arc::new(LoopContext {
            service: Arc::clone(&service),
            context: lesson_context(),
            draft_slot: Arc::new(Mutex::new(Some(draft_id))),
            published_slot: Arc::new(Mutex::new(None)),
            channel: LoopChannel::new(WIRE, BUDGETS.generate_max_rounds),
            math_verify: Arc::new(MathVerifyCounters::default()),
            stored_writes: Arc::new(Mutex::new(StoredWrites::default())),
        });
        let provider = ScriptedProvider::new(vec![
            // ── generation loop: finish immediately, blocked (no activities) ──
            vec![tool_use("ls_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![LlmEvent::TextDelta("需要补活动".into()), done(StopReason::EndTurn)],
            // ── repair loop 1: rewrite the document via the legacy contract ──
            vec![
                tool_use(
                    "ls_set_document",
                    serde_json::json!({ "document": long_document() }),
                ),
                done(StopReason::ToolUse),
            ],
            vec![LlmEvent::TextDelta("文档已重写".into()), done(StopReason::EndTurn)],
            // ── repair loop 2: add the activities ──
            vec![tool_use("ls_patch_activities", valid_activity_ops()), done(StopReason::ToolUse)],
            vec![LlmEvent::TextDelta("已补齐活动".into()), done(StopReason::EndTurn)],
        ]);
        let output = run_loops(
            &engine,
            provider,
            "test-model",
            &lesson_user_text(&ctx.context),
            ctx,
            BUDGETS.round_tokens,
            )
            .await
            .unwrap();
        assert!(output.summary.contains("## 描述"));
        assert_eq!(output.activities.len(), 3);
    }

    /// 续跑：预置存活草稿（manifest 已规划、部分节已写）后重入生成循环
    /// ——开场注入草稿状态与上次轮次日志，模型接着写完剩余节并发布；
    /// 发布后轮次日志清除（会话干净收尾）。
    #[tokio::test]
    async fn resume_continues_from_surviving_draft() {
        let (service, _dir) = test_service().await;
        let engine = engine(Arc::clone(&service));

        // 预置中断点：草稿由 ls_set_section_manifest + 一节正文构成。
        let view = service.create_lesson_draft(lesson_context()).unwrap();
        let draft_id = view.draft_id.clone();
        service
            .patch_lesson_draft(
                &draft_id,
                vec![
                    LessonOp::SetSectionManifest {
                        sections: serde_json::from_value(serde_json::json!([
                            { "section_key": "s1", "kind": "concept", "title": "概念：期权的定义", "points": "权利义务不对称", "visual": "表格" },
                            { "section_key": "s2", "kind": "practice", "title": "练习：期权判断", "points": "判断题自测", "visual": "无" }
                        ]))
                        .unwrap(),
                        capacity_note: None,
                    },
                    LessonOp::SetSectionBody {
                        section_key: "s1".into(),
                        body: format!(
                            "## 概念：期权的定义\n\n| 头寸 | 权利 |\n| --- | --- |\n| 买方 | 有 |\n\n{}\n",
                            "这是一个足够长的正文段落，用于通过节级质检门的长度下限要求。".repeat(12)
                        ),
                    },
                ],
            )
            .unwrap();
        // 归档上次会话的轮次日志（中断前的进度轨迹）。
        engine
            .round_logs
            .lock()
            .unwrap()
            .insert(draft_id.clone(), vec!["第3轮(生成): ls_set_section_body ✓ 写完s1".into()]);

        let ctx = Arc::new(LoopContext {
            service: Arc::clone(&service),
            context: lesson_context(),
            draft_slot: Arc::new(Mutex::new(Some(draft_id.clone()))),
            published_slot: Arc::new(Mutex::new(None)),
            channel: LoopChannel::new(WIRE, BUDGETS.generate_max_rounds),
            math_verify: Arc::new(MathVerifyCounters::default()),
            stored_writes: Arc::new(Mutex::new(StoredWrites::default())),
        });

        let provider = ScriptedProvider::new(vec![
            // 生成轮：直接写剩余的练习节并发布
            vec![
                tool_use("ls_set_section_body", serde_json::json!({
                    "section_key": "s2",
                    "body": "## 练习：期权判断\n\n完成以下判断题，检验你对权利义务不对称与四类基本头寸的理解；作答后阅读解析，回顾买方权利与卖方义务的关键区别。"
                })),
                done(StopReason::ToolUse),
            ],
            vec![
                tool_use("ls_patch_activities", valid_activity_ops()),
                done(StopReason::ToolUse),
            ],
            vec![tool_use("ls_finish", serde_json::json!({})), done(StopReason::ToolUse)],
            vec![LlmEvent::TextDelta("已发布".into()), done(StopReason::EndTurn)],
        ]);

        let output = run_loops(
            &engine,
            provider,
            "test-model",
            &lesson_user_text(&ctx.context),
            ctx,
            BUDGETS.round_tokens,
            )
            .await
            .expect("the resumed draft publishes");
        assert_eq!(output.sections.len(), 2, "the pre-written section survives");
        assert!(
            engine.round_logs.lock().unwrap().get(&draft_id).is_none(),
            "round logs are cleared after a successful publish"
        );
    }

    /// 续跑开场：草稿状态与轮次日志注入「上次会话进度」段，并引导先通读
    /// 草稿；新会话（generate）不出现该段。
    #[test]
    fn resume_opening_carries_draft_state_and_round_logs() {
        let mut context = lesson_context();
        context.clear_grounding();
        let resume_text = compose_resume_user_text(
            &context,
            r#"{"revision":3}"#,
            &["第3轮(生成): ls_set_section_body ✓ 写完s1".into()],
        );
        assert!(resume_text.contains("上次会话进度（1 轮后中断"), "{resume_text}");
        assert!(resume_text.contains("revision"), "{resume_text}");
        assert!(resume_text.contains("ls_inspect"), "{resume_text}");
        // 新会话（无日志）不出现该段。
        let fresh = lesson_user_text(&context);
        assert!(!fresh.contains("上次会话进度"), "{fresh}");
    }

    /// e4 门面重试环的反馈槽：resume 开场渲染【重试反馈】段（上次瞬断指纹
    /// + 段级幂等提示——认知面与 e1 的工具面执法同源）；无反馈不出现该段。
    #[test]
    fn resume_opening_carries_the_retry_feedback_slot() {
        let mut context = lesson_context();
        context.clear_grounding();
        context.retry_feedback = Some(
            "第 1 次尝试因瞬断中断（瞬断：连接被重置）；已写好的节已在草稿里。".into(),
        );
        let text = compose_resume_user_text(&context, r#"{"revision":2}"#, &[]);
        assert!(text.contains("【重试反馈】"), "{text}");
        assert!(text.contains("瞬断：连接被重置"), "{text}");

        let mut plain = lesson_context();
        plain.clear_grounding();
        let no_feedback = compose_resume_user_text(&plain, r#"{"revision":1}"#, &[]);
        assert!(!no_feedback.contains("【重试反馈】"), "{no_feedback}");
    }

    /// 发布帧披露 visual 声明分布（ADR-0008）：整课「无」独占即可疑的
    /// 纯文字课时，事后定位「没生成图表」类反馈时有据可查。f3 起同帧
    /// 披露 viz lint 事件计数（op 打回 + 终检剔除累计）。
    #[test]
    fn publish_frame_discloses_visual_distribution() {
        use crate::learning_loop::FlowCycle;
        use nomifun_learning::{SectionKind, SectionPack};

        let pack = |key: &str, kind: SectionKind, visual: &str| SectionPack {
            section_key: key.into(),
            kind,
            title: "t".into(),
            points: String::new(),
            visual: visual.into(),
            body_md: String::new(),
            degraded: false,
        };
        let output = LessonOutput {
            summary: String::new(),
            estimated_minutes: 15,
            activities: Vec::new(),
            sections: vec![
                pack("s1", SectionKind::Concept, "表格"),
                pack("s2", SectionKind::Practice, "无"),
                pack("s3", SectionKind::Summary, "无"),
            ],
            degraded_keys: Vec::new(),
            capacity_note: None,
            viz_lint_events: 3,
            scope_findings: 0,
        };
        let frame = <LiveLessonContentAgentEngine as FlowCycle>::publish_frame(
            &output,
            nomifun_learning::phase::REPAIR,
            Some(2),
        );
        assert_eq!(frame["phase"], nomifun_learning::phase::REPAIR);
        assert_eq!(frame["round"], 2);
        assert_eq!(frame["visuals"]["表格"], 1);
        assert_eq!(frame["visuals"]["无"], 2);
        assert_eq!(frame["viz_lint_events"], 3, "viz lint 事件计数随帧披露");
    }

    // ── search_concepts（y03）────────────────────────────────────────────

    /// 挂载条件：图线 ∧ 本节点 teaches 非空 → 生成面与修复面同备；teaches
    /// 空的图节点与传统线（graph = None）都不挂（防幽灵工具）。
    #[tokio::test]
    async fn search_concepts_is_mounted_only_for_teaches_bearing_graph_nodes() {
        let (service, _dir) = test_service().await;

        let (teaches_ctx, ..) = context_with(Arc::clone(&service), graph_teaches_context());
        for repair_face in [false, true] {
            let names: Vec<String> = lesson_content_tools(Arc::clone(&teaches_ctx), repair_face)
                .iter()
                .map(|tool| tool.name.clone())
                .collect();
            assert!(
                names.contains(&"search_concepts".to_owned()),
                "repair_face={repair_face} 也备只读检索工具：{names:?}"
            );
        }

        let (empty_ctx, ..) = context_with(Arc::clone(&service), graph_context(Vec::new()));
        for repair_face in [false, true] {
            let names: Vec<String> = lesson_content_tools(Arc::clone(&empty_ctx), repair_face)
                .iter()
                .map(|tool| tool.name.clone())
                .collect();
            assert!(
                !names.contains(&"search_concepts".to_owned()),
                "零 teaches 节点不挂：{names:?}"
            );
        }

        let (plain_ctx, ..) = context(service);
        let names: Vec<String> = lesson_content_tools(Arc::clone(&plain_ctx), false)
            .iter()
            .map(|tool| tool.name.clone())
            .collect();
        assert!(
            !names.contains(&"search_concepts".to_owned()),
            "传统线不挂：{names:?}"
        );
    }

    /// 图线 user 文本补一句自查提示（只读声明），同一句只出现一次且位于
    /// 档位/已知概念行之后；teaches 空与传统线不出现。
    #[test]
    fn lesson_user_text_appends_the_search_concepts_hint_only_for_teaches_nodes() {
        let teaches = lesson_user_text(&graph_teaches_context());
        assert_eq!(
            teaches.matches("search_concepts").count(),
            1,
            "同一句只出现一次：{teaches}"
        );
        assert!(teaches.contains("只读，不改登记表"), "{teaches}");
        assert!(
            teaches.find("节点档位").unwrap() < teaches.find("search_concepts").unwrap(),
            "提示位于档位行之后：{teaches}"
        );

        let empty_teaches = lesson_user_text(&graph_context(Vec::new()));
        assert!(
            !empty_teaches.contains("search_concepts"),
            "零 teaches 不出现：{empty_teaches}"
        );

        let traditional = lesson_user_text(&lesson_context());
        assert!(
            !traditional.contains("search_concepts"),
            "传统线不出现：{traditional}"
        );
    }

    /// 工具 schema（query 必填、mode 枚举 search/detail、concept_id）与
    /// handler 回包：命中带 canonical/matched_by、零命中 not_found 不抛错、
    /// detail 全别名、入参错误转模型可读 Err。
    #[tokio::test]
    async fn search_concepts_tool_schema_and_handlers() {
        let (service, _pool) = registry_service().await;
        let (ctx, ..) = context_with(service, graph_teaches_context());
        let tool = mounted_search_tool(&ctx, false);

        assert_eq!(
            tool.input_schema["properties"]["mode"]["enum"],
            serde_json::json!(["search", "detail"])
        );
        assert_eq!(
            tool.input_schema["required"],
            serde_json::json!(["query"])
        );
        assert!(
            tool.input_schema["properties"].get("concept_id").is_some(),
            "detail 用的 concept_id 字段应在 schema 面：{}",
            tool.input_schema
        );
        assert!(
            tool.description.contains("只读") && tool.description.contains("不改"),
            "描述说明只读自查：{}",
            tool.description
        );

        let hit: serde_json::Value = serde_json::from_str(
            &(tool.handler)(serde_json::json!({ "query": "极限" }))
                .await
                .expect("命中应 Ok"),
        )
        .unwrap();
        assert_eq!(hit["not_found"], false);
        assert_eq!(hit["hits"][0]["concept_id"], REGISTRY_CONCEPT);
        assert_eq!(hit["hits"][0]["canonical"], "极限");
        assert_eq!(hit["hits"][0]["matched_by"], "exact");

        // 别名通道。
        let alias: serde_json::Value = serde_json::from_str(
            &(tool.handler)(serde_json::json!({ "query": "limits" }))
                .await
                .expect("别名命中应 Ok"),
        )
        .unwrap();
        assert_eq!(alias["hits"][0]["matched_by"], "alias");
        assert_eq!(alias["hits"][0]["canonical"], "极限");

        // 零命中：Ok + not_found=true（不抛错、不中断循环）。
        let miss: serde_json::Value = serde_json::from_str(
            &(tool.handler)(serde_json::json!({ "query": "量子纠缠" }))
                .await
                .expect("零命中应 Ok"),
        )
        .unwrap();
        assert_eq!(miss["not_found"], true);
        assert!(miss["hits"].as_array().unwrap().is_empty());

        // detail：全定义 + 全部活跃别名（不受 top-5 限制）。
        let detail: serde_json::Value = serde_json::from_str(
            &(tool.handler)(serde_json::json!({
                "query": "极限", "mode": "detail", "concept_id": REGISTRY_CONCEPT
            }))
            .await
            .expect("detail 命中应 Ok"),
        )
        .unwrap();
        assert_eq!(detail["hits"][0]["definition"], "无限逼近的值：函数值趋近于某个定值");
        assert_eq!(detail["hits"][0]["aliases"].as_array().unwrap().len(), 6);

        // 入参错误转模型可读 Err。
        let missing_query = (tool.handler)(serde_json::json!({}))
            .await
            .expect_err("缺 query 应 Err");
        assert!(missing_query.contains("query"), "{missing_query}");
        let blank_query = (tool.handler)(serde_json::json!({ "query": "   " }))
            .await
            .expect_err("空白 query 应 Err");
        assert!(blank_query.contains("query"), "{blank_query}");
        let missing_id = (tool.handler)(serde_json::json!({ "query": "极限", "mode": "detail" }))
            .await
            .expect_err("detail 缺 concept_id 应 Err");
        assert!(missing_id.contains("concept_id"), "{missing_id}");
        let unknown_id = (tool.handler)(serde_json::json!({
            "query": "极限", "mode": "detail", "concept_id": "0190f5fe-7c00-7a00-8abc-999999999999"
        }))
        .await
        .expect_err("detail 查无应 Err");
        assert!(unknown_id.contains("absent from the registry"), "{unknown_id}");
        let bad_mode = (tool.handler)(serde_json::json!({ "query": "极限", "mode": "frobnicate" }))
            .await
            .expect_err("非法 mode 应 Err");
        assert!(bad_mode.contains("mode 只支持"), "{bad_mode}");
    }

    /// 只读自证：search / 零命中 / detail 三种调用前后，登记表行数、内容
    /// 与 `updated_at` 逐字不变（零写侧）。
    #[tokio::test]
    async fn search_concepts_handler_leaves_the_registry_untouched() {
        let (service, pool) = registry_service().await;
        let before = registry_snapshot(&pool).await;
        let (ctx, ..) = context_with(service, graph_teaches_context());
        let tool = mounted_search_tool(&ctx, true);

        (tool.handler)(serde_json::json!({ "query": "极限" }))
            .await
            .expect("search 命中");
        (tool.handler)(serde_json::json!({ "query": "量子纠缠" }))
            .await
            .expect("search 零命中");
        (tool.handler)(serde_json::json!({
            "query": "极限", "mode": "detail", "concept_id": REGISTRY_CONCEPT
        }))
        .await
        .expect("detail");

        let after = registry_snapshot(&pool).await;
        assert_eq!(before, after, "search_concepts 只读：登记表行与内容零变化");
    }
}
