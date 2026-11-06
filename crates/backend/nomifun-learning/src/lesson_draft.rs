//! Lesson content draft + deterministic audit — the lesson sibling of
//! `course_outline/draft.rs`. The agent builds one lesson's content through
//! patch ops (`set_document` / activity ops), every patch re-runs the
//! deterministic audit, and the finish gate re-audits live before the draft
//! converts into the generation stage's [`LessonOutput`].
//!
//! The audit reuses the fallback pipeline's rules verbatim — the document
//! contract (`validate_lesson_document`: length floor + three required
//! sections in order) and the activity rules (`validate_lesson_activities`:
//! count floors, reflection cap, per-kind shape) — inlined with
//! position-aware messages so one audit pass surfaces every finding.

use std::collections::{HashMap, HashSet};

use crate::generation::{
    LESSON_MAX_REFLECTION_ACTIVITIES, LESSON_MIN_ACTIVITIES, LESSON_MIN_OBJECTIVE_ACTIVITIES,
    LessonOutput, validate_lesson_document,
};
use crate::models::{
    ActivityKind, ActivityPack, ComplexityTier, SectionKind, SectionPack, VISUAL_OPTIONS,
    visual_menu_text,
};

use crate::course_outline::{SEV_DANGER, SEV_WARNING};

/// Hard cap on the activities a draft may hold: the design guidance is 3-5,
/// so anything near this cap is already off-script (the audit warns before
/// it ever blocks).
const MAX_ACTIVITIES: usize = 10;

/// 图课时「时长预算」行与超容自报的运维开关（k03，learnhub #154）：
/// 置 off/0/false 时不注入预算行、计划工具的自报键按缺席处理（=旧行为）。
/// 与 `NOMI_LEARNING_SOURCE_PLAN` 同款进程内运维开关，不进 UI/用户配置面。
pub const EST_BUDGET_SWITCH_ENV: &str = "NOMI_LEARNING_EST_BUDGET";

/// 开关取值判定（off/0/false；大小写与首尾空白宽容）。
pub fn is_est_budget_disabled_value(value: &str) -> bool {
    crate::is_off_switch_value(value)
}

/// 时长预算机制是否开启（未设/其它值 = 开启）。
pub fn est_budget_enabled() -> bool {
    std::env::var(EST_BUDGET_SWITCH_ENV)
        .map(|value| !is_est_budget_disabled_value(&value))
        .unwrap_or(true)
}

/// 档位优先序运维开关（u07，`NOMI_LEARNING_TIER_PRIORITY`）：只管两项
/// 模型可见面——①生成系统提示词的「档位优先序」条款（ai-agent 面）；
/// ②本面审计的节长上限 warning（`section_over_budget`）。未设/其它取值 =
/// 开（**缺省 on 待 A/B**；票面推翻条件：on 臂知道档出现「只剩定义、无
/// 情境引入」类空心化 → 默认 off）；off/0/false = 关（两处逐字节回基线）。
/// 修复轮上下文注入与发布前数学失败回给模型两项为机械正确性补全，**不挂
/// 本开关**（票面明示）。
pub const TIER_PRIORITY_SWITCH_ENV: &str = "NOMI_LEARNING_TIER_PRIORITY";

/// 开关取值判定（off/0/false；大小写与首尾空白宽容）。
pub fn is_tier_priority_disabled_value(value: &str) -> bool {
    crate::is_off_switch_value(value)
}

/// 档位优先序开关判定（取值注入面，纯函数——测试不触进程 env）。
pub fn tier_priority_enabled_value(value: Option<&str>) -> bool {
    value.map(|value| !is_tier_priority_disabled_value(value)).unwrap_or(true)
}

/// 档位优先序开关的进程读数（[`tier_priority_enabled_value`] 同款语义）。
pub fn tier_priority_enabled() -> bool {
    tier_priority_enabled_value(std::env::var(TIER_PRIORITY_SWITCH_ENV).ok().as_deref())
}

/// u07 节长预算锚映射：teaches 档位的**最高档** → 文字预算档（与提示词
/// `prose_budget_rules` 同源 [`ComplexityTier::prose_budget`]，禁止手写
/// 数字）——知道 = 低档、会用 = 中档、能教 = 高档；混档取最高（深度上限
/// 跟着最深概念走）；无档位（传统线/零 teaches）或档位标签不可识别 = 中
/// 档（不猜测）。上限 = 锚 × 2，由消费面（审计 warning）乘。
pub fn tier_prose_budget_cap(teaches_tiers: &[(String, String)]) -> usize {
    let tiers = teaches_tiers.iter().map(|(_, tier)| tier.trim());
    if tiers.clone().any(|tier| tier == "能教") {
        ComplexityTier::High.prose_budget()
    } else if tiers.clone().any(|tier| tier == "会用") {
        ComplexityTier::Mid.prose_budget()
    } else if tiers.clone().any(|tier| tier == "知道") {
        ComplexityTier::Low.prose_budget()
    } else {
        ComplexityTier::Mid.prose_budget()
    }
}

/// 图课时用户提示的「时长预算」行（k03）：est 是结构阶段给节点的尺寸锚，
/// 规划节容量与写作两阶段都对照它。行格式逐字固定——真机冒烟对观测存档
/// 机械 grep 这一串。
pub fn render_est_budget_line(est_minutes: i64) -> String {
    format!("- 时长预算：{est_minutes} 分钟（节点尺寸锚——规划与写作两阶段对照此预算）\n")
}

/// 「课程已知概念」行（Known Names，learnhub #177 输入面，域中立移植）：
/// 工作词边界的指称面——构件词边界与取材池条款的名单行。行格式逐字固定
/// （真机冒烟对观测存档机械 grep 这一串）；空名单整行省略（`None`，受众行
/// 省行先例同形）。
pub fn render_known_names_line(names: &[String]) -> Option<String> {
    (!names.is_empty()).then(|| {
        format!(
            "课程已知概念（本课已教 ∪ 已会声明；工作词只能取自这里 ∪ 本节点 teaches，名单外的领域名词不引入）：{}\n",
            names.join("、")
        )
    })
}

/// teaches 档位行（D4，意图画像四角的深度角）：此前写作纪律「档位决定
/// 内容形态」引用的是模型读不到的值——本行让档位进输入面。空 teaches 整行
/// 省略；行格式逐字固定（冒烟机械 grep 面）。
pub fn render_teaches_tier_line(tiers: &[(String, String)]) -> Option<String> {
    (!tiers.is_empty()).then(|| {
        let rows = tiers
            .iter()
            .map(|(concept, tier)| format!("{concept}（{tier}）"))
            .collect::<Vec<_>>()
            .join("、");
        format!("节点档位（teaches 概念的深度锚——内容形态与练习形态按档位定）：{rows}\n")
    })
}

/// 先例（Precedent，learnhub #180 域中立移植）：本点涉及（teaches ∪
/// assumes）的非首现概念，先前已由祖先闭包内某学习点教出的对应关系——
/// 概念 + 教学节点显示名。首现概念不入（本点自己教）、声明-only 概念不入
/// （零节点）、跨课程不入（前置路径已承载跨课程视角）；正文全文回看（工具
/// 轮）是雾区，不在先例语义内。p02 双传收敛：节要点摘要由前置路径条目
/// 唯一承载，本结构只做概念归属映射。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LessonPrecedent {
    /// 该节点教出的本点涉及概念正名（多概念「、」连接；一条先例一个节点）。
    pub concept: String,
    /// 教学节点显示名。
    pub node_name: String,
}

/// 先例摘要块（#180 输入面）：已建立语境（记号/句式/方法）的生成侧视野。
/// p02 双传收敛：前置路径已带节要点，本块只做概念归属映射——概念归属 +
/// 用名纪律所需的最小事实，细节由前置路径承载。空先例整块省略（`None`）；
/// 块格式逐字固定（冒烟机械 grep 面）。
pub fn render_precedent_block(precedents: &[LessonPrecedent]) -> Option<String> {
    if precedents.is_empty() {
        return None;
    }
    let mut text = String::from(
        "已建立先例（本点涉及的非首现概念先前已由下列节点教出——沿用其已建立的语境与叫法，不重新讲授）：\n",
    );
    for precedent in precedents {
        text.push_str(&format!(
            "- 概念「{}」← 节点「{}」\n",
            precedent.concept, precedent.node_name
        ));
    }
    Some(text)
}

/// 前置路径块（p02 B 双头合并）：单一块头吸收原「前置契约」句——头部
/// 一处说清「概念已教到档位、不要重新讲授、条目要点是前置实际教过的内容」，
/// 档位标尺括注由 `crate::service::lesson::render_prerequisite_path` 保留
/// 一次。空/空白路径整块省略（`None`——起点节点走「没有前置」句）。
/// 块头逐字固定（冒烟机械 grep 面）。
pub fn render_prerequisite_path_block(path: &str) -> Option<String> {
    let path = path.trim();
    (!path.is_empty()).then(|| {
        format!(
            "前置路径（学习者到达本节点前应已掌握，按学习顺序；概念已由其他节点教到标注档位——不要重新讲授，只在必要时按该档位复述或引用。条目下的要点是各前置实际教过的内容——不要重复讲授）：\n{path}"
        )
    })
}

/// 图臂头两行（p02 C 空值行省略）：「学习目标」「程度声明」各自在值
/// 非空（trim 后）时渲染——结构快照无程度声明时不再渲出空行（真机 L4
/// 空值行噪声；known_names 行缺席省略同款先例）。
pub fn render_graph_goal_degree_lines(goal: &str, degree_statement: &str) -> String {
    let goal = goal.trim();
    let degree_statement = degree_statement.trim();
    let mut lines = String::new();
    if !goal.is_empty() {
        lines.push_str(&format!("学习目标：{goal}\n"));
    }
    if !degree_statement.is_empty() {
        lines.push_str(&format!("程度声明：{degree_statement}\n"));
    }
    lines
}

/// 图线无简报尾注（p02 C，真机 L33）：列项动态拼装——学习目标/程度声明
/// 各自在值非空（trim 后）时入列，空值项省略（不再指向空白程度声明）；
/// 「前置/后续节点段落」恒在场。列项间「、」相连、末项前用「与」。
pub fn render_graph_no_brief_note(goal: &str, degree_statement: &str) -> String {
    let goal = goal.trim();
    let degree_statement = degree_statement.trim();
    let mut items: Vec<&str> = Vec::new();
    if !goal.is_empty() {
        items.push("学习目标");
    }
    if !degree_statement.is_empty() {
        items.push("程度声明");
    }
    items.push("前置/后续节点段落");
    // 末项恒在场（恒 push）→ 列项非空是本函数不变量；原 slice match 的
    // `[] =>` 空分支不可达，删臂改以 pop 末项拼装（单项直取、多项「、」
    // 相连末项前「与」，输出逐字不变）。
    let last = items.pop().expect("「前置/后续节点段落」恒在场");
    let sentence = if items.is_empty() {
        last.to_owned()
    } else {
        format!("{}与{last}", items.join("、"))
    };
    format!("\n学习图节点没有课程简报：内容忠于{sentence}。\n")
}

/// 声明面骨架行（absorb-8 z03，learnhub #205 契约 v2.17）：节点 intent/
/// scope 声明的两行投影——「创建意图」行 + 「教学范围」行。任一字段缺席
/// 或空白（旧档 DB 默认空串）即省该行；两字段皆不在场 = 整段省略（`None`，
/// 旧行为零变化）。行格式逐字固定——真机冒烟对观测存档机械 grep 这一串；
/// 范围行尾注即 content-write「点名边界不构成引用」配对条款的输入面短句。
pub fn render_intent_scope_lines(
    intent: Option<&str>,
    scope: Option<&str>,
) -> Option<String> {
    let intent = intent.map(str::trim).filter(|intent| !intent.is_empty());
    let scope = scope.map(str::trim).filter(|scope| !scope.is_empty());
    if intent.is_none() && scope.is_none() {
        return None;
    }
    let mut lines = String::new();
    if let Some(intent) = intent {
        lines.push_str(&format!("创建意图：{intent}\n"));
    }
    if let Some(scope) = scope {
        lines.push_str(&format!(
            "教学范围：{scope}（范围外内容不教，越界点名不构成引用）\n"
        ));
    }
    Some(lines)
}

/// 超容自报归一（k03）：出现时按非空字符串收下（trim 后原样保留语义），
/// 空白串按缺席裁成 `None`——形状条款只查非空，**永不退稿**（learnhub
/// `extract_capacity_note` 同款；自报是自愿留痕，不是数量门）。
pub fn normalize_capacity_note(note: Option<&str>) -> Option<String> {
    note.map(str::trim)
        .filter(|note| !note.is_empty())
        .map(str::to_owned)
}

/// Grounding 片段的角色（t3 锚定纪律）：`Primary` = 大纲引用的主 source
/// （全文；检索命中**永不替换它**），`Supplement` = 检索命中扩读的相关文件
/// 片段。旧构造点（快照兜底/描述流）只有 primary。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExcerptRole {
    #[default]
    Primary,
    Supplement,
}

/// Grounding excerpt for the kb flow: the sampled file the lesson cites.
/// t3 扩为多片段：`path`/`text` 是兼容位（旧消费方只读这两个字段），
/// `kb_id`/`kb_name` 是库归属，`heading` 是检索命中的章节标题，`role`
/// 区分主引用与补充片段——补充片段靠这些字段可溯源（库+文件+章节）。
#[derive(Debug, Clone)]
pub struct LessonExcerpt {
    /// 库内相对路径（primary = 大纲引用的 source 路径）。
    pub path: String,
    /// 片段文本（primary = 主 source 全文，按片段预算截断）。
    pub text: String,
    /// 来源库 id（实时读库时才有；快照兜底与描述流为 `None`）。
    pub kb_id: Option<String>,
    /// 来源库名（提示词里可读的归属；与 `kb_id` 同来同去）。
    pub kb_name: Option<String>,
    /// 检索命中的章节标题（`search_bases` 的 `heading`；primary 恒为 `None`）。
    pub heading: Option<String>,
    /// 主引用 / 检索补充。
    pub role: ExcerptRole,
}

impl LessonExcerpt {
    /// 旧形状构造（path + text、无库归属）：快照兜底与旧消费方用。
    pub fn text_only(path: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            text: text.into(),
            kb_id: None,
            kb_name: None,
            heading: None,
            role: ExcerptRole::Primary,
        }
    }

    /// 提示词里的角色标签。
    pub fn role_label(&self) -> &'static str {
        match self.role {
            ExcerptRole::Primary => "主引用",
            ExcerptRole::Supplement => "补充片段",
        }
    }

    /// 片段出处标签（提示词与披露共用）：文件 + 库归属 + 命中章节。主引用
    /// 在快照兜底时没有库归属，只有文件——它是大纲引用的源，身份已明确。
    pub fn provenance_label(&self) -> String {
        let mut label = format!("文件 {}", self.path);
        let kb = self
            .kb_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .or_else(|| self.kb_id.as_deref());
        if let Some(kb) = kb {
            label.push_str(&format!("（知识库 {kb}）"));
        }
        if let Some(heading) = self
            .heading
            .as_deref()
            .map(str::trim)
            .filter(|heading| !heading.is_empty())
        {
            label.push_str(&format!(" 章节「{heading}」"));
        }
        label
    }
}

/// 学习图课程节点专属的生成上下文（beta）。传统课时恒为 `None`；为 Some
/// 时，引擎提示词以图语义段落（学习目标/学习范围/前置/后续）取代课程大纲
/// 段。概念网口径（ADR-0009）：前置段是本课 assumes 的概念与跨课程教这些
/// 概念的节点摘要；后续段是同课程位于本课之后的节点（衔接参照）。
#[derive(Debug, Clone)]
pub struct GraphLessonContext {
    /// 用户生成图时输入的学习目标。
    pub goal: String,
    /// 结构快照回执的程度声明（达成后能独立做成什么；learning_scope 列随
    /// 结构改造退役）。
    pub degree_statement: String,
    /// 预渲染：本课 assumes 的概念 + 跨课程教这些概念的节点摘要（离题最近
    /// 的教师优先，超长截断），让生成知道「学习者此刻已经会什么」。节点内
    /// 容是共享课程资产，不含任何用户个人进度。
    pub prerequisite_path: String,
    /// 预渲染：同课程位于本课之后的节点（衔接参照，非依赖关系），让生成
    /// 知道「要为哪些后续学习做衔接」。
    pub upcoming_nodes: String,
    /// 概念面（j07 读侧投影）：首现/非首现 + 引用名 + 活跃别名池的结构化
    /// 数据。`None` = 本节点无概念挂接（teaches/assumes 双空），概念面段
    /// 整体缺席。
    pub concept_faces: Option<GraphConceptFaces>,
    /// 节点尺寸锚（k03，learnhub #154）：结构阶段给本节点估的课时时长
    /// （分钟）。提示词「时长预算」行的唯一数据源——规划节容量与写作时
    /// 对照它；`None` = 无 est（allo 图节点列 NOT NULL DEFAULT，恒 Some）。
    pub est_minutes: Option<i64>,
    /// 课程已知概念（Known Names，learnhub #177 输入面，n3 D1 口径）：本
    /// 课程图全部节点 teaches ∪ 本课程已会声明（m2 锚表）的显示名并集，
    /// 升序稳定——内容工作词边界的指称面（构件词边界/取材池条款消费它）。
    /// `None` = 名单为空（整行省略，受众行省行先例同形）。
    pub known_names: Option<Vec<String>>,
    /// 本节点 teaches 概念的档位（D4，意图画像四角的深度角）：(概念正名,
    /// 中文档位) 有序对——此前写作纪律「档位决定内容形态」引用模型读不到
    /// 的值，本字段补上输入面。空 = 本节点零 teaches 挂接（行省略）。
    pub teaches_tiers: Vec<(String, String)>,
    /// 先例摘要（Precedent，learnhub #180）：按节点 id 升序稳定；空 =
    /// 无先例（块省略）。
    pub precedents: Vec<LessonPrecedent>,
    /// 节点创建意图（absorb-8 z03，learnhub #205 声明面）：节点诞生即声明
    /// 的「为什么有这个点」一句话——骨架行「创建意图」行的唯一数据源；
    /// `None` = 旧档未声明（DB 空串折叠，行省略）。
    pub intent: Option<String>,
    /// 节点教学范围（absorb-8 z03）：「教什么/教到哪/明确不碰什么」的边界
    /// 声明——声明优先条款的边界第一真相 + `scope_findings` 词面核对的
    /// 提取源；`None`/空 = 旧档未声明（行省略 + 零扫描）。
    pub scope: Option<String>,
}

/// 概念面读侧投影（二轮 g2 / j07）：首现判定（祖先闭包不动点，零新存储）
/// + 引用名开讲（j04 ref_name 落库）+ 投影词表（活跃别名池）。结构化承载
/// ——提示词渲染与审计观察条（用名一致性/首现引入）共用同一事实源。
#[derive(Debug, Clone, Default)]
pub struct GraphConceptFaces {
    /// 首现概念（正名）：本节点 teaches 中不在祖先闭包已教集合里的——
    /// 先情境引入再给正式定义。
    pub first_seen: Vec<String>,
    /// 非首现概念（正名）：祖先闭包已教过——一两句回收即可，不重新讲授。
    pub returning: Vec<String>,
    /// 引用名（节点挂接时模型实际用名）→ 正名；仅列与正名不同的条目。
    /// 正文以引用名开讲，正名只在首次出现时括注。
    pub ref_names: Vec<(String, String)>,
    /// 正名 → 活跃别名（投影词表，R3）：旧叫法正文不使用，必要时括注帮
    /// 学习者辨认。
    pub aliases: Vec<(String, Vec<String>)>,
}

impl GraphConceptFaces {
    /// 是否携带任何事实（全空 = 概念面段缺席）。
    pub fn is_empty(&self) -> bool {
        self.first_seen.is_empty()
            && self.returning.is_empty()
            && self.ref_names.is_empty()
            && self.aliases.is_empty()
    }

    /// 渲染进节点内容生成上下文的「概念面」段（含 R2/R5 条款——概念面
    /// 条款只进图节点路径，本渲染即其唯一落点）。
    pub fn render_text(&self) -> String {
        let mut lines = vec![
            "概念面（本节点要教的概念如何登场——只约束本节点 teaches 的概念）：".to_owned(),
            "- 登场纪律：首现概念先用情境引出再给正式定义；非首现概念一两句话回收即可，不重新讲授。".to_owned(),
            "- 用名纪律：正文以本节点标注的名字开讲（前置路径标了引用名的用引用名），正名只在首次出现时括注一次；同一概念从头到尾用同一个名字，不混用别名。".to_owned(),
        ];
        if !self.first_seen.is_empty() {
            lines.push(format!("- 首现（先引入再定义）：{}", self.first_seen.join("、")));
        }
        if !self.returning.is_empty() {
            lines.push(format!(
                "- 非首现（前置已教过，简短回收）：{}",
                self.returning.join("、")
            ));
        }
        if !self.ref_names.is_empty() {
            let rows = self
                .ref_names
                .iter()
                .map(|(reference, canonical)| format!("{reference}（正名：{canonical}）"))
                .collect::<Vec<_>>()
                .join("；");
            lines.push(format!("- 引用名（正文用它开讲）：{rows}"));
        }
        if !self.aliases.is_empty() {
            // p02 D 方向化措辞：「本课现用名（旧称：…）」——首现概念与
            // 旧叫法并读不再可能误读（旧「X 又叫 Y + 正文不使用」的方向
            // 读法易误伤首现概念）；数据面零改动（aliases 仍为活跃别名池
            // 投影）。
            let rows = self
                .aliases
                .iter()
                .map(|(canonical, aliases)| format!("{canonical}（旧称：{}）", aliases.join("/")))
                .collect::<Vec<_>>()
                .join("；");
            lines.push(format!(
                "- 本课现用名（别处可能见过的旧叫法正文不使用，必要时括注一次帮助辨认）：{rows}"
            ));
        }
        lines.join("\n")
    }
}

/// The lesson context an engine generates content for: course/module/lesson
/// coordinates and the grounding (the cited excerpt for the kb flow, the
/// course brief for the description flow). Built by `generate_lesson_content`
/// from the persisted course snapshot.
#[derive(Debug, Clone)]
pub struct LessonGenerationContext {
    /// 课时行 id——草稿按它挂到课时（断点续跑的查找键：重试时定位到
    /// 仍存活的草稿接着跑，而不是从零重建）。
    pub lesson_id: String,
    pub course_title: String,
    /// Description-flow grounding (kb-flow lessons ground in `excerpt`).
    pub course_description: String,
    pub module_title: String,
    pub module_index: usize,
    pub lesson_title: String,
    pub lesson_index: usize,
    pub total_lessons: usize,
    pub next_lesson_title: Option<String>,
    pub purpose: String,
    /// Kb-flow grounding；`None` on the description flow. 兼容位：主引用
    /// （或快照兜底摘录）——新消费方请用 [`LessonGenerationContext::grounding`]。
    pub excerpt: Option<LessonExcerpt>,
    /// 多片段 grounding（t3）：primary 在前（主 source 全文），其后是按相关度
    /// 排序的检索补充片段。空 = 回退兼容位 `excerpt`（旧构造点只填它）。
    pub excerpts: Vec<LessonExcerpt>,
    /// Pre-rendered full course outline with the current lesson marked — the
    /// anti-duplication / no-scope-creep reference shared by both pipelines.
    pub outline_tree: String,
    /// Pre-rendered prev/next lesson reference (title/purpose/truncated
    /// excerpt); empty when there is nothing to reference.
    pub adjacent_context: String,
    /// 学习图节点专属上下文；传统课时恒为 `None`。
    pub graph: Option<GraphLessonContext>,
    /// 用户目标（t1，传统课程）：课程行 learning_goal 的全文——目标即课程
    /// 的意图，写作时始终对照；旧课程（无目标）恒为 `None`。学习图节点的
    /// 目标在 `graph.goal` 里，本字段不重复承载。
    pub goal: Option<String>,
    /// 防超纲黑名单：传统课时已随概念契约下线（恒空）；学习图节点预渲染
    /// 下游节点标题清单。空 = 无。由上下文构造方决定原料,提示词统一消费。
    pub forbidden_concepts: String,
    /// 相位耗时读数（e1）：长任务注册表派发的 LLM 用量累计句柄——引擎侧的
    /// provider 计数装饰器每次 LLM 调用后上报（调用数 + 输入/输出字符量），
    /// 注册表在 finish 行聚合落 `learning-tasks.jsonl`。`None` = 无读数
    /// （测试/旧构造点缺省；观测面缺席零语义）。
    pub usage: Option<crate::completer::LlmUsageMeter>,
    /// 门面重试环的反馈槽（e4，learnhub contentgen facade 的 retry_feedback
    /// 同职能）：上次尝试因瞬断中断时，重试环在这里写下「第几次尝试、为何
    /// 中断、已写好的节不要重写」；续跑开场（[`crate::lesson_draft`] 渲染面）
    /// 把它注入用户轮。首试恒 `None`。
    pub retry_feedback: Option<String>,
}

impl LessonGenerationContext {
    /// Grounding 片段全集（t3）：`excerpts` 优先，空时回退兼容位 `excerpt`
    /// ——旧构造点（只填 `excerpt`）与新构造点（两个都填，值一致）都得到
    /// 正确结果，消费方只有一个读取入口。
    pub fn grounding(&self) -> Vec<&LessonExcerpt> {
        if self.excerpts.is_empty() {
            self.excerpt.iter().collect()
        } else {
            self.excerpts.iter().collect()
        }
    }

    /// 清空全部 grounding（兼容位与多片段一起清）。agent 循环的历史清理点
    /// （检不到源、图节点等）统一走这里，避免两处状态漂移。
    pub fn clear_grounding(&mut self) {
        self.excerpt = None;
        self.excerpts.clear();
    }
}

/// One deterministic audit finding. `severity` uses the shared vocabulary
/// (`SEV_DANGER` blocks the finish gate, `SEV_WARNING` is advisory).
#[derive(Debug, Clone, serde::Serialize)]
pub struct LessonFinding {
    pub severity: &'static str,
    pub kind: String,
    pub message: String,
}

/// One lesson's in-memory draft. Short-lived (same lifecycle as the outline
/// drafts): `finish_lesson_draft` is the single publish path.
#[derive(Debug, Clone)]
pub struct LessonDraft {
    pub context: LessonGenerationContext,
    /// The study document (plain Markdown, no JSON wrapper) — the legacy
    /// single-document shape. `None` when the draft is sectioned.
    pub document: Option<String>,
    pub estimated_minutes: i64,
    pub activities: Vec<ActivityPack>,
    /// 分节契约（ADR-0002）：大纲清单 + 已写节正文。`set_section_manifest`
    /// 规划节型，`set_section_body` 逐节写入——agent loop 的「分职责轮次」，
    /// 每节一次工具调用，注意力集中在单一学习单元上。
    pub section_manifest: Option<Vec<SectionPack>>,
    /// section_key → (正文, 计划)。正文经节级质检后才入表。
    pub section_bodies: HashMap<String, (String, Option<SectionPack>)>,
    /// 超容自报（k03）：规划阶段模型对照时长预算写下的如实留痕；`None`
    /// = 未自报（缺席合法）。只进报告面与观察条，不参与任何 gate。
    pub capacity_note: Option<String>,
    /// Bumps once per accepted op — the repair loop's "did the model touch
    /// anything" signal.
    pub revision: usize,
    pub findings: Vec<LessonFinding>,
    /// f3 viz lint 事件计数：op 接纳面打回（lint 违约被拒）与终检剔除累计
    /// ——经 to_output 穿出，publish 帧披露（smoke viz_lint_events 读数）。
    viz_lint_events: usize,
    /// f3 终检系统剔块降声明的节 key（to_output 穿出 → 落库 degraded 列，
    /// service/lesson.rs persist 先例）。清单序。
    degraded_keys: Vec<String>,
}

/// Patch ops for a lesson draft. Tagged `{"op": "..."}` over the wire like
/// the outline ops. Activity positions are 0-based indices into the list at
/// the moment the op applies; ops execute in array order.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum LessonOp {
    SetDocument {
        document: String,
    },
    /// 规划分节清单（整组替换；重规划时已写正文按 section_key 保留）。
    SetSectionManifest {
        sections: Vec<SectionPack>,
        /// 节点意图超载信号（k03/#178 换血）：先砍雄心再砍内容仍收敛不住
        /// 「时长预算」时必写、指名超载原因（上游 est 复议的信号）；预算内
        /// 收敛整键省略（缺席合法，形状面照旧只查非空）。
        #[serde(default)]
        capacity_note: Option<String>,
    },
    /// 写入一节正文；该节已存在时整体替换。
    SetSectionBody {
        section_key: String,
        body: String,
    },
    SetEstimatedMinutes {
        minutes: i64,
    },
    AddActivity {
        activity: ActivityPack,
    },
    UpdateActivity {
        position: usize,
        activity: ActivityPack,
    },
    RemoveActivity {
        position: usize,
    },
}

impl LessonOp {
    fn name(&self) -> &'static str {
        match self {
            Self::SetDocument { .. } => "set_document",
            Self::SetSectionManifest { .. } => "set_section_manifest",
            Self::SetSectionBody { .. } => "set_section_body",
            Self::SetEstimatedMinutes { .. } => "set_estimated_minutes",
            Self::AddActivity { .. } => "add_activity",
            Self::UpdateActivity { .. } => "update_activity",
            Self::RemoveActivity { .. } => "remove_activity",
        }
    }
}

/// Draft snapshot returned by `ls_start` — everything the model needs to
/// start working without re-asking for context.
#[derive(Debug, serde::Serialize)]
pub struct LessonDraftView {
    pub draft_id: String,
    pub course_title: String,
    pub module_title: String,
    pub lesson_title: String,
    pub purpose: String,
    /// `excerpt:<path>` for the kb flow, `course_brief` for the description
    /// flow (the grounding text itself rode in on the engine's user turn).
    pub grounding: String,
    pub revision: usize,
}

/// One activity in the inspect view: position + kind + prompt head, so the
/// model can address `update_activity`/`remove_activity` precisely.
#[derive(Debug, serde::Serialize)]
pub struct LessonActivitySummary {
    pub position: usize,
    pub kind: String,
    pub prompt: String,
}

/// 一节已写正文的摘要（e1 段级续跑）：key + 非空字符数。inspect 视图与续
/// 跑开场共用的「哪些节已落库」读数——字符数与节级质检长度口径一致。
#[derive(Debug, serde::Serialize)]
pub struct LessonSectionBodySummary {
    pub section_key: String,
    pub chars: usize,
}

/// Overview for `ls_inspect`: document shape, activity list and the fresh
/// audit findings.
#[derive(Debug, serde::Serialize)]
pub struct LessonInspectView {
    pub revision: usize,
    pub document_written: bool,
    /// Non-whitespace characters — the same counting the length floor uses.
    pub document_chars: usize,
    /// The document's `## ` heading lines in order.
    pub document_sections: Vec<String>,
    pub estimated_minutes: i64,
    /// 超容自报（k03）：规划时对照时长预算的自报留痕；缺席 = 未自报。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity_note: Option<String>,
    /// 分节契约的逐节正文状态（e1 段级续跑读数）：已落库正文各节的 key 与
    /// 非空字符数，按清单序；单篇旧草稿为空。续跑开场据此告诉模型「已写的
    /// 节就在草稿里」——段级幂等跳过的判定面也读这里。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub section_bodies: Vec<LessonSectionBodySummary>,
    pub activities: Vec<LessonActivitySummary>,
    pub findings: Vec<LessonFinding>,
}

/// One rejected op with its reason (accepted ops summarize what changed).
#[derive(Debug, serde::Serialize)]
pub struct LessonRejectedOp {
    pub op: String,
    pub reason: String,
}

/// Per-op verdicts plus a fresh audit snapshot — the `ls_patch_activities`
/// / `ls_set_document` return shape.
#[derive(Debug, serde::Serialize)]
pub struct LessonPatchReport {
    pub revision: usize,
    pub accepted: Vec<String>,
    pub rejected: Vec<LessonRejectedOp>,
    pub findings: Vec<LessonFinding>,
}

impl LessonDraft {
    pub fn new(context: LessonGenerationContext) -> Self {
        // est 铸值（k03/#178）：图节点带 est 时草稿 estimated_minutes 直接
        // 以节点尺寸锚开盘（learnhub frontmatter est_minutes 铸值同源）——
        // 模型仍可用 set_estimated_minutes 复议；传统课时维持旧默认 10。
        // 开关 off 臂回退旧默认（时长预算面整体回退）。
        let estimated_minutes = if est_budget_enabled() {
            context
                .graph
                .as_ref()
                .and_then(|graph| graph.est_minutes)
                // 草稿键值域 1-240（set_estimated_minutes 同款）；越界 est 不铸。
                .filter(|est| (1..=240).contains(est))
                .unwrap_or(10)
        } else {
            10
        };
        let mut draft = Self {
            context,
            document: None,
            estimated_minutes,
            activities: Vec::new(),
            section_manifest: None,
            section_bodies: HashMap::new(),
            capacity_note: None,
            revision: 0,
            findings: Vec::new(),
            viz_lint_events: 0,
            degraded_keys: Vec::new(),
        };
        draft.refresh_audit();
        draft
    }

    pub fn view(&self, draft_id: &str) -> LessonDraftView {
        LessonDraftView {
            draft_id: draft_id.to_owned(),
            course_title: self.context.course_title.clone(),
            module_title: self.context.module_title.clone(),
            lesson_title: self.context.lesson_title.clone(),
            purpose: self.context.purpose.clone(),
            grounding: match self.context.grounding().first() {
                // 披露多片段结构（t3）：主引用路径 + 补充片段计数。
                Some(excerpt) => {
                    let supplements = self.context.grounding().len().saturating_sub(1);
                    if supplements > 0 {
                        format!("excerpt:{} +{supplements} 补充", excerpt.path)
                    } else {
                        format!("excerpt:{}", excerpt.path)
                    }
                }
                None => "course_brief".into(),
            },
            revision: self.revision,
        }
    }

    /// Apply a batch of ops in order; each accepted op bumps the revision.
    /// Positions refer to the activity list at the moment the op applies.
    pub fn apply_ops(&mut self, ops: Vec<LessonOp>) -> LessonPatchReport {
        let mut accepted = Vec::new();
        let mut rejected = Vec::new();
        for op in ops {
            let op_name = op.name().to_owned();
            match self.apply_one(op) {
                Ok(summary) => {
                    self.revision += 1;
                    accepted.push(summary);
                }
                Err(reason) => rejected.push(LessonRejectedOp {
                    op: op_name,
                    reason,
                }),
            }
        }
        self.refresh_audit();
        LessonPatchReport {
            revision: self.revision,
            accepted,
            rejected,
            findings: self.findings.clone(),
        }
    }

    fn apply_one(&mut self, op: LessonOp) -> Result<String, String> {
        match op {
            LessonOp::SetDocument { document } => {
                let trimmed = document.trim().to_owned();
                if trimmed.is_empty() {
                    return Err("document must not be empty".into());
                }
                let chars = trimmed.chars().filter(|c| !c.is_whitespace()).count();
                self.document = Some(trimmed);
                Ok(format!("document replaced ({chars} non-whitespace characters)"))
            }
            LessonOp::SetSectionManifest {
                sections,
                capacity_note,
            } => {
                if sections.is_empty() {
                    return Err("section manifest must not be empty".into());
                }
                // f3：degraded 是终检系统 mutation 的节级标记，模型 op 不得
                // 走私（demo 硬门豁免只属于终检兜底面）——op 入口统一清零。
                let mut sections = sections;
                sections
                    .iter_mut()
                    .for_each(|section| section.degraded = false);
                let mut seen = HashSet::new();
                for (index, section) in sections.iter().enumerate() {
                    if section.title.trim().is_empty() {
                        return Err(format!(
                            "section {} has an empty title",
                            section.section_key
                        ));
                    }
                    if !seen.insert(section.section_key.as_str()) {
                        return Err(format!(
                            "duplicate section key {}",
                            section.section_key
                        ));
                    }
                    if section.kind == SectionKind::Practice && index != sections.len() - 1 {
                        return Err(
                            "the practice section must be the last section (exactly one)".into()
                        );
                    }
                    if matches!(
                        section.kind,
                        SectionKind::Concept | SectionKind::Example | SectionKind::Demo
                    ) {
                        if !VISUAL_OPTIONS.contains(&section.visual.trim()) {
                            return Err(format!(
                                "section {} ({}) must declare its planned visual, one of: {}",
                                section.section_key,
                                section.kind.label(),
                                VISUAL_OPTIONS.join(" / ")
                            ));
                        }
                    }
                    // Keep already-written bodies whose key survives the replan.
                    if let Some((body, _)) = self.section_bodies.get(&section.section_key) {
                        let body = body.clone();
                        self.section_bodies.insert(
                            section.section_key.clone(),
                            (body, Some(section.clone())),
                        );
                    }
                }
                if !sections.last().is_some_and(|section| section.kind == SectionKind::Practice) {
                    return Err(
                        "the manifest must close with exactly one practice section as its last section"
                            .into(),
                    );
                }
                self.section_manifest = Some(sections);
                // 超容自报（k03）：只收下非空自报；开关 off 时按缺席处理
                // （旧行为）——形状条款零内容拦截，恶劣值裁 None 不退稿。
                self.capacity_note = if est_budget_enabled() {
                    normalize_capacity_note(capacity_note.as_deref())
                } else {
                    None
                };
                Ok("section manifest planned (write each body with set_section_body)".into())
            }
            LessonOp::SetSectionBody { section_key, body } => {
                let trimmed = body.trim().to_owned();
                if trimmed.is_empty() {
                    return Err("section body must not be empty".into());
                }
                // The manifest comes first: the kind's quality gate (length
                // floor, demo visualization, practice brevity) needs it.
                let planned = self
                    .section_manifest
                    .as_ref()
                    .and_then(|manifest| {
                        manifest
                            .iter()
                            .find(|section| section.section_key == section_key)
                    })
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "section {section_key} is not in the manifest — call set_section_manifest first"
                        )
                    })?;
                let candidate = SectionPack {
                    section_key: section_key.clone(),
                    kind: planned.kind,
                    title: planned.title.clone(),
                    points: planned.points.clone(),
                    visual: planned.visual.clone(),
                    body_md: crate::generation::fix_mermaid_quotes(&trimmed),
                    degraded: false,
                };
                // f3 lint 打回通道（4.2）：lint 并入 validate_body，坏块 op 在
                // 接纳面被拒（拒绝理由含定位与处置指引）→ 模型重提或改申报；
                // 事件计数穿出 publish 帧（viz_lint_events 读数）。
                self.viz_lint_events +=
                    crate::models::lint_visual_blocks(&candidate.body_md).len();
                candidate.validate_body()?;
                let existed = self.section_bodies.contains_key(&section_key);
                let report_key = section_key.clone();
                let fixed = candidate.body_md.clone();
                self.section_bodies
                    .insert(section_key, (fixed, Some(planned)));
                Ok(format!(
                    "section {report_key} body {} ({chars} non-whitespace characters)",
                    if existed { "replaced" } else { "written" },
                    chars = candidate.body_md.chars().filter(|c| !c.is_whitespace()).count()
                ))
            }
            LessonOp::SetEstimatedMinutes { minutes } => {
                if !(1..=240).contains(&minutes) {
                    return Err(format!(
                        "estimated_minutes must be 1-240, got {minutes}"
                    ));
                }
                self.estimated_minutes = minutes;
                Ok(format!("estimated_minutes set to {minutes}"))
            }
            LessonOp::AddActivity { activity } => {
                if self.activities.len() >= MAX_ACTIVITIES {
                    return Err(format!(
                        "lesson already holds {} activities (cap {MAX_ACTIVITIES}); remove one first",
                        self.activities.len()
                    ));
                }
                let position = self.activities.len();
                self.activities.push(activity);
                Ok(format!("activity added at position {position}"))
            }
            LessonOp::UpdateActivity { position, activity } => {
                if position >= self.activities.len() {
                    return Err(format!(
                        "position {position} out of range (lesson holds {} activities, positions 0..{})",
                        self.activities.len(),
                        self.activities.len()
                    ));
                }
                self.activities[position] = activity;
                Ok(format!("activity at position {position} replaced"))
            }
            LessonOp::RemoveActivity { position } => {
                if position >= self.activities.len() {
                    return Err(format!(
                        "position {position} out of range (lesson holds {} activities, positions 0..{})",
                        self.activities.len(),
                        self.activities.len()
                    ));
                }
                self.activities.remove(position);
                Ok(format!("activity at position {position} removed"))
            }
        }
    }

    pub fn refresh_audit(&mut self) {
        let document = self.document.clone();
        let estimated_minutes = self.estimated_minutes;
        let activities = self.activities.clone();
        let section_manifest = self.section_manifest.clone();
        let section_bodies = self.section_bodies.clone();
        let capacity_note = self.capacity_note.clone();
        self.findings = audit_findings(
            &self.context,
            document.as_deref(),
            estimated_minutes,
            &activities,
            section_manifest.as_deref(),
            &section_bodies,
            capacity_note.as_deref(),
        );
    }

    /// The full findings text — the repair loop's primary input.
    pub fn audit_report(&self) -> String {
        if self.findings.is_empty() {
            return "课时草稿审计：无问题 — 可以调用 ls_finish 发布。".into();
        }
        let mut report = format!("课时草稿审计 — {} 条 finding：\n", self.findings.len());
        for finding in &self.findings {
            report.push_str(&format!(
                "- [{}] {}: {}\n",
                finding.severity, finding.kind, finding.message
            ));
        }
        report.push_str(
            "\n存在 [danger] 时发布会被门禁拒绝：逐条修复（ls_set_section_manifest / \
             ls_set_section_body / ls_patch_activities，修复轮另有 ls_set_document 单篇契约）\
             后重新 ls_audit。",
        );
        report
    }

    pub fn inspect(&self) -> LessonInspectView {
        let document = self.document.as_deref().unwrap_or_default();
        let document_sections: Vec<String> = if self.section_manifest.is_some() {
            // Sectioned drafts report the manifest with per-section status.
            self.section_manifest
                .as_ref()
                .map(|manifest| {
                    manifest
                        .iter()
                        .map(|section| {
                            let state =
                                if self.section_bodies.contains_key(&section.section_key) {
                                    "ready"
                                } else {
                                    "pending"
                                };
                            format!(
                                "{} [{}] {}（{state}）",
                                section.section_key,
                                section.kind.label(),
                                section.title.trim()
                            )
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            document
                .lines()
                .map(str::trim)
                .filter(|line| line.starts_with("## "))
                .map(str::to_owned)
                .collect()
        };
        LessonInspectView {
            revision: self.revision,
            document_written: self.document.is_some(),
            document_chars: document.chars().filter(|c| !c.is_whitespace()).count(),
            document_sections,
            estimated_minutes: self.estimated_minutes,
            capacity_note: self.capacity_note.clone(),
            section_bodies: self
                .section_manifest
                .as_ref()
                .map(|manifest| {
                    manifest
                        .iter()
                        .filter_map(|section| {
                            self.section_bodies
                                .get(&section.section_key)
                                .map(|(body, _)| LessonSectionBodySummary {
                                    section_key: section.section_key.clone(),
                                    chars: body.chars().filter(|c| !c.is_whitespace()).count(),
                                })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            activities: self
                .activities
                .iter()
                .enumerate()
                .map(|(position, activity)| LessonActivitySummary {
                    position,
                    kind: activity.kind.as_str().to_owned(),
                    prompt: activity.prompt.trim().chars().take(60).collect(),
                })
                .collect(),
            findings: self.findings.clone(),
        }
    }

    /// Ready sections as packs, ordered by the manifest. Legacy
    /// single-document drafts yield an empty list — the dual-read fallback
    /// renders the flat summary.
    pub fn sections(&self) -> Vec<crate::models::SectionPack> {
        let mut sections: Vec<crate::models::SectionPack> = self
            .section_bodies
            .iter()
            .filter_map(|(key, (body, planned))| {
                planned.as_ref().map(|planned| crate::models::SectionPack {
                    section_key: key.clone(),
                    kind: planned.kind,
                    title: planned.title.clone(),
                    points: planned.points.clone(),
                    visual: planned.visual.clone(),
                    body_md: body.clone(),
                    degraded: planned.degraded,
                })
            })
            .collect();
        sections.sort_by_key(|section| {
            self.section_manifest
                .as_ref()
                .and_then(|manifest| {
                    manifest
                        .iter()
                        .position(|item| item.section_key == section.section_key)
                })
                .unwrap_or(usize::MAX)
        });
        sections
    }

    /// Convert a gate-cleared draft into the generation stage's output.
    /// Only called after the finish gate. Sectioned drafts assemble the flat
    /// summary from their sections; legacy single-document drafts pass the
    /// document through (the dual-read fallback renders it).
    pub fn to_output(&self) -> LessonOutput {
        let sections = self.sections();
        let summary = if sections.is_empty() {
            self.document.clone().unwrap_or_default()
        } else {
            sections
                .iter()
                .map(|section| section.body_md.trim())
                .collect::<Vec<_>>()
                .join("\n\n")
        };
        LessonOutput {
            summary,
            estimated_minutes: self.estimated_minutes,
            activities: self.activities.clone(),
            sections,
            // f3：degraded_keys 从图线草稿穿出（不再恒空）——终检系统剔块的
            // 节落库 degraded 列；viz lint 事件计数随发布帧披露。
            degraded_keys: self.degraded_keys.clone(),
            capacity_note: self.capacity_note.clone(),
            viz_lint_events: self.viz_lint_events,
            // scope 边界词面 findings 计数（absorb-8 z03，报告-only）随发布
            // 帧披露（viz_lint_events 读数同缝——冒烟 scope_findings 读数行
            // 的数据源）。
            scope_findings: self
                .findings
                .iter()
                .filter(|finding| finding.kind == "scope_boundary_hit")
                .count(),
        }
    }

    /// f3 终检兜底（4.4/A4）：对每节已落库正文跑 lint，仍坏的围栏块系统
    /// 剔除（节其余保留）；该节 visual 声明按余块形态同步降级（无余块 →
    /// 无），节记 degraded（to_output 穿出落库 degraded 列）。系统
    /// mutation，非模型 op。返回剔除 findings（warning viz_block_lint_
    /// dropped）——调用方（finish 门）在 refresh_audit 之后追加，避免被
    /// 审计整表重算覆盖；tracing::warn 同步留痕（findings 无持久表）。
    pub(crate) fn sweep_broken_visual_blocks(&mut self) -> Vec<LessonFinding> {
        let mut dropped = Vec::new();
        let keys: Vec<String> = match self.section_manifest.as_ref() {
            Some(manifest) => manifest
                .iter()
                .map(|section| section.section_key.clone())
                .collect(),
            None => return dropped,
        };
        for key in keys {
            let Some((body, stored)) = self.section_bodies.get_mut(&key) else {
                continue;
            };
            let (stripped, removed) =
                crate::models::strip_broken_visual_blocks(body);
            if removed == 0 {
                continue;
            }
            let downgraded = crate::models::declared_visual_after_strip(&stripped);
            *body = stripped;
            let stored = stored.as_mut().expect("stored plan present");
            stored.visual = downgraded.to_owned();
            stored.degraded = true;
            if let Some(section) = self
                .section_manifest
                .as_mut()
                .and_then(|manifest| {
                    manifest
                        .iter_mut()
                        .find(|section| section.section_key == key)
                })
            {
                section.visual = downgraded.to_owned();
                section.degraded = true;
            }
            self.degraded_keys.push(key.clone());
            self.viz_lint_events += removed;
            dropped.push(LessonFinding {
                severity: SEV_WARNING,
                kind: "viz_block_lint_dropped".to_owned(),
                message: format!(
                    "节 {key} 的 {removed} 个坏可视化块已由终检系统剔除，visual 声明降为\
                     「{downgraded}」——修复该围栏块语法或改用表格/公式承载后可恢复原声明"
                ),
            });
            tracing::warn!(
                target: "nomifun_learning",
                section_key = %key,
                removed,
                visual = %downgraded,
                "viz_block_lint_dropped"
            );
        }
        dropped
    }
}

/// The deterministic audit: the fallback pipeline's document + activity
/// rules, inlined with position-aware messages, plus draft-specific
/// warnings. DANGER findings block the finish gate.
fn audit_findings(
    context: &LessonGenerationContext,
    document: Option<&str>,
    estimated_minutes: i64,
    activities: &[ActivityPack],
    section_manifest: Option<&[SectionPack]>,
    section_bodies: &HashMap<String, (String, Option<SectionPack>)>,
    capacity_note: Option<&str>,
) -> Vec<LessonFinding> {
    let mut findings = Vec::new();
    let danger = |kind: &str, message: String| LessonFinding {
        severity: SEV_DANGER,
        kind: kind.to_owned(),
        message,
    };
    let warning = |kind: &str, message: String| LessonFinding {
        severity: SEV_WARNING,
        kind: kind.to_owned(),
        message,
    };

    // ── Content contract: sectioned drafts are the ADR-0002 shape; a
    // legacy single-document draft (set_document) stays valid so old
    // sessions and tests keep publishing. Exactly one of the two must hold.
    let sectioned = section_manifest.is_some() || !section_bodies.is_empty();
    if sectioned {
        match section_manifest {
            None => findings.push(danger(
                "sections_missing",
                "分节正文已写入但节清单缺失（先调用 ls_set_section_manifest）".into(),
            )),
            Some(manifest) => {
                for section in manifest {
                    match section_bodies.get(&section.section_key) {
                        None => findings.push(danger(
                            "section_body_missing",
                            format!(
                                "节 {} [{}] 「{}」还没有正文（ls_set_section_body）",
                                section.section_key,
                                section.kind.label(),
                                section.title.trim()
                            ),
                        )),
                        Some((body, _planned)) => {
                            let candidate = crate::models::SectionPack {
                                section_key: section.section_key.clone(),
                                kind: section.kind,
                                title: section.title.clone(),
                                points: section.points.clone(),
                                visual: section.visual.clone(),
                                body_md: body.clone(),
                                degraded: section.degraded,
                            };
                            if let Err(error) = candidate.validate_body() {
                                findings.push(danger("section_invalid", error));
                            }
                            // f3 观察条（viz_block_lint warning，4.5）：存量正文
                            // 里的坏可视化块点名——修复对照行的触发源。正常路径
                            // op 接纳已拦死坏块，存量坏块只出现在重规划保留正文
                            // /跨版本续跑等旁路；danger 面由 section_invalid 承载。
                            let issues = crate::models::lint_visual_blocks(body);
                            if !issues.is_empty() {
                                let details = crate::models::lint_issue_details(&issues);
                                findings.push(warning(
                                    "viz_block_lint",
                                    format!(
                                        "节 {} 的正文存在可视化块语法问题：{details}——\
                                         修复该围栏块语法，或改用表格/公式承载该节讲解",
                                        section.section_key
                                    ),
                                ));
                            }
                        }
                    }
                }
                // 「无」声明监督（ADR-0008）：概念/例题节全部声明 visual=无
                // 时给非阻断 warning——硬配额会误伤确实无法可视化的纯理论
                // 课时，但整课零可视化的「承诺兑现」形同虚设，值得在审计
                // 报告里点名，让修复轮与发布前自查都能看见。
                let content_sections: Vec<&SectionPack> = manifest
                    .iter()
                    .filter(|section| {
                        matches!(section.kind, SectionKind::Concept | SectionKind::Example)
                    })
                    .collect();
                if !content_sections.is_empty()
                    && content_sections
                        .iter()
                        .all(|section| section.visual.trim() == "无")
                {
                    findings.push(warning(
                        "visual_none_heavy",
                        format!(
                            "全部 {} 个概念/例题节都声明了 visual=无——除非本课时内容确实\
                             无法可视化，否则用 ls_set_section_manifest 重新规划，为承载核心\
                             讲解的节声明具体形态（{}）",
                            content_sections.len(),
                            visual_menu_text()
                        ),
                    ));
                }
            }
        }
    } else {
        match document {
            None => findings.push(danger(
                "document_missing",
                "学习内容尚未写入（先 ls_set_section_manifest 规划分节，再逐节 \
                 ls_set_section_body 写正文）"
                    .into(),
            )),
            Some(document) => {
                if let Err(error) = validate_lesson_document(document) {
                    findings.push(danger("document_invalid", error));
                }
            }
        }
    }

    // ── Activity count floors / caps (validate_lesson_activities rules) ──
    if activities.len() < LESSON_MIN_ACTIVITIES {
        findings.push(danger(
            "activities_too_few",
            format!(
                "lesson has {} activities, expected at least {LESSON_MIN_ACTIVITIES}",
                activities.len()
            ),
        ));
    }
    let objective = activities
        .iter()
        .filter(|activity| activity.kind != ActivityKind::Reflection)
        .count();
    if objective < LESSON_MIN_OBJECTIVE_ACTIVITIES {
        findings.push(danger(
            "objective_activities_too_few",
            format!(
                "lesson has {objective} objective activities, expected at least {LESSON_MIN_OBJECTIVE_ACTIVITIES}"
            ),
        ));
    }
    let reflections = activities.len() - objective;
    if reflections > LESSON_MAX_REFLECTION_ACTIVITIES {
        findings.push(danger(
            "reflections_too_many",
            format!(
                "lesson has {reflections} reflection questions, expected at most {LESSON_MAX_REFLECTION_ACTIVITIES}"
            ),
        ));
    }
    if activities.len() > 5 {
        findings.push(warning(
            "activities_over_target",
            format!("lesson has {} activities; the design guidance is 3-5", activities.len()),
        ));
    }
    // 证据回引（t2）：objective 题缺 evidence 留痕（生成面已硬门拒重试，
    // 这里兜住历史行与旁路写入——修补回合输入面）。
    let missing_evidence = activities
        .iter()
        .filter(|activity| crate::generation::evidence_missing(activity))
        .count();
    if missing_evidence > 0 {
        findings.push(warning(
            "evidence_missing",
            format!(
                "{missing_evidence} objective question(s) carry no evidence quote — the grade-time reveal shows nothing beneath the explanation"
            ),
        ));
    }
    if reflections > 1 {
        findings.push(warning(
            "reflections_multiple",
            "prefer exactly one reflection question per lesson (more only when one question \
             cannot cover the whole lesson)"
                .into(),
        ));
    }

    // ── estimated_minutes sanity ──
    if !(5..=60).contains(&estimated_minutes) {
        findings.push(warning(
            "estimated_minutes_out_of_range",
            format!(
                "estimated_minutes is {estimated_minutes}; the design guidance is 5-60"
            ),
        ));
    }

    // ── 节点意图超载观察条（k03/#178，观察面非阻断）：规划面收敛不住
    // 「时长预算」（保 teaches 覆盖就必超）时的 capacity_note 点名留痕——
    // 它是上游 est 复议的信号，不是可写可不写的自报；让修复轮与发布前
    // 自查看得见。观察键沿用 over_capacity（telemetry 稳定键，语义已换血）。
    if let Some(note) = capacity_note {
        findings.push(warning(
            "over_capacity",
            format!(
                "节点意图超载（{estimated_minutes} 分钟预算收敛不住）：{note}——\
                 先砍雄心再砍内容（减题/并节/降示例密度，teaches 覆盖不可砍）；\
                 本条是上游 est 复议的信号"
            ),
        ));
    }

    // ── u07 节长上限 warning（`NOMI_LEARNING_TIER_PRIORITY` on 臂，warning
    // 不阻断）：审计此前只有下限无上限——负样本「中 250 预算」被 s1（1330
    // 字）突破、全公式推导挤掉认识性讲解。锚 = teaches 档位最高档的文字预
    // 算（[`tier_prose_budget_cap`]，与预算行同源），单节 prose（剥可视化
    // 块，`prose_char_count` 同下限口径）超过锚 2 倍即点名。修复轮与发布
    // 前自查看得见；off 臂零该 finding（基线逐字节）。
    if tier_priority_enabled() && sectioned {
        let teaches_tiers = context
            .graph
            .as_ref()
            .map(|graph| graph.teaches_tiers.as_slice())
            .unwrap_or(&[]);
        let anchor = tier_prose_budget_cap(teaches_tiers);
        let cap = anchor * 2;
        for (key, (body, planned)) in section_bodies {
            let Some(planned) = planned else { continue };
            let counted = crate::models::prose_char_count(body.trim());
            if counted > cap {
                findings.push(warning(
                    "section_over_budget",
                    format!(
                        "section {key}（{}）纯文字 {counted} 字，超过节长预算锚 2 倍\
                         （锚 {anchor} 字 × 2 = 上限 {cap} 字）——按档位收敛正文\
                         （warning 留痕，不阻断发布）",
                        planned.title.trim()
                    ),
                ));
            }
        }
    }

    // ── Per-activity rules (shape), position-aware ──
    let mut seen_prompts: Vec<String> = Vec::new();
    for (position, activity) in activities.iter().enumerate() {
        let at = format!("activity at position {position}");
        if activity.prompt.trim().is_empty() {
            findings.push(danger("activity_prompt_empty", format!("{at}: prompt is empty")));
        }
        // Shared per-kind shape rules (all nine kinds); the generation
        // bounds (3-5 options for choice-like kinds) apply.
        if let Err(error) = activity.validate_shape((3, 5), true) {
            findings.push(danger("activity_shape_invalid", format!("{at}: {error}")));
        }
        // 重复题面（k04 升级）：三口径相似度 ≥0.7 命中即点名命中对与
        // 分值；开关 off 臂退回规范化精确相等（旧观察口径）。
        let prompt = activity.prompt.trim().to_owned();
        if !prompt.is_empty() {
            let hit = if crate::generation::qb_similarity_enabled() {
                crate::generation::similarity_hit(&prompt, &seen_prompts)
                    .map(|(previous, score)| (previous.to_owned(), Some(score)))
            } else {
                let normalized = crate::generation::normalize_prompt(&prompt);
                seen_prompts
                    .iter()
                    .find(|previous| crate::generation::normalize_prompt(previous) == normalized)
                    .map(|previous| (previous.clone(), None))
            };
            if let Some((previous, score)) = hit {
                let detail = match score {
                    Some(score) => format!(
                        "（相似度 {score:.2}，对照「{}」）",
                        previous.chars().take(40).collect::<String>()
                    ),
                    None => String::new(),
                };
                findings.push(warning(
                    "activity_prompt_duplicate",
                    format!("{at}: duplicates an earlier activity's prompt{detail}"),
                ));
            }
            seen_prompts.push(prompt);
        }
    }

    // ── 成色观察条（j07，观察面非阻断——写作纪律不机械化，这里只点名让
    // 修复轮与发布前自查看得见）：概念面（正文用名一致性 / 首现引入）与
    // R9 叙述声部（正文自称「本课」、开发面结构词不入正文）。
    let bodies: Vec<(String, &str)> = section_bodies
        .iter()
        .map(|(key, (body, _))| (key.clone(), body.as_str()))
        .chain(document.iter().map(|body| ("document".to_owned(), *body)))
        .collect();
    if !bodies.is_empty() {
        // 概念面观察（仅图节点路径：概念挂接只在图路径存在）。
        if let Some(faces) = context
            .graph
            .as_ref()
            .and_then(|graph| graph.concept_faces.as_ref())
        {
            let joined = bodies
                .iter()
                .map(|(_, body)| *body)
                .collect::<Vec<_>>()
                .join("\n");
            for (reference, canonical) in &faces.ref_names {
                let uses_reference = joined.contains(reference.as_str());
                let uses_canonical = joined.contains(canonical.as_str());
                if uses_reference && uses_canonical {
                    findings.push(warning(
                        "naming_mixed",
                        format!(
                            "概念「{canonical}」的正文同时出现引用名「{reference}」与正名——\
                             同一概念从头到尾用同一个名字（引用名开讲，正名仅首现括注）"
                        ),
                    ));
                } else if !uses_reference && uses_canonical {
                    findings.push(warning(
                        "naming_canonical_only",
                        format!(
                            "概念「{canonical}」的正文只用了正名，没有用声明的引用名「{reference}」开讲"
                        ),
                    ));
                }
            }
            for concept in &faces.first_seen {
                let mentioned = faces
                    .ref_names
                    .iter()
                    .find(|(_, canonical)| canonical == concept)
                    .map(|(reference, _)| joined.contains(reference.as_str()))
                    .unwrap_or_else(|| joined.contains(concept.as_str()));
                if !mentioned {
                    findings.push(warning(
                        "first_seen_unintroduced",
                        format!(
                            "首现概念「{concept}」的名字没有出现在任何正文里——\
                             首现概念应先用情境引出再给正式定义"
                        ),
                    ));
                }
            }
        }
        // ── scope 边界词面核对（absorb-8 z03，learnhub #205 Q1=b 域中立
        // 移植，报告-only 零拦截）：「教学范围」声明排除段名词串 vs 节标题
        // 与正文——「不该无痕地生成」由人判读 findings，不由机械打回；
        // 否定性提及豁免（范围外点名 ≠ 引用）。scope 缺席/空白（旧档）=
        // 零扫描零开销（模块对空 scope 恒返回空表）。
        if let Some(scope) = context
            .graph
            .as_ref()
            .and_then(|graph| graph.scope.as_deref())
        {
            // 排除词串每课时提取一次，逐节复用（scope None = 不提取零开销）。
            let excluded_terms =
                crate::learning_graph::scope_findings::excluded_terms(scope);
            for section in section_manifest.into_iter().flatten() {
                let body = section_bodies
                    .get(&section.section_key)
                    .map(|(body, _)| body.as_str())
                    .unwrap_or("");
                for message in crate::learning_graph::scope_findings::scan_section_with_terms(
                    &excluded_terms,
                    &section.section_key,
                    &section.title,
                    body,
                ) {
                    findings.push(warning("scope_boundary_hit", message));
                }
            }
            if let Some(document) = document {
                for message in crate::learning_graph::scope_findings::scan_body(scope, document) {
                    findings.push(warning("scope_boundary_hit", message));
                }
            }
        }
        // R9 叙述声部（两条管线通用）：逐类点名命中节。
        const NARRATIVE_SELF: &[&str] = &["本课", "这一课", "本课时"];
        const STRUCTURE_WORDS: &[&str] = &["本节将", "下一节将", "接下来我们将", "如前所述"];
        for (kind, terms, label) in [
            (
                "narrative_self_reference",
                NARRATIVE_SELF,
                "正文自称「本课/这一课」（学习者读的是学习材料，不是课程导语）",
            ),
            (
                "narrative_structure_words",
                STRUCTURE_WORDS,
                "开发面结构词混入正文（结构导航是大纲的事，不是正文的）",
            ),
        ] {
            let mut hits: Vec<String> = Vec::new();
            for (key, body) in &bodies {
                let matched: Vec<&str> = terms
                    .iter()
                    .filter(|term| body.contains(**term))
                    .copied()
                    .collect();
                if !matched.is_empty() {
                    hits.push(format!("{}（{}）", key, matched.join("、")));
                }
            }
            if !hits.is_empty() {
                findings.push(warning(kind, format!("{label}：{}", hits.join("；"))));
            }
        }
    }
    findings
}

/// The lesson-content agent engine seam: nomifun-learning owns the draft,
/// the deterministic audit and the finish gate; an injected engine (the
/// nomifun-ai-agent two-loop implementation) produces the content. Without
/// an engine, `generate_lesson_content` falls back to the legacy two-stage
/// one-shot pipeline.
#[async_trait::async_trait]
pub trait LessonContentAgentEngine: Send + Sync {
    /// Run the lesson agent loop; returns the audit-gated lesson output.
    async fn generate(
        &self,
        user_id: &nomifun_common::UserId,
        context: &LessonGenerationContext,
        model_override: Option<(&str, &str)>,
    ) -> Result<LessonOutput, nomifun_common::AppError>;

    /// Resume a live draft from a previous interrupted run: the opening
    /// user turn carries the draft's current state plus the archived round
    /// logs, so the model continues where it stopped instead of rebuilding
    /// from scratch (same mechanism as the learning-graph loop's resume).
    async fn resume(
        &self,
        user_id: &nomifun_common::UserId,
        draft_id: &str,
        context: &LessonGenerationContext,
        model_override: Option<(&str, &str)>,
    ) -> Result<LessonOutput, nomifun_common::AppError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> LessonGenerationContext {
        LessonGenerationContext {
            lesson_id: "lesson-1".into(),
            course_title: "测试课程".into(),
            course_description: "零基础期权入门".into(),
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

    /// ≥800 non-whitespace characters across the three required sections
    /// (10 × 30-char sentence per section = 912 including the headings).
    fn long_document() -> String {
        let body = "这是一个用于测试的完整段落，覆盖课时要求的知识点并且足够长。".repeat(10);
        format!("## 描述\n{body}\n## 例子\n{body}\n## 验证\n{body}\n")
    }

    fn activity_json(kind: &str, extra: serde_json::Value) -> ActivityPack {
        let mut value = serde_json::json!({
            "kind": kind,
            "prompt": "期权的本质是什么？",
            "explanation": "因为买方持有权利。"
        });
        if let (Some(object), Some(extra)) = (value.as_object_mut(), extra.as_object()) {
            object.extend(extra.clone());
        }
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn empty_draft_audits_document_and_activity_dangers() {
        let draft = LessonDraft::new(context());
        let kinds: Vec<&str> = draft.findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"document_missing"));
        assert!(kinds.contains(&"activities_too_few"));
        assert!(kinds.contains(&"objective_activities_too_few"));
        assert!(draft.audit_report().contains("[danger]"));
    }

    /// 成色观察条（j07，观察面非阻断）：概念面（用名混用/只用正名/首现
    /// 未引入）与 R9 叙述声部——warning 级、不阻断发布；传统路径（无概念
    /// 面）不产生概念面观察。
    #[test]
    fn audit_observes_content_discipline_without_blocking() {
        let mut ctx = context();
        ctx.graph = Some(GraphLessonContext {
            goal: "g".into(),
            degree_statement: "d".into(),
            prerequisite_path: String::new(),
            upcoming_nodes: String::new(),
            concept_faces: Some(GraphConceptFaces {
                first_seen: vec!["Delta".into()],
                returning: vec![],
                ref_names: vec![("Delta".into(), "德尔塔".into())],
                aliases: vec![],
            }),
            est_minutes: Some(20),
            known_names: None,
            teaches_tiers: Vec::new(),
            precedents: Vec::new(),
            intent: None,
            scope: None,
        });
        let mut draft = LessonDraft::new(ctx);
        // 正文同时出现引用名与正名（混用）+ 首现名在场 + R9 自称。
        let body = "这是一个用于测试的完整段落，覆盖课时要求的知识点并且足够长。".repeat(10);
        draft.apply_ops(vec![LessonOp::SetDocument {
            document: format!(
                "## 描述\n{body}Delta 与德尔塔是同一回事。本课将介绍它的用法。\n## 例子\n{body}\n## 验证\n{body}\n"
            ),
        }]);
        assert!(
            draft
                .findings
                .iter()
                .any(|f| f.severity == SEV_WARNING && f.kind == "naming_mixed"),
            "mixed naming observed: {:?}",
            draft.findings
        );
        assert!(
            !draft
                .findings
                .iter()
                .any(|f| f.kind == "first_seen_unintroduced"),
            "first-seen name present → no unintroduced observation"
        );
        assert!(
            draft
                .findings
                .iter()
                .any(|f| f.severity == SEV_WARNING && f.kind == "narrative_self_reference"),
            "R9 self-reference observed"
        );
        // 观察面不阻断：danger 只来自活动下限等既有契约。
        assert!(
            !draft
                .findings
                .iter()
                .any(|f| f.severity == SEV_DANGER && f.kind.contains("narrative"))
        );

        // 只用正名、不用引用名 → naming_canonical_only；首现名缺席 →
        // first_seen_unintroduced。
        let mut ctx = context();
        ctx.graph = Some(GraphLessonContext {
            goal: "g".into(),
            degree_statement: "d".into(),
            prerequisite_path: String::new(),
            upcoming_nodes: String::new(),
            concept_faces: Some(GraphConceptFaces {
                first_seen: vec!["Vega".into()],
                returning: vec![],
                ref_names: vec![("Delta".into(), "德尔塔".into())],
                aliases: vec![],
            }),
            est_minutes: Some(20),
            known_names: None,
            teaches_tiers: Vec::new(),
            precedents: Vec::new(),
            intent: None,
            scope: None,
        });
        let mut draft = LessonDraft::new(ctx);
        let body = "这是一个用于测试的完整段落，覆盖课时要求的知识点并且足够长。".repeat(10);
        draft.apply_ops(vec![LessonOp::SetDocument {
            document: format!("## 描述\n{body}德尔塔的定义。\n## 例子\n{body}\n## 验证\n{body}\n"),
        }]);
        assert!(
            draft
                .findings
                .iter()
                .any(|f| f.kind == "naming_canonical_only")
        );
        assert!(
            draft
                .findings
                .iter()
                .any(|f| f.kind == "first_seen_unintroduced")
        );

        // 传统路径（graph None）：概念面观察缺席，R9 观察仍然生效。
        let mut draft = LessonDraft::new(context());
        draft.apply_ops(vec![LessonOp::SetDocument {
            document: format!("## 描述\n{body}本课将介绍。\n## 例子\n{body}\n## 验证\n{body}\n"),
        }]);
        assert!(
            !draft
                .findings
                .iter()
                .any(|f| f.kind.starts_with("naming_"))
        );
        assert!(
            draft
                .findings
                .iter()
                .any(|f| f.kind == "narrative_self_reference")
        );
    }

    /// scope 边界词面核对（absorb-8 z03，报告-only）：图路径 scope 声明
    /// 排除段名词串撞节标题/正文 → `scope_boundary_hit` warning（指名来源
    /// 字段「教学范围」与命中位置）；否定性提及豁免；scope 缺席/空（旧档）
    /// 与传统路径零扫描零 finding。
    #[test]
    fn audit_names_scope_boundary_hits_without_blocking() {
        let graph_ctx = |scope: Option<&str>| {
            let mut ctx = context();
            ctx.graph = Some(GraphLessonContext {
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
                scope: scope.map(str::to_owned),
            });
            ctx
        };
        let body = "这是一个用于测试的完整段落，覆盖课时要求的知识点并且足够长。".repeat(10);
        // 播种负样本（票面场景）：scope「不引入方程」+ 节标题含「方程」→
        // 命中一条，指名来源与位置。
        let mut draft = LessonDraft::new(graph_ctx(Some("教四则运算；不引入方程")));
        draft.apply_ops(vec![LessonOp::SetDocument {
            document: format!("## 描述\n{body}\n## 例子·方程的初步认识\n{body}\n## 验证\n{body}\n"),
        }]);
        let hits: Vec<&LessonFinding> = draft
            .findings
            .iter()
            .filter(|f| f.kind == "scope_boundary_hit")
            .collect();
        assert_eq!(hits.len(), 1, "标题命中一条：{:?}", draft.findings);
        assert_eq!(hits[0].severity, SEV_WARNING, "观察条非阻断");
        assert!(
            hits[0].message.contains("方程")
                && hits[0].message.contains("教学范围")
                && hits[0].message.contains("节标题"),
            "finding 指名来源字段与命中位置：{}",
            hits[0].message
        );

        // 否定性提及豁免：正文只在否定子句里点名边界 = 零 finding。
        let mut draft = LessonDraft::new(graph_ctx(Some("教四则运算；不引入方程")));
        draft.apply_ops(vec![LessonOp::SetDocument {
            document: format!(
                "## 描述\n{body}本课不引入方程，后续课程再展开。\n## 例子\n{body}\n## 验证\n{body}\n"
            ),
        }]);
        assert!(
            !draft
                .findings
                .iter()
                .any(|f| f.kind == "scope_boundary_hit"),
            "否定性提及豁免不报：{:?}",
            draft.findings
        );

        // 旧档零扫描：scope None / 空串（DB 默认）= 正文撞词也不报。
        for scope in [None, Some("")] {
            let mut draft = LessonDraft::new(graph_ctx(scope));
            draft.apply_ops(vec![LessonOp::SetDocument {
                document: format!("## 描述·方程\n{body}\n## 例子\n{body}\n## 验证\n{body}\n"),
            }]);
            assert!(
                !draft
                    .findings
                    .iter()
                    .any(|f| f.kind == "scope_boundary_hit"),
                "旧档（scope {scope:?}）零扫描零 finding"
            );
        }

        // 传统路径（graph None）：无声明面，恒零扫描。
        let mut draft = LessonDraft::new(context());
        draft.apply_ops(vec![LessonOp::SetDocument {
            document: format!("## 描述·方程\n{body}\n## 例子\n{body}\n## 验证\n{body}\n"),
        }]);
        assert!(
            !draft
                .findings
                .iter()
                .any(|f| f.kind == "scope_boundary_hit"),
            "传统路径零扫描"
        );
    }

    #[test]
    fn gate_rules_mirror_validate_lesson_contract() {
        let mut draft = LessonDraft::new(context());
        draft.apply_ops(vec![LessonOp::SetDocument {
            document: long_document(),
        }]);
        // Document only: activities dangers remain, document dangers gone.
        let kinds: Vec<&str> = draft.findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(!kinds.contains(&"document_missing"));
        assert!(!kinds.contains(&"document_invalid"));
        assert!(kinds.contains(&"activities_too_few"));

        // A short document fails the length floor through the same validator.
        draft.apply_ops(vec![LessonOp::SetDocument {
            document: "## 描述\n太短\n## 例子\n还是短\n## 验证\n依然短".into(),
        }]);
        assert!(draft
            .findings
            .iter()
            .any(|f| f.kind == "document_invalid"
                && f.message.contains("non-whitespace characters")));

        // 补齐客观题后客观题下限告警消失（概念绑定校验已随概念体系退役，
        // 题目不再携带 concept 绑定）。
        draft.apply_ops(vec![LessonOp::SetDocument {
            document: long_document(),
        }]);
        draft.apply_ops(vec![
            LessonOp::AddActivity {
                activity: activity_json("single_choice", serde_json::json!({
                    "options": ["权利", "义务", "债务"], "answer": "权利"
                })),
            },
            LessonOp::AddActivity {
                activity: activity_json("true_false", serde_json::json!({ "answer": true })),
            },
            LessonOp::AddActivity {
                activity: activity_json("reflection", serde_json::json!({ "answer": null })),
            },
        ]);
        assert!(!draft
            .findings
            .iter()
            .any(|f| f.kind == "objective_activities_too_few"));
        assert!(!draft
            .findings
            .iter()
            .any(|f| f.severity == SEV_DANGER && f.kind.starts_with("activity")));
    }

    #[test]
    fn finish_gate_converts_a_clean_draft_into_lesson_output() {
        let mut draft = LessonDraft::new(context());
        draft.apply_ops(vec![
            LessonOp::SetDocument {
                document: long_document(),
            },
            LessonOp::SetEstimatedMinutes { minutes: 15 },
            LessonOp::AddActivity {
                activity: activity_json("single_choice", serde_json::json!({
                    "options": ["权利", "义务", "债务"], "answer": "权利"
                })),
            },
            LessonOp::AddActivity {
                activity: activity_json("true_false", serde_json::json!({ "answer": false })),
            },
            LessonOp::AddActivity {
                activity: activity_json("reflection", serde_json::json!({ "answer": null })),
            },
        ]);
        assert!(draft.findings.iter().all(|f| f.severity != SEV_DANGER));
        let output = draft.to_output();
        assert_eq!(output.estimated_minutes, 15);
        assert_eq!(output.activities.len(), 3);
        assert!(output.summary.contains("## 描述"));
        // The gate itself lives on the service facade; here only the shape.
        assert_eq!(draft.view("d1").grounding, "excerpt:docs/basics.md");
    }

    #[test]
    fn rejected_ops_do_not_bump_the_revision() {
        let mut draft = LessonDraft::new(context());
        let report = draft.apply_ops(vec![
            LessonOp::RemoveActivity { position: 3 },
            LessonOp::SetEstimatedMinutes { minutes: 0 },
        ]);
        assert_eq!(report.accepted.len(), 0);
        assert_eq!(report.rejected.len(), 2);
        assert_eq!(report.revision, 0, "no accepted op, no revision bump");
        assert_eq!(report.rejected[0].op, "remove_activity");
        assert!(report.rejected[1].reason.contains("1-240"));

        let report = draft.apply_ops(vec![LessonOp::SetEstimatedMinutes { minutes: 20 }]);
        assert_eq!(report.revision, 1);
        assert_eq!(draft.estimated_minutes, 20);
    }

    fn manifest_pack(key: &str, kind: SectionKind, visual: &str) -> SectionPack {
        SectionPack {
            section_key: key.into(),
            kind,
            title: format!("t-{key}"),
            points: String::new(),
            visual: visual.into(),
            body_md: String::new(),
            degraded: false,
        }
    }

    /// f3 图线打回通道（4.2/A3）：坏 mermaid 的 op 被 validate_body 拒（拒绝
    /// 理由含 lint 定位与处置指引），正文不入库；重提合法 mermaid 过闸。
    /// lint 打回事件计数留痕（publish 帧 viz_lint_events 的数据源）。
    #[test]
    fn viz_lint_rejects_the_bad_mermaid_op_then_accepts_the_fixed_resubmit() {
        let mut draft = LessonDraft::new(context());
        draft
            .apply_ops(vec![LessonOp::SetSectionManifest {
                sections: vec![
                    manifest_pack("s1", SectionKind::Concept, "流程图"),
                    manifest_pack("s2", SectionKind::Practice, "无"),
                ],
                capacity_note: None,
            }])
            .accepted
            .into_iter()
            .next()
            .expect("manifest accepted");
        let prose =
            "这是一个足够长的概念讲解正文段落，用于通过节级质检门的文字下限要求。".repeat(6);
        // op 面红样取 fix_mermaid_quotes 不可吸收的形态（两个 `[` 开、无闭合
        // `]`——无引号跨度可包裹，fix 原样返回，块级净差 +2 必红）。法证原行
        // 本体在 lint 层红样钉死（models::viz_lint_tests）；它经 fix 的引号改
        // 写后括号恰被"配平"（现行为不动，蓝图禁区），op 面不再对它打回。
        let bad = format!(
            "## t-s1\n\n```mermaid\nflowchart TD\n\
             A[本金 P -->|算利息| B[利息1\n```\n\n{prose}"
        );
        let report = draft.apply_ops(vec![LessonOp::SetSectionBody {
            section_key: "s1".into(),
            body: bad,
        }]);
        assert_eq!(report.rejected.len(), 1, "坏块 op 被拒");
        assert!(
            report.rejected[0]
                .reason
                .contains("invalid visualization block syntax"),
            "{}",
            report.rejected[0].reason
        );
        assert!(
            report.rejected[0].reason.contains("修复该围栏块语法"),
            "{}",
            report.rejected[0].reason
        );
        assert!(draft.section_bodies.is_empty(), "被拒正文不入库");
        // 计数按 issue 条：该坏块两条查项同时命中（括号净差 +2 + 泄漏端点
        // 「P」无形状定义）→ 2 条事件。
        assert_eq!(draft.viz_lint_events, 2, "lint 打回事件计数");

        // 重提合法 mermaid 过闸；观察条不误报。
        let good = format!(
            "## t-s1\n\n```mermaid\nflowchart TD\n\
             A[本金] -->|计息| B[本息]\n```\n\n{prose}"
        );
        let report = draft.apply_ops(vec![LessonOp::SetSectionBody {
            section_key: "s1".into(),
            body: good,
        }]);
        assert_eq!(report.accepted.len(), 1, "合法 mermaid 过闸");
        assert_eq!(draft.viz_lint_events, 2, "合法重提不计事件");
        assert!(!draft.findings.iter().any(|f| f.kind == "viz_block_lint"));

        // 模型不得走私 degraded 豁免（f3：系统 mutation 专属标记，op 入口
        // 统一清零——demo 硬门豁免只属于终检兜底面）。
        let smuggled: LessonOp = serde_json::from_value(serde_json::json!({
            "op": "set_section_manifest",
            "sections": [
                { "section_key": "s1", "kind": "concept", "title": "t-s1",
                  "points": "", "visual": "流程图", "degraded": true },
                { "section_key": "s2", "kind": "practice", "title": "t-s2",
                  "points": "", "visual": "无", "degraded": true }
            ]
        }))
        .expect("op JSON parses");
        draft.apply_ops(vec![smuggled]);
        assert!(
            draft
                .section_manifest
                .iter()
                .flatten()
                .all(|section| !section.degraded),
            "op 入口的 degraded 被清零"
        );
    }

    /// f3 观察条（4.5）：存量坏块（重规划保留/跨版本续跑等旁路入库形态）
    /// 在审计里以 viz_block_lint warning 点名（修复对照行的触发源）；
    /// danger 面由 section_invalid 承载（lint 已并入 validate_body）。
    #[test]
    fn audit_names_stored_broken_visual_blocks_as_viz_block_lint_warnings() {
        let mut draft = LessonDraft::new(context());
        draft
            .apply_ops(vec![LessonOp::SetSectionManifest {
                sections: vec![
                    manifest_pack("s1", SectionKind::Concept, "流程图"),
                    manifest_pack("s2", SectionKind::Practice, "无"),
                ],
                capacity_note: None,
            }])
            .accepted
            .into_iter()
            .next()
            .expect("manifest accepted");
        let prose =
            "这是一个足够长的概念讲解正文段落，用于通过节级质检门的文字下限要求。".repeat(6);
        let bad = format!(
            "## t-s1\n\n```mermaid\nflowchart TD\n\
             A[[\"第1期 本金 P] -->|\"算利息 P×利率\"| B[利息1\"]\n```\n\n{prose}"
        );
        // 旁路入库（字段直塞，不过 op）——lint 落地前已存量的正文形态。
        draft.section_bodies.insert(
            "s1".to_owned(),
            (
                bad,
                Some(manifest_pack("s1", SectionKind::Concept, "流程图")),
            ),
        );
        draft.refresh_audit();
        let warning = draft
            .findings
            .iter()
            .find(|finding| finding.kind == "viz_block_lint")
            .expect("存量坏块被点名");
        assert_eq!(warning.severity, SEV_WARNING, "观察条非阻断");
        assert!(warning.message.contains("s1"), "{}", warning.message);
        assert!(warning.message.contains("修复该围栏块语法"));
        assert!(
            draft.findings.iter().any(|finding| finding.kind == "section_invalid"
                && finding.severity == SEV_DANGER),
            "lint 违约经 validate_body 进 danger"
        );
    }

    /// 「无」声明监督（ADR-0008）：概念/例题节全部声明 visual=无 时审计给
    /// 非阻断 warning 点名；只要有一节声明了具体形态，警告即消失。
    #[test]
    fn visual_none_heavy_warns_when_every_content_section_declares_none() {
        let mut draft = LessonDraft::new(context());
        draft
            .apply_ops(vec![LessonOp::SetSectionManifest {
                sections: vec![
                    manifest_pack("s1", SectionKind::Concept, "无"),
                    manifest_pack("s2", SectionKind::Example, "无"),
                    manifest_pack("s3", SectionKind::Practice, "无"),
                ],
                capacity_note: None,
            }])
            .accepted
            .into_iter()
            .next()
            .expect("manifest accepted");
        let warning = draft
            .findings
            .iter()
            .find(|f| f.kind == "visual_none_heavy")
            .expect("an all-无 manifest must be named");
        assert_eq!(warning.severity, SEV_WARNING, "supervision warns, it never blocks");
        assert!(warning.message.contains("visual=无"));

        // 具体声明落位（练习节无关本检查）——警告消失。
        draft
            .apply_ops(vec![LessonOp::SetSectionManifest {
                sections: vec![
                    manifest_pack("s1", SectionKind::Concept, "表格"),
                    manifest_pack("s2", SectionKind::Example, "公式"),
                    manifest_pack("s3", SectionKind::Practice, "无"),
                ],
                capacity_note: None,
            }])
            .accepted
            .into_iter()
            .next()
            .expect("replan accepted");
        assert!(!draft
            .findings
            .iter()
            .any(|f| f.kind == "visual_none_heavy"));
    }

    /// ── k03：est 时长预算行 + 超容自报 capacity_note ──

    /// 预算行装配：est 值逐字在场；开关取值判定只认 off/0/false。
    #[test]
    fn est_budget_line_embeds_est_and_switch_values_are_strict() {
        let line = render_est_budget_line(20);
        assert!(line.contains("时长预算：20 分钟"), "{line}");
        assert!(line.contains("节点尺寸锚"), "{line}");
        for value in ["off", "OFF", "0", "false", " False "] {
            assert!(is_est_budget_disabled_value(value), "{value:?} 关");
        }
        for value in ["on", "1", "true", "", "disable"] {
            assert!(!is_est_budget_disabled_value(value), "{value:?} 不关");
        }
    }

    /// ── n3：Known Names 行 / teaches 档位行 / 先例块（渲染面）──

    /// Known Names 行：名单逐字在场 + 边界措辞；空名单整行省略（None）。
    #[test]
    fn known_names_line_renders_the_boundary_and_omits_empty() {
        assert_eq!(render_known_names_line(&[]), None, "空名单整行省略");
        let line = render_known_names_line(&["对数收益率".into(), "历史波动率".into()])
            .expect("非空名单有行");
        assert!(line.contains("课程已知概念"), "{line}");
        assert!(line.contains("对数收益率、历史波动率"), "{line}");
        assert!(line.contains("本课已教 ∪ 已会声明"), "{line}");
        assert!(line.contains("名单外的领域名词不引入"), "{line}");
    }

    /// teaches 档位行：深度角（概念（档位））逐字在场；空 teaches 省行。
    #[test]
    fn teaches_tier_line_renders_the_depth_anchor_and_omits_empty() {
        assert_eq!(render_teaches_tier_line(&[]), None, "空 teaches 省行");
        let line = render_teaches_tier_line(&[("历史波动率".into(), "知道".into())])
            .expect("非空档位有行");
        assert!(line.contains("节点档位"), "{line}");
        assert!(line.contains("历史波动率（知道）"), "{line}");
        assert!(line.contains("内容形态与练习形态按档位定"), "{line}");
    }

    /// 先例块（p02 双传收敛）：概念 ← 节点映射只承载概念归属 + 用名纪律
    /// 所需的最小事实——前置路径条目已带节点要点摘要，本块不再重复节
    /// 清单（无括注尾巴、无「正文未生成」标记）；空先例整块省略。
    #[test]
    fn precedent_block_renders_concept_node_mapping_and_omits_empty() {
        assert_eq!(render_precedent_block(&[]), None, "空先例整块省略");
        let block = render_precedent_block(&[
            LessonPrecedent {
                concept: "对数收益率".into(),
                node_name: "收益率与对数收益率".into(),
            },
            LessonPrecedent {
                concept: "历史波动率".into(),
                node_name: "未生成节点".into(),
            },
        ])
        .expect("非空先例有块");
        assert!(block.contains("已建立先例"), "{block}");
        assert!(block.contains("概念「对数收益率」← 节点「收益率与对数收益率」"), "{block}");
        assert!(block.contains("概念「历史波动率」← 节点「未生成节点」"), "{block}");
        // 双传收敛：节清单不进先例块（细节由前置路径承载），零正文场景
        // 不再有「（正文未生成）」尾巴。
        assert!(!block.contains("概念：对数收益"), "{block}");
        assert!(!block.contains("例题：换算"), "{block}");
        assert!(!block.contains("正文未生成"), "{block}");
    }

    /// 前置路径块头（p02 B 双头合并）：单一头吸收「前置契约」句——不再
    /// 出现独立的「前置契约：」行；档位标尺括注由 render_prerequisite_path
    /// 保留一次。空/空白路径整块省略（起点节点「没有前置」句照走旧分支）。
    #[test]
    fn prerequisite_path_block_renders_single_merged_head() {
        assert_eq!(render_prerequisite_path_block(""), None, "空路径整块省略");
        assert_eq!(
            render_prerequisite_path_block("   \n"),
            None,
            "空白路径按缺席折叠"
        );
        let block = render_prerequisite_path_block(
            "- 对数收益率（学习者应已掌握到「会用」）：\n   定义：价格比取对数的一阶近似收益度量",
        )
        .expect("非空路径有块");
        assert!(
            block.starts_with(
                "前置路径（学习者到达本节点前应已掌握，按学习顺序；概念已由其他节点教到标注档位——不要重新讲授，只在必要时按该档位复述或引用。条目下的要点是各前置实际教过的内容——不要重复讲授）：\n"
            ),
            "{block}"
        );
        assert!(block.contains("- 对数收益率"), "{block}");
        // 单头单次：块内不再有独立「前置契约：」行。
        assert!(!block.contains("前置契约："), "{block}");
    }

    /// 图臂头两行（p02 C 空值行省略）：goal/degree 齐全两行在场；degree
    /// 空白省「程度声明」行；全空两行全省（known_names 省行先例同形）。
    #[test]
    fn graph_goal_degree_lines_omit_empty_values() {
        let both = render_graph_goal_degree_lines(
            "  理解衍生品的基础计量工具  ",
            "聚焦场内标准期权，不含结构性产品",
        );
        assert_eq!(
            both,
            "学习目标：理解衍生品的基础计量工具\n程度声明：聚焦场内标准期权，不含结构性产品\n",
            "{both}"
        );
        let no_degree = render_graph_goal_degree_lines("理解衍生品的基础计量工具", "   ");
        assert_eq!(
            no_degree,
            "学习目标：理解衍生品的基础计量工具\n",
            "{no_degree}"
        );
        assert!(!no_degree.contains("程度声明"), "{no_degree}");
        let all_empty = render_graph_goal_degree_lines("", "  ");
        assert_eq!(all_empty, "", "全空两行全省");
    }

    /// 无简报尾注（p02 C，真机 L33）：列项动态拼装——空值项省略，不再
    /// 指向空白程度声明；两项齐全时逐字等于旧行（既有形态零变化）。
    #[test]
    fn graph_no_brief_note_omits_empty_items() {
        let full = render_graph_no_brief_note("理解衍生品的基础计量工具", "聚焦场内标准期权");
        assert_eq!(
            full,
            "\n学习图节点没有课程简报：内容忠于学习目标、程度声明与前置/后续节点段落。\n",
            "{full}"
        );
        let no_degree = render_graph_no_brief_note("理解衍生品的基础计量工具", " ");
        assert_eq!(
            no_degree,
            "\n学习图节点没有课程简报：内容忠于学习目标与前置/后续节点段落。\n",
            "{no_degree}"
        );
        let all_empty = render_graph_no_brief_note("", "");
        assert_eq!(
            all_empty,
            "\n学习图节点没有课程简报：内容忠于前置/后续节点段落。\n",
            "{all_empty}"
        );
    }

    /// 旧叫法行方向化（p02 D）：「本课现用名（旧称：…）」——首现概念与
    /// 旧叫法并读不再可能误读（真机首现概念恰为行内正名，「X 又叫 Y +
    /// 正文不使用」的方向读法易误伤）；数据面零改动（aliases 仍为活跃
    /// 别名池投影）。
    #[test]
    fn concept_faces_aliases_line_reads_direction_explicitly() {
        let faces = GraphConceptFaces {
            first_seen: vec!["历史波动率".into()],
            returning: Vec::new(),
            ref_names: Vec::new(),
            aliases: vec![(
                "历史波动率".into(),
                vec!["贝塔系数".into(), "Beta".into()],
            )],
        };
        let text = faces.render_text();
        assert!(
            text.contains(
                "- 本课现用名（别处可能见过的旧叫法正文不使用，必要时括注一次帮助辨认）：历史波动率（旧称：贝塔系数/Beta）"
            ),
            "{text}"
        );
        // 旧方向措辞退场：「X 又叫 Y」不再出现。
        assert!(!text.contains("又叫"), "{text}");
    }

    /// 声明面骨架行（absorb-8 z03，learnhub #205 契约 v2.17）：两字段齐 =
    /// 两行在场；单缺席 = 只渲染在场行；全空/全缺席 = 整段省略（`None`，
    /// 旧档零变化——Known Names 省行先例同形）。
    #[test]
    fn intent_scope_lines_render_present_rows_and_omit_absent() {
        // 全缺席（旧档）与全空串（DB 默认折叠后的等价形）= 整段省略。
        assert_eq!(render_intent_scope_lines(None, None), None, "旧档整段省略");
        assert_eq!(
            render_intent_scope_lines(Some(""), Some("   ")),
            None,
            "空串/空白串按缺席折叠"
        );
        // 两字段齐：两行逐字在场（行格式逐字固定——真机冒烟机械 grep 面）。
        let both = render_intent_scope_lines(
            Some("在收益率之后解释波动率，承接换算例题"),
            Some("教波动率的直观刻画；不引入 GARCH 模型"),
        )
        .expect("两字段齐有两行");
        assert!(both.contains("创建意图：在收益率之后解释波动率，承接换算例题\n"), "{both}");
        assert!(
            both.contains("教学范围：教波动率的直观刻画；不引入 GARCH 模型（范围外内容不教，越界点名不构成引用）\n"),
            "{both}"
        );
        // 单缺席：只渲染在场行（缺 intent 只出范围行，反之亦然）。
        let scope_only =
            render_intent_scope_lines(None, Some("教基本运算；不展开对数")).expect("有 scope 有行");
        assert!(scope_only.starts_with("教学范围："), "{scope_only}");
        assert!(!scope_only.contains("创建意图"), "{scope_only}");
        let intent_only =
            render_intent_scope_lines(Some("补齐内存安全主线"), None).expect("有 intent 有行");
        assert!(intent_only.starts_with("创建意图："), "{intent_only}");
        assert!(!intent_only.contains("教学范围"), "{intent_only}");
    }

    /// est 铸值（k03/#178）：图节点带 est 时草稿 estimated_minutes 以节点
    /// 尺寸锚开盘（模型仍可 set_estimated_minutes 复议）；传统课时维持旧
    /// 默认 10；越界 est（>240）不铸回旧默认。
    #[test]
    // 读 `NOMI_LEARNING_EST_BUDGET` 开关（est_budget_enabled）——必须与改写
    // 该环境变量的 serial 开关臂测试互斥（并行读到 off 臂 = 铸值消失）。
    #[serial_test::serial]
    fn draft_seeds_estimated_minutes_from_the_node_est() {
        let mut ctx = context();
        ctx.graph = Some(GraphLessonContext {
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
        assert_eq!(LessonDraft::new(ctx).estimated_minutes, 20, "节点 est 铸值");

        // 传统课时（graph None）：旧默认。
        assert_eq!(LessonDraft::new(context()).estimated_minutes, 10);

        // 越界 est 不铸（草稿键值域 1-240）。
        let mut ctx = context();
        ctx.graph = Some(GraphLessonContext {
            goal: "g".into(),
            degree_statement: "d".into(),
            prerequisite_path: String::new(),
            upcoming_nodes: String::new(),
            concept_faces: None,
            est_minutes: Some(500),
            known_names: None,
            teaches_tiers: Vec::new(),
            precedents: Vec::new(),
            intent: None,
            scope: None,
        });
        assert_eq!(LessonDraft::new(ctx).estimated_minutes, 10, "越界 est 回默认");
    }

    /// 开关生效：off 时 est_budget_enabled() 为 false（默认开）。
    #[test]
    #[serial_test::serial]
    fn est_budget_switch_toggles_the_env_gate() {
        struct Restore(Option<String>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => unsafe { std::env::set_var(EST_BUDGET_SWITCH_ENV, value) },
                    None => unsafe { std::env::remove_var(EST_BUDGET_SWITCH_ENV) },
                }
            }
        }
        let previous = std::env::var(EST_BUDGET_SWITCH_ENV).ok();
        unsafe { std::env::set_var(EST_BUDGET_SWITCH_ENV, "off") };
        let _restore = Restore(previous);
        assert!(!est_budget_enabled(), "off 臂关闭预算行与自报");

        unsafe { std::env::set_var(EST_BUDGET_SWITCH_ENV, "on") };
        assert!(est_budget_enabled(), "on 臂开启");
    }

    /// capacity_note 形状三态：缺席合法 / 非空通过（trim 保留）/ 空白串
    /// 按空裁成 None——恶劣值永不退稿。
    #[test]
    #[serial_test::serial]
    fn capacity_note_shape_is_non_empty_string_or_absent() {
        assert_eq!(normalize_capacity_note(None), None, "缺席合法");
        assert_eq!(
            normalize_capacity_note(Some("  节 s2 明显超容，约多 8 分钟  ")),
            Some("节 s2 明显超容，约多 8 分钟".into()),
            "非空自报 trim 保留"
        );
        assert_eq!(normalize_capacity_note(Some("   \n\t ")), None, "空白按缺席");

        // 计划 op 三态走通：只收非空自报，不产生 danger。
        let mut draft = LessonDraft::new(context());
        let report = draft.apply_ops(vec![LessonOp::SetSectionManifest {
            sections: vec![
                manifest_pack("s1", SectionKind::Concept, "表格"),
                manifest_pack("s2", SectionKind::Practice, "无"),
            ],
            capacity_note: Some("  s1 规划偏长，预计超 6 分钟 ".into()),
        }]);
        assert_eq!(report.accepted.len(), 1);
        assert_eq!(
            draft.capacity_note.as_deref(),
            Some("s1 规划偏长，预计超 6 分钟")
        );
        assert!(!draft
            .findings
            .iter()
            .any(|f| f.severity == SEV_DANGER && f.kind == "over_capacity"));
        // 观察条点名（warning 非阻断）。
        assert!(
            draft
                .findings
                .iter()
                .any(|f| f.severity == SEV_WARNING && f.kind == "over_capacity"),
            "自报进观察条: {:?}",
            draft.findings
        );

        // 空白自报 → 缺席（重规划覆盖）。
        draft.apply_ops(vec![LessonOp::SetSectionManifest {
            sections: vec![
                manifest_pack("s1", SectionKind::Concept, "表格"),
                manifest_pack("s2", SectionKind::Practice, "无"),
            ],
            capacity_note: Some("   ".into()),
        }]);
        assert_eq!(draft.capacity_note, None);
        assert!(!draft.findings.iter().any(|f| f.kind == "over_capacity"));
    }

    /// 开关 off 臂：计划工具的自报键按缺席处理（=旧行为）。
    #[test]
    #[serial_test::serial]
    fn capacity_note_is_ignored_when_the_switch_is_off() {
        struct Restore(Option<String>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => unsafe { std::env::set_var(EST_BUDGET_SWITCH_ENV, value) },
                    None => unsafe { std::env::remove_var(EST_BUDGET_SWITCH_ENV) },
                }
            }
        }
        let previous = std::env::var(EST_BUDGET_SWITCH_ENV).ok();
        unsafe { std::env::set_var(EST_BUDGET_SWITCH_ENV, "off") };
        let _restore = Restore(previous);

        let mut draft = LessonDraft::new(context());
        draft.apply_ops(vec![LessonOp::SetSectionManifest {
            sections: vec![
                manifest_pack("s1", SectionKind::Concept, "表格"),
                manifest_pack("s2", SectionKind::Practice, "无"),
            ],
            capacity_note: Some("超容 10 分钟".into()),
        }]);
        assert_eq!(draft.capacity_note, None, "off 臂忽略自报键");
    }

    /// 透传：inspect 投影与 to_output 报告面都带自报；旧课时包（无键）
    /// 反序列化兼容为 None。
    #[test]
    #[serial_test::serial]
    fn capacity_note_projects_to_inspect_and_output_and_old_packages_still_parse() {
        let mut draft = LessonDraft::new(context());
        draft.apply_ops(vec![LessonOp::SetSectionManifest {
            sections: vec![
                manifest_pack("s1", SectionKind::Concept, "表格"),
                manifest_pack("s2", SectionKind::Practice, "无"),
            ],
            capacity_note: Some("s1 偏长".into()),
        }]);
        assert_eq!(draft.inspect().capacity_note.as_deref(), Some("s1 偏长"));
        assert_eq!(draft.to_output().capacity_note.as_deref(), Some("s1 偏长"));

        // 旧包（无 capacity_note 键）→ None，不报错。
        let legacy: LessonOutput = serde_json::from_value(serde_json::json!({
            "summary": "## 描述\n旧正文\n",
            "estimated_minutes": 15,
            "activities": [],
            "sections": [],
            "degraded_keys": []
        }))
        .expect("旧课时包可反序列化");
        assert_eq!(legacy.capacity_note, None);
        // 未自报的草稿同样缺席。
        assert_eq!(LessonDraft::new(context()).to_output().capacity_note, None);
    }

    /// e1 段级续跑读数：inspect 视图的 section_bodies 按清单序携带已落库
    /// 节（key + 非空字符数）；缺正文的节不出现；单篇旧草稿（无清单）为
    /// 空。生成面幂等跳过与续跑开场都读这份投影（认知与执法单源）。
    #[test]
    fn inspect_view_carries_section_body_summaries_in_manifest_order() {
        let mut draft = LessonDraft::new(context());
        // 清单前不产生投影（无清单 = 单篇形态）。
        assert!(draft.inspect().section_bodies.is_empty());

        draft.apply_ops(vec![LessonOp::SetSectionManifest {
            sections: vec![
                manifest_pack("s1", SectionKind::Concept, "表格"),
                manifest_pack("s2", SectionKind::Practice, "无"),
            ],
            capacity_note: None,
        }]);
        assert!(draft.inspect().section_bodies.is_empty(), "规划后未写正文仍为空");

        draft.apply_ops(vec![
            LessonOp::SetSectionBody {
                section_key: "s2".into(),
                // 练习节下限 40 非空字符（只写能力目标与作答引导）。
                body: format!("## t-s2\n\n完成以下判断题，检验对本节要点的掌握；{}", "作答后阅读解析回顾关键区别。".repeat(2)),
            },
            LessonOp::SetSectionBody {
                section_key: "s1".into(),
                // 概念节：清单声明了「表格」visual，正文须真带表格（承诺兑现
                // 门）且剥可视化后纯文字过下限（ComplexityTier::Low 150）。
                body: format!(
                    "## t-s1\n\n| 要点 | 说明 |\n| --- | --- |\n| 甲 | 乙 |\n\n{}\n",
                    "这是一段足够长的概念讲解正文，用于通过节级质检门的文字下限。".repeat(8)
                ),
            },
        ]);
        let bodies = draft.inspect().section_bodies;
        assert_eq!(bodies.len(), 2, "两节都已落库");
        // 清单序（s1, s2），不随写入序漂移。
        assert_eq!(bodies[0].section_key, "s1");
        assert_eq!(bodies[1].section_key, "s2");
        // 非空字符口径：与节级质检长度底数一致（尾随空白不计）。
        let s1_body = format!(
            "## t-s1\n\n| 要点 | 说明 |\n| --- | --- |\n| 甲 | 乙 |\n\n{}\n",
            "这是一段足够长的概念讲解正文，用于通过节级质检门的文字下限。".repeat(8)
        );
        assert_eq!(bodies[0].chars, s1_body.chars().filter(|c| !c.is_whitespace()).count());
    }

    // ── u07：档位优先序开关 + 节长上限 warning ──────────────────────────

    use crate::LearningService;

    /// NOMI_LEARNING_* 开关 env 窗口（串行写 env——并行测试互踩的竞态教训，
    /// 与本文件既有的 est_budget 守卫同款）。drop 时恢复原值。
    fn tier_priority_env_guard(value: &'static str) -> impl Drop {
        struct Restore(Option<String>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(previous) => unsafe {
                        std::env::set_var(TIER_PRIORITY_SWITCH_ENV, previous)
                    },
                    None => unsafe { std::env::remove_var(TIER_PRIORITY_SWITCH_ENV) },
                }
            }
        }
        let previous = std::env::var(TIER_PRIORITY_SWITCH_ENV).ok();
        unsafe { std::env::set_var(TIER_PRIORITY_SWITCH_ENV, value) };
        Restore(previous)
    }

    /// 档位优先序开关（u07）：未设 = 开（**缺省 on 待 A/B**——票面推翻条件
    /// = on 臂知道档出现「只剩定义、无情境引入」空心化才翻默认 off）；
    /// off 系 = 关（条款与上限 warning 逐字节回基线）。纯函数面测试。
    #[test]
    fn tier_priority_switch_defaults_on_and_off_arm_is_explicit() {
        assert!(tier_priority_enabled_value(None), "未设 = 条款在位（缺省 on 待 A/B）");
        for value in ["off", "OFF", "0", "false", " False ", " off "] {
            assert!(!tier_priority_enabled_value(Some(value)), "{value:?} = 基线对照臂");
        }
        for value in ["on", "1", "true", "yes", ""] {
            assert!(tier_priority_enabled_value(Some(value)), "{value:?} = 条款在位");
        }
    }

    /// 节长预算锚映射（u07）：teaches 档位最高档 → 文字预算档（与
    /// prose_budget_rules 同源 ComplexityTier::prose_budget）——知道 = 低
    /// 150、会用 = 中 250、能教 = 高 400；混档取最高；无档位（传统线/零
    /// teaches）= 中档。上限 = 锚 × 2（票面口径），由消费面乘。
    #[test]
    fn tier_prose_budget_cap_maps_teaching_tier_to_budget_anchor() {
        let tiers = |rows: &[(&str, &str)]| -> Vec<(String, String)> {
            rows.iter()
                .map(|(name, tier)| ((*name).to_owned(), (*tier).to_owned()))
                .collect()
        };
        assert_eq!(
            tier_prose_budget_cap(&tiers(&[("历史波动率", "知道")])),
            ComplexityTier::Low.prose_budget(),
            "知道档 = 低档锚"
        );
        assert_eq!(
            tier_prose_budget_cap(&tiers(&[("期权定价", "会用")])),
            ComplexityTier::Mid.prose_budget(),
            "会用档 = 中档锚"
        );
        assert_eq!(
            tier_prose_budget_cap(&tiers(&[("希腊字母", "能教")])),
            ComplexityTier::High.prose_budget(),
            "能教档 = 高档锚"
        );
        assert_eq!(
            tier_prose_budget_cap(&tiers(&[("历史波动率", "知道"), ("希腊字母", "能教")])),
            ComplexityTier::High.prose_budget(),
            "混档取最高（深度上限跟着最深概念走）"
        );
        assert_eq!(
            tier_prose_budget_cap(&[]),
            ComplexityTier::Mid.prose_budget(),
            "无档位（传统线/零 teaches）= 中档锚"
        );
        assert_eq!(
            tier_prose_budget_cap(&tiers(&[("旧档节点", "?")])),
            ComplexityTier::Mid.prose_budget(),
            "无法识别的档位标签落中档（不猜测）"
        );
    }

    /// 知道档概念节：带表格（兑现 visual 承诺）+ prose 超过低档锚 2 倍。
    fn tier_over_budget_body() -> String {
        format!(
            "## t-s1\n\n| 概念 | 直观 |\n| --- | --- |\n| 波动率 | 离散度 |\n\n{}",
            "认识性讲解的正文段落，不引入逐步实算的推导，只建立直观。".repeat(14)
        )
    }

    /// 知道档概念节：prose 低于锚 2 倍但高于下限（150）——不触发 warning。
    fn tier_within_budget_body() -> String {
        format!(
            "## t-s1\n\n| 概念 | 直观 |\n| --- | --- |\n| 波动率 | 离散度 |\n\n{}",
            "认识性讲解的正文段落，不引入逐步实算的推导，只建立直观。".repeat(6)
        )
    }

    /// u07 节长上限 warning（on 臂）：单节 prose 超过预算锚 2 倍 →
    /// `section_over_budget`（warning 不阻断——发布照常成功，正优化协议
    /// 第 3 条：新检查只走 warning 留痕）；锚按 teaches 档位映射（知道 =
    /// 低档 150 → 上限 300）；预算内不报；off 臂零该 finding（基线逐字节）。
    #[tokio::test]
    #[serial_test::serial]
    async fn audit_warns_section_over_budget_only_on_tier_priority_arm() {
        let database = nomifun_db::init_database_memory().await.unwrap();
        let service = LearningService::new(database.pool().clone());

        // 知道档节点（低档锚 150 → 上限 300）。
        let mut ctx = context();
        ctx.graph = Some(GraphLessonContext {
            goal: "g".into(),
            degree_statement: "d".into(),
            prerequisite_path: String::new(),
            upcoming_nodes: String::new(),
            concept_faces: None,
            est_minutes: Some(20),
            known_names: None,
            teaches_tiers: vec![("历史波动率".to_owned(), "知道".to_owned())],
            precedents: Vec::new(),
            intent: None,
            scope: None,
        });

        // on 臂 + 超锚：warning 在案（severity warning、kind 点名、message
        // 带节 key 与锚口径），且不阻断（无 danger 新增）。
        let _env = tier_priority_env_guard("on");
        let view = service.create_lesson_draft(ctx.clone()).unwrap();
        service
            .patch_lesson_draft(
                &view.draft_id,
                vec![
                    LessonOp::SetSectionManifest {
                        sections: vec![
                            manifest_pack("s1", SectionKind::Concept, "表格"),
                            manifest_pack("s2", SectionKind::Practice, "无"),
                        ],
                        capacity_note: None,
                    },
                    LessonOp::SetSectionBody {
                        section_key: "s1".into(),
                        body: tier_over_budget_body(),
                    },
                ],
            )
            .unwrap();
        let findings = service.inspect_lesson_draft(&view.draft_id).unwrap().findings;
        let hits: Vec<_> = findings
            .iter()
            .filter(|finding| finding.kind == "section_over_budget")
            .collect();
        assert_eq!(hits.len(), 1, "超锚 warning 在案：{findings:?}");
        assert_eq!(hits[0].severity, SEV_WARNING);
        assert!(hits[0].message.contains("s1"), "{hits:?}");
        let anchor = ComplexityTier::Low.prose_budget();
        assert!(
            hits[0].message.contains(&(anchor * 2).to_string()),
            "message 带上限口径（锚 {anchor} × 2）：{hits:?}"
        );
        // 超锚只产 warning：无任何 danger 级新键（不阻断面——既有 danger
        // 全来自未完成的草稿本身，与本检查无关）。
        assert!(
            !findings
                .iter()
                .any(|finding| finding.severity == SEV_DANGER
                    && finding.kind.contains("budget")),
            "超锚不得产生 danger（正优化协议第 3 条）：{findings:?}"
        );

        // on 臂 + 预算内：零该 finding（下限门照旧独立工作）。
        let within = service.create_lesson_draft(ctx.clone()).unwrap();
        service
            .patch_lesson_draft(
                &within.draft_id,
                vec![
                    LessonOp::SetSectionManifest {
                        sections: vec![
                            manifest_pack("s1", SectionKind::Concept, "表格"),
                            manifest_pack("s2", SectionKind::Practice, "无"),
                        ],
                        capacity_note: None,
                    },
                    LessonOp::SetSectionBody {
                        section_key: "s1".into(),
                        body: tier_within_budget_body(),
                    },
                ],
            )
            .unwrap();
        let findings = service.inspect_lesson_draft(&within.draft_id).unwrap().findings;
        assert!(
            !findings.iter().any(|finding| finding.kind == "section_over_budget"),
            "预算内零 warning：{findings:?}"
        );

        // off 臂（基线）：同一超锚正文零该 finding（逐字节回基线行为）。
        drop(_env);
        let _env = tier_priority_env_guard("off");
        let off = service.create_lesson_draft(ctx).unwrap();
        service
            .patch_lesson_draft(
                &off.draft_id,
                vec![
                    LessonOp::SetSectionManifest {
                        sections: vec![
                            manifest_pack("s1", SectionKind::Concept, "表格"),
                            manifest_pack("s2", SectionKind::Practice, "无"),
                        ],
                        capacity_note: None,
                    },
                    LessonOp::SetSectionBody {
                        section_key: "s1".into(),
                        body: tier_over_budget_body(),
                    },
                ],
            )
            .unwrap();
        let findings = service.inspect_lesson_draft(&off.draft_id).unwrap().findings;
        assert!(
            !findings.iter().any(|finding| finding.kind == "section_over_budget"),
            "off 臂零该 finding（基线行为）：{findings:?}"
        );
    }
}
