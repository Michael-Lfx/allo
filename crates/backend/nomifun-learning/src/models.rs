use nomifun_common::{
    AppError, KnowledgeBaseId, LearningActivityId, LearningAttemptId, LearningConceptId,
    LearningCourseId, LearningEnrollmentId, LearningLessonId, LearningModuleId,
    LearningReviewItemId, ProviderId, TimestampMs,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct CoursePack {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_domain")]
    pub domain: String,
    #[serde(default)]
    pub source_kb_id: Option<KnowledgeBaseId>,
    #[serde(default = "default_version")]
    pub version: i64,
    #[serde(default)]
    pub concepts: Vec<ConceptPack>,
    pub modules: Vec<ModulePack>,
    /// 讲解风格（课程级，ADR-0002）：决定节写作提示词变体；缺省 standard。
    #[serde(default)]
    pub teaching_style: TeachingStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateCourseRequest {
    /// Knowledge base to ground the course in (kb flow). Exactly one of
    /// `knowledge_base_id` / `description` must be provided.
    #[serde(default)]
    pub knowledge_base_id: Option<KnowledgeBaseId>,
    /// Free-text course brief (description flow): the course is generated
    /// from this briefing alone — no knowledge base is involved, sampled
    /// sources stay empty and lessons carry no `source` span.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub provider_id: Option<ProviderId>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub mode: CourseGenerationMode,
    /// 课程类型（beta）：`learning_graph` 走学习图生成（描述即学习目标），
    /// 缺省为传统课程。
    #[serde(default)]
    pub course_kind: CourseKind,
    /// 讲解风格（standard/socratic/feynman）；缺省 standard。
    #[serde(default)]
    pub teaching_style: Option<TeachingStyle>,
}

/// 续建学习图生成的请求体：全部字段可选——模型缺省走默认解析，草稿由
/// 服务端按「最近活跃」自行定位（草稿仅在内存存活，TTL 1 小时）。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ResumeLearningGraphRequest {
    #[serde(default)]
    pub provider_id: Option<ProviderId>,
    #[serde(default)]
    pub model: Option<String>,
}

/// 课程生成状态（后台指示条/取消入口的数据源，学习图与大纲流共用）。生成
/// 在 HTTP 请求内同步执行，但创建对话框可以随时关闭——注册表让运行对外
/// 可发现、可取消。
#[derive(Debug, Serialize)]
pub struct LearningGraphGenerationStatus {
    pub running: bool,
    pub topic: Option<String>,
    pub elapsed_secs: Option<u64>,
}

impl GenerateCourseRequest {
    /// Shared request validation for the synchronous generate endpoint and
    /// the agent tool sink: model fields come as a pair and exactly one of
    /// the two generation sources is chosen.
    pub fn validate(&self) -> Result<(), AppError> {
        if self.provider_id.is_some() != self.model.is_some() {
            return Err(AppError::BadRequest(
                "provider_id and model must be provided together".into(),
            ));
        }
        if self
            .model
            .as_deref()
            .is_some_and(|model| model.trim().is_empty())
        {
            return Err(AppError::BadRequest("model must not be empty".into()));
        }
        if self.course_kind == CourseKind::LearningGraph {
            // 学习图课程只走描述流：描述即学习目标，知识库采样与
            // 模块/课时数都不参与生成。
            if self.knowledge_base_id.is_some() {
                return Err(AppError::BadRequest(
                    "learning graph courses ground in the description only".into(),
                ));
            }
            let Some(description) = &self.description else {
                return Err(AppError::BadRequest(
                    "learning graph courses require a description (the learning goal)".into(),
                ));
            };
            if description.trim().is_empty() {
                return Err(AppError::BadRequest("description must not be empty".into()));
            }
            return Ok(());
        }
        if self.knowledge_base_id.is_some() == self.description.is_some() {
            return Err(AppError::BadRequest(
                "exactly one of knowledge_base_id or description must be provided".into(),
            ));
        }
        if let Some(description) = &self.description {
            if description.trim().is_empty() {
                return Err(AppError::BadRequest("description must not be empty".into()));
            }
        }
        Ok(())
    }
}

/// Course generation strategy. Only `on_demand` exists today: the outline is
/// imported immediately and each lesson's body and activities are generated
/// only when the learner opens it. The `mode` field and the
/// `learning_course_jobs.generation_mode` column are kept as extension points
/// for future generation strategies (e.g. a learning-graph-driven mode); any
/// other value is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CourseGenerationMode {
    #[default]
    OnDemand,
}

impl CourseGenerationMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OnDemand => "on_demand",
        }
    }
}

impl TryFrom<&str> for CourseGenerationMode {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "on_demand" => Ok(Self::OnDemand),
            other => Err(format!("unsupported course generation mode: {other}")),
        }
    }
}

/// On-demand lesson content generation: optional model preference, mirroring
/// the reflection-grading request. Both fields are sent together (or neither);
/// when absent the backend falls back to its default completer.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct GenerateLessonRequest {
    #[serde(default)]
    pub provider_id: Option<ProviderId>,
    #[serde(default)]
    pub model: Option<String>,
}

/// A lesson figure that failed to render, sent back for AI repair. `language`
/// is the fence language (`svg` or `jsxgraph`), `error` the renderer error
/// message, `code` the original figure source.
#[derive(Debug, Clone, Deserialize)]
pub struct RepairFigureRequest {
    pub language: String,
    pub code: String,
    pub error: String,
}

/// Corrected figure body returned by the repair call, rendered in place of
/// the broken one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairFigureResponse {
    pub code: String,
}

fn default_domain() -> String {
    "general".to_string()
}

const fn default_version() -> i64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptPack {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub prerequisites: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModulePack {
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub lessons: Vec<LessonPack>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LessonPack {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub purpose: String,
    #[serde(default = "default_estimated_minutes")]
    pub estimated_minutes: i64,
    #[serde(default)]
    pub source: Option<SourceSpan>,
    #[serde(default)]
    pub concepts: Vec<String>,
    #[serde(default)]
    pub activities: Vec<ActivityPack>,
    /// 分节正文（ADR-0002）。缺省为空 = 旧导入课程无分节，读取端回退整篇
    /// summary 渲染（双读）。
    #[serde(default)]
    pub sections: Vec<SectionPack>,
}

/// 课时内部节段类型（ADR-0002 首期 5 种；交互节暂缓，见 ADR 妥协清单）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    /// 概念：只讲一个知识点——动机融进行文，定义 → 最小示例。
    Concept,
    /// 例题：完整 worked example——题目 → 分步解答 → 参考答案。
    Example,
    /// 演示：可视化承载主要信息（svg/jsxgraph/mermaid/KaTeX），文字只作旁注。
    Demo,
    /// 小结：要点回顾与易错点清单。
    Summary,
    /// 练习：题组承载——正文只写能力目标与作答引导，不写题。
    Practice,
}

impl SectionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Concept => "concept",
            Self::Example => "example",
            Self::Demo => "demo",
            Self::Summary => "summary",
            Self::Practice => "practice",
        }
    }

    /// 提示词里的中文类型名（节标题前缀与类型菜单用）。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Concept => "概念",
            Self::Example => "例题",
            Self::Demo => "演示",
            Self::Summary => "小结",
            Self::Practice => "练习",
        }
    }

    /// 该节型的正文下限（生成质检门）。可视化为主的三节型（概念/例题/
    /// 演示）与文字预算同口径同源：下限取低档预算（`ComplexityTier::
    /// prose_budget` 的最小值）——任何档位的预算都不低于它，照预算写就
    /// 必然过门，且计量口径一致（剥可视化块，见 `SectionPack::
    /// validate_body`）。数值禁止在此之外另写一份。
    pub const fn min_body_chars(self) -> usize {
        match self {
            Self::Concept | Self::Example | Self::Demo => ComplexityTier::Low.prose_budget(),
            Self::Summary => 120,
            // 练习节只写能力目标与作答引导（PRACTICE_BODY_TARGET_CHARS 字
            // 目标），下限从宽。
            Self::Practice => 40,
        }
    }

    /// 该节是否承载题组（出题时按内容节配题，练习节本身不配）。
    pub const fn is_content(self) -> bool {
        !matches!(self, Self::Practice)
    }
}

impl TryFrom<&str> for SectionKind {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "concept" => Ok(Self::Concept),
            "example" => Ok(Self::Example),
            "demo" => Ok(Self::Demo),
            "summary" => Ok(Self::Summary),
            "practice" => Ok(Self::Practice),
            other => Err(format!("unsupported section kind: {other}")),
        }
    }
}

/// 节清单条目：大纲阶段规划、逐节生成进度的持久事实源（对齐 learnhub
/// SectionManifest；status/version 持久在节表，不进生成载荷）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionPack {
    /// 节内稳定键（s1、s2……题目经 section_key 绑定来源节）。
    pub section_key: String,
    pub kind: SectionKind,
    pub title: String,
    /// 大纲要点（一句话）；逐节生成时锚定内容，防跑偏。
    #[serde(default)]
    pub points: String,
    /// 大纲规划的本节讲解主体可视化（公式/函数图/示意图/流程图/图表/
    /// 表格/无 之一）——正文质检门按它做承诺兑现检查；几乎每节都有，
    /// 确无（纯推理/论述节）才声明「无」。
    #[serde(default)]
    pub visual: String,
    /// 节正文（Markdown）。大纲阶段为空。
    #[serde(default)]
    pub body_md: String,
}

impl SectionPack {
    /// 节级质检门：长度下限、节内禁 ### 子标题、演示节必须有可视化块、
    /// 练习节不写题（超 250 字提示性拦截）。失败信息即修复轮的定位输入。
    pub fn validate_body(&self) -> Result<(), String> {
        let body = self.body_md.trim();
        if body.is_empty() {
            return Err(format!("section {} ({}) body is empty", self.section_key, self.title));
        }
        // 可视化为主的三节型按文字预算口径计量（剥可视化块）——提示词按
        // 档位预算写、门按下限查，同一把尺子；小结/练习无可视化承诺，按
        // 全文字符计量。
        let chars = body.chars().filter(|c| !c.is_whitespace()).count();
        let counted = if matches!(
            self.kind,
            SectionKind::Concept | SectionKind::Example | SectionKind::Demo
        ) {
            prose_char_count(body)
        } else {
            chars
        };
        let min = self.kind.min_body_chars();
        if counted < min {
            return Err(format!(
                "section {} ({}) has {counted} body characters (visualization blocks \
                 excluded for visual-first kinds), expected at least {min}",
                self.section_key, self.title
            ));
        }
        if body
            .lines()
            .any(|line| line.trim_start().starts_with("###"))
        {
            return Err(format!(
                "section {} ({}) must not contain ### sub-headings — one section teaches one unit",
                self.section_key, self.title
            ));
        }
        // 纯文字字数(剥可视化块后):learnhub 的文字预算纪律——公式与图表
        // 不占文字预算;>2000 硬拦(>600 的软引导走提示词档位额度)。
        let prose_chars = prose_char_count(body);
        if prose_chars > 2000 {
            return Err(format!(
                "section {} ({}) has {prose_chars} prose characters (visualization blocks \
                 excluded): the hard cap is 2000 — tighten the prose or split the section",
                self.section_key, self.title
            ));
        }
        let has_visual = body.contains("```svg")
            || body.contains("```jsxgraph")
            || body.contains("```mermaid")
            || body.contains("$$")
            || body.contains("| ---")
            || body.contains("| --- ");
        if self.kind == SectionKind::Demo && !has_visual {
            return Err(format!(
                "section {} ({}) is a demo: it must carry its message in a visualization \
                 block (```svg / ```jsxgraph / ```mermaid / $$math$$), not prose",
                self.section_key, self.title
            ));
        }
        // 承诺兑现制:概念/例题节按大纲声明的 visual 检查交付——声明了什么
        // 载体,正文就必须真的用它承载核心讲解;声明「无」则纯文字合法
        // (learnhub:「几乎每节都有,确无才写无;纯推理/论述节可无」)。
        if matches!(self.kind, SectionKind::Concept | SectionKind::Example) {
            if let Some(missing) = undelivered_visual(&self.visual, has_visual, body) {
                return Err(format!(
                    "section {} ({}) planned visual 「{}」 is not delivered: {}",
                    self.section_key, self.title, self.visual.trim(), missing
                ));
            }
        }
        if self.kind == SectionKind::Practice && chars > 250 {
            return Err(format!(
                "section {} ({}) is a practice section: write only the capability goal and \
                 answering guidance (≤{PRACTICE_BODY_TARGET_CHARS} characters target); the \
                 questions come from the bank",
                self.section_key, self.title
            ));
        }
        Ok(())
    }
}

/// 承诺兑现检查:Some(说明) = 声明的可视化形态未在正文中交付。
/// 空 visual(历史/异常路径)按保守处理:要求任意可视化块。
fn undelivered_visual(visual: &str, has_visual: bool, body: &str) -> Option<String> {
    let visual = visual.trim();
    match visual {
        "无" => None,
        "公式" => {
            if body.contains("$$") {
                None
            } else {
                Some("expected a $$…$$ display formula".into())
            }
        }
        "函数图" | "示意图" => {
            if body.contains("```svg") || body.contains("```jsxgraph") {
                None
            } else {
                Some("expected a ```svg or ```jsxgraph figure".into())
            }
        }
        "流程图" => {
            if body.contains("```mermaid") {
                None
            } else {
                Some("expected a ```mermaid diagram".into())
            }
        }
        "图表" | "表格" => {
            if body.contains("| ---") || body.contains("| --- ") {
                None
            } else {
                Some("expected a Markdown comparison table".into())
            }
        }
        _ => {
            if has_visual {
                None
            } else {
                Some("no visual declared and no visualization block found — declare the \
                      planned visual in the outline (公式/函数图/示意图/流程图/图表/表格/无)"
                    .into())
            }
        }
    }
}

/// 剥离可视化块(svg/jsxgraph/mermaid 围栏、$$ 展示公式、表格行)后的
/// 纯文字字符数——文字预算的计量口径。
pub(crate) fn prose_char_count(body: &str) -> usize {
    let mut count = 0usize;
    let mut in_visual_fence = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            let language = trimmed[3..].trim();
            if in_visual_fence {
                in_visual_fence = false;
            } else if matches!(language, "svg" | "jsxgraph" | "mermaid") {
                in_visual_fence = true;
            }
            continue;
        }
        if in_visual_fence {
            continue;
        }
        if trimmed.starts_with('|') || trimmed.starts_with("$$") || trimmed.contains("$$") {
            continue;
        }
        count += trimmed.chars().filter(|c| !c.is_whitespace()).count();
    }
    count
}

/// 节清单的 JSON 形状：`{"tier": "...", "sections": [...]}`，大纲阶段
/// 一次调用产出。tier 只在生成期使用，不落节表。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionOutline {
    /// 该课时自声明的复杂度档位（low/mid/high）。
    #[serde(default)]
    pub tier: Option<ComplexityTier>,
    pub sections: Vec<SectionPack>,
}

/// 复杂度档位（对齐 learnhub：难度/bloom/前置规模折叠出的内容规模级别）。
/// 锚点沿用 learnhub——低 [1,3] 节 / 中 [3,5] / 高 [4,6]，上限 8；
/// 每内容节题量低 2 / 中 3 / 高 4。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComplexityTier {
    Low,
    Mid,
    High,
}

impl ComplexityTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Mid => "mid",
            Self::High => "high",
        }
    }

    /// 提示词里的中文档位名（配比规则的渲染用）。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Low => "低",
            Self::Mid => "中",
            Self::High => "高",
        }
    }

    /// 节数区间（含端点）。
    pub const fn section_range(self) -> (usize, usize) {
        match self {
            Self::Low => (1, 3),
            Self::Mid => (3, 5),
            Self::High => (4, 6),
        }
    }

    /// 每内容节的出题数。
    pub const fn questions_per_section(self) -> usize {
        match self {
            Self::Low => 2,
            Self::Mid => 3,
            Self::High => 4,
        }
    }

    /// 每节正文的文字预算（纯文字，公式/图表/可视化块不占）——learnhub
    /// 复杂度档案 §9 的字数额度；文字纪律靠它才能硬起来。
    pub const fn prose_budget(self) -> usize {
        match self {
            Self::Low => 150,
            Self::Mid => 250,
            Self::High => 400,
        }
    }
}

impl std::fmt::Display for ComplexityTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<&str> for ComplexityTier {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "low" => Ok(Self::Low),
            "mid" => Ok(Self::Mid),
            "high" => Ok(Self::High),
            other => Err(format!("unsupported complexity tier: {other}")),
        }
    }
}

/// ── 内容契约：常量与提示词渲染 ───────────────────────────────────────
/// 节配比、visual 菜单、文字预算在提示词与工具描述里的一切拼写都必须由
/// 本节渲染函数从 owner 表格生成（ADR-0002 追加决策：契约数值唯一事实
/// 源）——改数值只改表格；新增拼写点必须调用这里，不允许手写字面量。

/// 节清单硬上限（大纲与清单护栏、提示词配比渲染共用）。
pub(crate) const SECTION_OUTLINE_CAP: usize = 8;

/// 可视化声明菜单：大纲/清单 visual 字段的全部取值（几乎每节都有，确无
/// 才写「无」）。两处质检门、提示词与工具 schema 枚举共用。
pub const VISUAL_OPTIONS: [&str; 7] = ["公式", "函数图", "示意图", "流程图", "图表", "表格", "无"];

/// 练习节正文的作答引导目标字数（提示词口径；质检门硬拦 250）。
pub const PRACTICE_BODY_TARGET_CHARS: usize = 120;

/// 节数配比规则的渲染（大纲提示词与 ls_set_section_manifest 工具描述共
/// 用）：`低 1-3 节、中 3-5、高 4-6，硬上限 8`。
pub fn section_range_rules() -> String {
    let low = ComplexityTier::Low.section_range();
    let mid = ComplexityTier::Mid.section_range();
    let high = ComplexityTier::High.section_range();
    format!(
        "{} {}-{} 节、{} {}-{}、{} {}-{}，硬上限 {}",
        ComplexityTier::Low.label(),
        low.0,
        low.1,
        ComplexityTier::Mid.label(),
        mid.0,
        mid.1,
        ComplexityTier::High.label(),
        high.0,
        high.1,
        SECTION_OUTLINE_CAP,
    )
}

/// visual 声明菜单的渲染：`公式 / 函数图 / 示意图 / 流程图 / 图表 / 表格 / 无`。
pub fn visual_menu_text() -> String {
    VISUAL_OPTIONS.join(" / ")
}

/// 文字预算规则的渲染（逐节正文提示词与 ls_set_section_body 工具描述共
/// 用）：档位预算表即事实源，质检门下限同源（低档预算）。
pub fn prose_budget_rules() -> String {
    format!(
        "文字预算按课时复杂度档位（仅正文，公式/图表/可视化块不占）：{} {} 字、{} {} 字、{} {} 字；质检门下限 {} 字（同一纯文字口径，低于即拒）",
        ComplexityTier::Low.label(),
        ComplexityTier::Low.prose_budget(),
        ComplexityTier::Mid.label(),
        ComplexityTier::Mid.prose_budget(),
        ComplexityTier::High.label(),
        ComplexityTier::High.prose_budget(),
        SectionKind::Concept.min_body_chars(),
    )
}

/// 节清单护栏：只拦方向性极端（learnhub checkOutlineBudget 的对齐）。
/// 低档 >7 节、高档 ≤2 节、任意 >8 节都判定大纲跑偏，回灌反馈重跑一次。
pub(crate) fn validate_section_outline(outline: &SectionOutline) -> Result<(), String> {
    let count = outline.sections.len();
    if count == 0 {
        return Err("section outline is empty: plan at least one section".into());
    }
    if count > SECTION_OUTLINE_CAP {
        return Err(format!(
            "section outline plans {count} sections, the hard cap is {SECTION_OUTLINE_CAP}"
        ));
    }
    if let Some(tier) = outline.tier {
        // 对齐 learnhub checkOutlineBudget:只拦方向性极端,不掐精确区间。
        let (min, max) = tier.section_range();
        if tier == ComplexityTier::Low && count > 7 {
            return Err(format!(
                "tier {tier} anchors {min}-{max} sections but the outline plans {count} — \
                 either raise the tier or split the lesson differently"
            ));
        }
        if tier == ComplexityTier::High && count <= 2 {
            return Err(format!(
                "tier high anchors 4-6 sections but the outline plans only {count}"
            ));
        }
    }
    let mut seen = std::collections::HashSet::new();
    for section in &outline.sections {
        if section.title.trim().is_empty() {
            return Err(format!("section {} has an empty title", section.section_key));
        }
        if !seen.insert(section.section_key.as_str()) {
            return Err(format!("duplicate section key {}", section.section_key));
        }
        // 可视化前置规划(learnhub):几乎每节都有,确无才写「无」。正文
        // 质检门按声明做承诺兑现检查。
        if matches!(
            section.kind,
            SectionKind::Concept | SectionKind::Example | SectionKind::Demo
        ) && !VISUAL_OPTIONS.contains(&section.visual.trim())
        {
            return Err(format!(
                "section {} ({}) must declare its planned visual, one of: {}",
                section.section_key,
                section.kind.label(),
                visual_menu_text()
            ));
        }
    }
    // 收尾练习是硬性结构:恰好一个练习节,且必须是最后一节——学习者读完
    // 即进入统一的练习轮(题目全部挂在这一轮作答)。
    let practice_count = outline
        .sections
        .iter()
        .filter(|section| section.kind == SectionKind::Practice)
        .count();
    if practice_count != 1 {
        return Err(format!(
            "the outline must close with exactly one practice section, found {practice_count}"
        ));
    }
    if outline.sections.last().is_some_and(|last| last.kind != SectionKind::Practice) {
        return Err("the last section must be the practice section".into());
    }
    Ok(())
}

/// 讲解风格（课程级选择，课时生成时决定节写作提示词变体）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TeachingStyle {
    #[default]
    Standard,
    Socratic,
    Feynman,
}

impl TeachingStyle {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Socratic => "socratic",
            Self::Feynman => "feynman",
        }
    }
}

impl TryFrom<&str> for TeachingStyle {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "standard" => Ok(Self::Standard),
            "socratic" => Ok(Self::Socratic),
            "feynman" => Ok(Self::Feynman),
            other => Err(format!("unsupported teaching style: {other}")),
        }
    }
}

const fn default_estimated_minutes() -> i64 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSpan {
    pub path: String,
    #[serde(default)]
    pub start: Option<i64>,
    #[serde(default)]
    pub end: Option<i64>,
}

/// Serde helper: tolerate an explicit `null` where a string is expected by
/// degrading to an empty string. `#[serde(default)]` only covers a *missing*
/// field, while LLM outputs often write `"field": null` — which would
/// otherwise fail the whole parse ("invalid type: null, expected a string").
/// Degraded values then flow through the same validation as any other weak
/// output, so structural mistakes still trigger the targeted retry.
pub fn de_string_or_empty<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

/// Serde helper: tolerate `null` in place of a string list (or `null`
/// elements inside it) by degrading to an empty list. Same rationale as
/// [`de_string_or_empty`]; non-string elements still fail loudly.
pub fn de_vec_string_or_empty<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<Vec<Option<String>>>::deserialize(deserializer)?
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityPack {
    pub kind: ActivityKind,
    #[serde(default, deserialize_with = "de_string_or_empty")]
    pub prompt: String,
    #[serde(default, deserialize_with = "de_vec_string_or_empty")]
    pub options: Vec<String>,
    #[serde(default)]
    pub answer: Value,
    #[serde(default, deserialize_with = "de_string_or_empty")]
    pub explanation: String,
    #[serde(default, deserialize_with = "de_vec_string_or_empty")]
    pub concepts: Vec<String>,
    /// Near-synonym traps for fill_in_blank blanks (or physically adjacent
    /// quantities), forcing fine discrimination. Only fill_in_blank uses it.
    #[serde(default, deserialize_with = "de_vec_string_or_empty")]
    pub distractors: Vec<String>,
    /// Accepted deviation for numeric answers (|response − answer| ≤ tol).
    /// Only numeric uses it; absent means exact match (floats compared with
    /// a tiny epsilon).
    #[serde(default)]
    pub tol: Option<f64>,
    /// 来源节的 section_key（如 s2）。None = 跨节综合题（通用）。
    #[serde(default)]
    pub section_key: Option<String>,
    /// 难度分档 1-3（1 概念辨析 / 2 应用 / 3 综合陷阱），出题阶段声明。
    #[serde(default)]
    pub difficulty: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    SingleChoice,
    TrueFalse,
    Reflection,
    FillInBlank,
    /// 多选：answer 是选项字符串数组，顺序无关。
    MultiChoice,
    /// 数值题：answer 是 JSON number，`tol` 为可接受容差（缺省 0）。
    Numeric,
    /// 排序题：options 是打乱后的条目，answer 是正确顺序的条目数组。
    Ordering,
    /// 匹配题：options 是左列条目，answer 是与 options 一一对应的右列值数组。
    Matching,
    /// 开放题：answer 为 null，走 AI 批改（0-10 分制，≥6 及格）。
    OpenQuestion,
}

impl ActivityKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SingleChoice => "single_choice",
            Self::TrueFalse => "true_false",
            Self::Reflection => "reflection",
            Self::FillInBlank => "fill_in_blank",
            Self::MultiChoice => "multi_choice",
            Self::Numeric => "numeric",
            Self::Ordering => "ordering",
            Self::Matching => "matching",
            Self::OpenQuestion => "open_question",
        }
    }

    /// 规则判卷的客观题集合：作答即得分，可进复习队列。reflection 与
    /// open_question 走 AI 批改，不在此列。
    pub const fn is_objective(self) -> bool {
        matches!(
            self,
            Self::SingleChoice
                | Self::TrueFalse
                | Self::FillInBlank
                | Self::MultiChoice
                | Self::Numeric
                | Self::Ordering
                | Self::Matching
        )
    }

    /// AI 批改的题型（answer 为 null，空回答被拒绝）。
    pub const fn is_ai_graded(self) -> bool {
        matches!(self, Self::Reflection | Self::OpenQuestion)
    }
}

impl ActivityPack {
    /// Per-kind shape validation shared by every entry point (generation,
    /// single-addition, draft audit, manual authoring, course import). The
    /// only caller-specific rule is the options range for choice-like kinds:
    /// generation demands 3-5 options while manual authoring accepts 2-5.
    pub(crate) fn validate_shape(
        &self,
        options_bounds: (usize, usize),
        require_distractors: bool,
    ) -> Result<(), String> {
        let (min_options, max_options) = options_bounds;
        let distinct_options = {
            let mut seen = std::collections::HashSet::new();
            self.options.iter().all(|option| {
                !option.trim().is_empty() && seen.insert(option.trim().to_lowercase())
            })
        };
        match self.kind {
            ActivityKind::SingleChoice => {
                if !(min_options..=max_options).contains(&self.options.len()) || !distinct_options
                {
                    return Err(format!(
                        "single_choice \"{}\" needs {min_options}-{max_options} distinct non-empty options, got {}",
                        self.prompt,
                        self.options.len()
                    ));
                }
                let Some(answer) = self.answer.as_str() else {
                    return Err(format!(
                        "single_choice \"{}\" answer must be a string",
                        self.prompt
                    ));
                };
                if !self.options.iter().any(|option| option == answer) {
                    return Err(format!(
                        "single_choice \"{}\" answer does not match any option",
                        self.prompt
                    ));
                }
            }
            ActivityKind::TrueFalse => {
                if !self.answer.is_boolean() {
                    return Err(format!(
                        "true_false \"{}\" answer must be a boolean",
                        self.prompt
                    ));
                }
            }
            ActivityKind::Reflection | ActivityKind::OpenQuestion => {
                if !self.answer.is_null() {
                    return Err(format!(
                        "{} \"{}\" answer must be null",
                        self.kind.as_str(),
                        self.prompt
                    ));
                }
            }
            ActivityKind::FillInBlank => {
                if !self.prompt.contains("___") {
                    return Err(format!(
                        "fill_in_blank \"{}\" prompt must contain a ___ blank",
                        self.prompt
                    ));
                }
                let Some(answers) = self.answer.as_array() else {
                    return Err(format!(
                        "fill_in_blank \"{}\" answer must be a JSON array of accepted answers",
                        self.prompt
                    ));
                };
                if answers.is_empty() || answers.len() > 3 {
                    return Err(format!(
                        "fill_in_blank \"{}\" must have 1-3 accepted answers",
                        self.prompt
                    ));
                }
                if answers.iter().any(|accepted| {
                    !accepted.as_str().is_some_and(|text| !text.trim().is_empty())
                }) {
                    return Err(format!(
                        "fill_in_blank \"{}\" accepted answers must be non-empty strings",
                        self.prompt
                    ));
                }
                if require_distractors
                    && self
                        .distractors
                        .iter()
                        .all(|distractor| distractor.trim().is_empty())
                {
                    return Err(format!(
                        "fill_in_blank \"{}\" must provide at least one near-synonym distractor",
                        self.prompt
                    ));
                }
            }
            ActivityKind::MultiChoice => {
                if self.options.len() < min_options
                    || self.options.len() > max_options
                    || !distinct_options
                {
                    return Err(format!(
                        "multi_choice \"{}\" needs {min_options}-{max_options} distinct non-empty options, got {}",
                        self.prompt,
                        self.options.len()
                    ));
                }
                let Some(answers) = self.answer.as_array() else {
                    return Err(format!(
                        "multi_choice \"{}\" answer must be a JSON array of option strings",
                        self.prompt
                    ));
                };
                if answers.is_empty() {
                    return Err(format!(
                        "multi_choice \"{}\" answer must select at least one option",
                        self.prompt
                    ));
                }
                let mut picked = std::collections::HashSet::new();
                for answer in answers {
                    let Some(text) = answer.as_str() else {
                        return Err(format!(
                            "multi_choice \"{}\" answer items must be strings",
                            self.prompt
                        ));
                    };
                    if !self.options.iter().any(|option| option == text) {
                        return Err(format!(
                            "multi_choice \"{}\" answer must reference only the given options",
                            self.prompt
                        ));
                    }
                    if !picked.insert(text.to_owned()) {
                        return Err(format!(
                            "multi_choice \"{}\" answer must not repeat an option",
                            self.prompt
                        ));
                    }
                }
            }
            ActivityKind::Numeric => {
                if self.answer.as_f64().is_none() {
                    return Err(format!(
                        "numeric \"{}\" answer must be a JSON number",
                        self.prompt
                    ));
                }
                if let Some(tol) = self.tol {
                    if !tol.is_finite() || tol < 0.0 {
                        return Err(format!(
                            "numeric \"{}\" tol must be a non-negative finite number",
                            self.prompt
                        ));
                    }
                }
            }
            ActivityKind::Ordering => {
                if self.options.len() < 2 || !distinct_options {
                    return Err(format!(
                        "ordering \"{}\" needs at least 2 distinct non-empty items",
                        self.prompt
                    ));
                }
                let Some(correct_order) = self.answer.as_array() else {
                    return Err(format!(
                        "ordering \"{}\" answer must be a JSON array with the items in correct order",
                        self.prompt
                    ));
                };
                // The answer must be a permutation of the presented items.
                let mut remaining: Vec<&str> = self.options.iter().map(String::as_str).collect();
                for item in correct_order {
                    let Some(text) = item.as_str() else {
                        return Err(format!(
                            "ordering \"{}\" answer items must be strings",
                            self.prompt
                        ));
                    };
                    let Some(at) = remaining.iter().position(|candidate| *candidate == text) else {
                        return Err(format!(
                            "ordering \"{}\" answer must contain exactly the presented items",
                            self.prompt
                        ));
                    };
                    remaining.remove(at);
                }
                if !remaining.is_empty() {
                    return Err(format!(
                        "ordering \"{}\" answer must contain exactly the presented items",
                        self.prompt
                    ));
                }
            }
            ActivityKind::Matching => {
                if self.options.len() < 2 || !distinct_options {
                    return Err(format!(
                        "matching \"{}\" needs at least 2 distinct non-empty left-column items",
                        self.prompt
                    ));
                }
                let Some(right) = self.answer.as_array() else {
                    return Err(format!(
                        "matching \"{}\" answer must be a JSON array aligned with the left column",
                        self.prompt
                    ));
                };
                if right.len() != self.options.len() {
                    return Err(format!(
                        "matching \"{}\" answer needs one right-column value per left item ({} != {})",
                        self.prompt,
                        right.len(),
                        self.options.len()
                    ));
                }
                if right
                    .iter()
                    .any(|value| !value.as_str().is_some_and(|text| !text.trim().is_empty()))
                {
                    return Err(format!(
                        "matching \"{}\" right-column values must be non-empty strings",
                        self.prompt
                    ));
                }
            }
        }
        if self.prompt.trim().is_empty() {
            return Err("activity prompt is empty".into());
        }
        // 难度分档 1-3(出题模板的难度递进:概念辨析→应用→综合陷阱)。
        if let Some(difficulty) = self.difficulty {
            if !(1..=3).contains(&difficulty) {
                return Err(format!(
                    "{} \"{}\" difficulty must be 1-3, got {}",
                    self.kind.as_str(),
                    self.prompt,
                    difficulty
                ));
            }
        }
        // 选择题选项不得自带 A./B. 编号前缀(系统自动编号,前缀是高频模型
        // 手误且渲染出双编号)。
        for kind in [ActivityKind::SingleChoice, ActivityKind::MultiChoice] {
            if self.kind != kind {
                continue;
            }
            for option in &self.options {
                if has_letter_prefix(option) {
                    return Err(format!(
                        "{} \"{}\" option \"{}\" carries a letter prefix (A./B.…) — \
                         the system numbers options automatically, write bare option text",
                        kind.as_str(),
                        self.prompt,
                        option.trim()
                    ));
                }
            }
        }
        Ok(())
    }
}

/// `A.` / `b、` / `C：` 式的字母编号前缀(选项文本应裸写)。
fn has_letter_prefix(option: &str) -> bool {
    let option = option.trim_start();
    let mut chars = option.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    matches!(chars.next(), Some('.' | '、' | ':'))
}



impl TryFrom<&str> for ActivityKind {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "single_choice" => Ok(Self::SingleChoice),
            "true_false" => Ok(Self::TrueFalse),
            "reflection" => Ok(Self::Reflection),
            "fill_in_blank" => Ok(Self::FillInBlank),
            "multi_choice" => Ok(Self::MultiChoice),
            "numeric" => Ok(Self::Numeric),
            "ordering" => Ok(Self::Ordering),
            "matching" => Ok(Self::Matching),
            "open_question" => Ok(Self::OpenQuestion),
            other => Err(format!("unsupported activity kind: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LessonStatus {
    NotStarted,
    InProgress,
    Completed,
    /// 学习图节点跳过：学习者声明已掌握、跳过学习。它满足前置条件
    /// （解锁下游），但不等于 completed：不种复习项、不进推荐候选，
    /// completed_at 保持 NULL。取消跳过 = 传回 not_started。
    Skipped,
}

impl LessonStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Skipped => "skipped",
        }
    }
}

impl TryFrom<&str> for LessonStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "not_started" => Ok(Self::NotStarted),
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "skipped" => Ok(Self::Skipped),
            other => Err(format!("unsupported lesson status: {other}")),
        }
    }
}

impl LessonStatus {
    /// 该状态是否视为「已满足」：completed 与 skipped 都解锁下游节点、
    /// 不再进入推荐候选。跳过是学习图（beta）的能力，但对传统课时
    /// 同样语义自洽（声明已掌握）。
    pub const fn satisfies(self) -> bool {
        matches!(self, Self::Completed | Self::Skipped)
    }
}

/// 课程目录类型。`traditional` 为模块/课时大纲课程；`learning_graph`
/// （beta）由 AI 把宽泛学习目标拆解为前置 DAG，课程大纲被
/// 「下一步推荐学习的节点」取代。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CourseKind {
    #[default]
    Traditional,
    LearningGraph,
}

impl CourseKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Traditional => "traditional",
            Self::LearningGraph => "learning_graph",
        }
    }
}

impl TryFrom<&str> for CourseKind {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "traditional" => Ok(Self::Traditional),
            "learning_graph" => Ok(Self::LearningGraph),
            other => Err(format!("unsupported course kind: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CourseSummary {
    pub id: LearningCourseId,
    pub title: String,
    pub description: String,
    pub domain: String,
    pub course_kind: CourseKind,
    pub source_kb_id: Option<KnowledgeBaseId>,
    pub version: i64,
    pub enrolled: bool,
    pub total_lessons: i64,
    pub completed_lessons: i64,
    pub updated_at: TimestampMs,
    pub tags: Vec<String>,
}

/// 学习图课程视图的一个节点：底层课时（标题/摘要/估计分钟/生成状态）
/// 加图坐标（拓扑序 position、depth 层深）与学习者进度状态。正文永远
/// 不进全图载荷——内容经现有课时接口按需拉取。
#[derive(Debug, Clone, Serialize)]
pub struct GraphNodeView {
    pub lesson_id: LearningLessonId,
    pub title: String,
    pub summary: String,
    pub purpose: String,
    pub estimated_minutes: i64,
    pub generated: bool,
    /// 发布时的 Kahn 拓扑序（也是推荐排序键）。
    pub position: i64,
    /// 前置层深（零前置为 0），供分层渲染与宏观 LOD 使用。
    pub depth: i64,
    pub status: LessonStatus,
    pub prerequisite_count: i64,
}

/// 学习图课程视图的一条前置边：`from` 应先于 `to` 被满足（lesson_id 引用）。
#[derive(Debug, Clone, Serialize)]
pub struct GraphEdgeView {
    pub from: LearningLessonId,
    pub to: LearningLessonId,
    pub reason: String,
}

/// 学习图课程的图视图（挂在 `CourseDetail.graph` 下）：图结构的事实来源
/// 是 lessons + prerequisites 两张表，这里只做投影；`recommended` 是
/// 「下一步推荐学习的节点」（≤10，就绪集按拓扑序）。
#[derive(Debug, Clone, Serialize)]
pub struct LearningGraphView {
    /// 用户生成图时输入的学习目标。
    pub goal: String,
    /// 学习范围（scope 分析文本）。
    pub scope: String,
    pub nodes: Vec<GraphNodeView>,
    pub edges: Vec<GraphEdgeView>,
    pub recommended: Vec<LearningLessonId>,
    /// 课程行 graph_meta_json 透传（审计快照/生成留档/扩展备注）。
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CourseDetail {
    pub course: CourseSummary,
    pub enrollment_id: Option<LearningEnrollmentId>,
    pub modules: Vec<ModuleView>,
    pub concepts: Vec<ConceptView>,
    pub next_lesson_id: Option<LearningLessonId>,
    pub due_review_count: i64,
    /// 仅 `learning_graph` 课程携带：图结构 + 下一步推荐节点。
    pub graph: Option<LearningGraphView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleView {
    pub id: LearningModuleId,
    pub title: String,
    pub description: String,
    pub position: i64,
    pub lessons: Vec<LessonView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LessonView {
    pub id: LearningLessonId,
    pub title: String,
    pub summary: String,
    pub purpose: String,
    pub position: i64,
    pub estimated_minutes: i64,
    pub generated: bool,
    pub source: Option<SourceSpan>,
    pub status: LessonStatus,
    pub concepts: Vec<LearningConceptId>,
    pub activities: Vec<ActivityView>,
    /// 分节正文（ADR-0002）。空 = 旧课时的单篇 summary（双读回退）。
    pub sections: Vec<SectionView>,
}

/// API 视角的一个节段：清单字段 + 正文与生成状态。
#[derive(Debug, Clone, Serialize)]
pub struct SectionView {
    pub section_key: String,
    pub kind: SectionKind,
    pub title: String,
    pub points: String,
    /// 大纲声明的本节讲解主体可视化（承诺事实源，迁移 050 起落库）；
    /// 历史行为空串（未声明）。
    pub visual: String,
    pub body_md: String,
    /// pending | ready | failed。
    pub status: String,
    pub version: i64,
    pub position: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActivityView {
    pub id: LearningActivityId,
    pub kind: ActivityKind,
    pub prompt: String,
    pub options: Vec<String>,
    /// Matching questions only: scrambled right-column candidates.
    #[serde(default)]
    pub matches: Vec<String>,
    pub position: i64,
    pub concepts: Vec<LearningConceptId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticItem {
    pub lesson_id: LearningLessonId,
    pub lesson_title: String,
    pub activity: ActivityView,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticPlan {
    pub course_id: LearningCourseId,
    pub total_concepts: i64,
    pub items: Vec<DiagnosticItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConceptView {
    pub id: LearningConceptId,
    pub key: String,
    pub title: String,
    pub description: String,
    pub prerequisites: Vec<LearningConceptId>,
    pub mastery: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateLessonProgressRequest {
    pub status: LessonStatus,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubmitAttemptRequest {
    pub response: Value,
    /// Explicit AI model preference for reflection grading. Both fields are
    /// sent together (or neither); the backend falls back to its default
    /// completer when absent and to rule-based grading on any AI failure.
    #[serde(default)]
    pub provider_id: Option<ProviderId>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttemptResult {
    pub id: LearningAttemptId,
    pub score: f64,
    pub passed: bool,
    pub feedback: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRating {
    Again,
    Hard,
    Good,
    Easy,
}

impl ReviewRating {
    /// FSRS rating scale (1=again .. 4=easy) used by the review log.
    pub fn fsrs_value(self) -> i64 {
        match self {
            ReviewRating::Again => 1,
            ReviewRating::Hard => 2,
            ReviewRating::Good => 3,
            ReviewRating::Easy => 4,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateReviewRequest {
    pub rating: ReviewRating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewSource {
    /// Concept-bound review item coming from a course enrollment.
    Course,
    /// Learner-authored custom question with its own schedule.
    Custom,
}

#[derive(Debug, Clone, Serialize)]
pub struct DueReview {
    pub id: LearningReviewItemId,
    pub source: ReviewSource,
    pub enrollment_id: Option<LearningEnrollmentId>,
    pub course_id: Option<LearningCourseId>,
    pub course_title: Option<String>,
    pub module_title: Option<String>,
    pub lesson_title: Option<String>,
    pub concept_id: Option<LearningConceptId>,
    pub concept_title: Option<String>,
    pub question: ReviewQuestion,
    pub due_at: TimestampMs,
    pub stability_days: f64,
    pub difficulty: f64,
    pub review_count: i64,
    pub lapse_count: i64,
    /// FSRS-predicted recall probability at now (0..1); `None` for cards
    /// that never carried a memory state. Drives the queue's
    /// forgettability ranking and the card's predicted-recall disclosure.
    pub r: Option<f64>,
    /// Marked "edit me later" from the review session; the card keeps its
    /// schedule untouched and a note (optional) records the intent.
    pub edit_pending: bool,
    pub edit_note: Option<String>,
}

/// Objective question attached to a due review. Never includes the stored
/// answer; correctness is judged server-side by `answer_review`. Custom
/// questions carry no activity id.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewQuestion {
    pub activity_id: Option<LearningActivityId>,
    pub kind: ActivityKind,
    pub prompt: String,
    pub options: Vec<String>,
    /// Matching questions only: right-column candidates (order preserved).
    #[serde(default)]
    pub matches: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnswerReviewRequest {
    #[serde(default)]
    pub response: Value,
    /// The learner admits they cannot recall the answer. Skips guessing:
    /// the item is rated `again` and the correct answer is returned.
    #[serde(default)]
    pub forgot: bool,
    /// Wall-clock milliseconds from the card being shown to the answer
    /// being submitted, as measured by the review session. Stored on the
    /// attempt for future anti-guessing heuristics.
    #[serde(default)]
    pub elapsed_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewAnswerResult {
    pub correct: bool,
    pub feedback: String,
    /// Correct answer, only populated when the response was wrong.
    pub correct_answer: Option<Value>,
    /// Present when the answer was wrong and the item was automatically
    /// rated `again`; otherwise the caller rates after a correct answer.
    pub rated: Option<ReviewResult>,
    /// Whether this submission actually moved the card's schedule. `false`
    /// means the due-ness gate blocked the push (a stale repeat of a card
    /// that was already pushed today and is not due): the attempt is still
    /// recorded for accuracy/diagnostics, but scheduling and the review log
    /// are untouched.
    pub advanced: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewResult {
    /// Review item id for course reviews, custom question id otherwise.
    pub id: String,
    pub due_at: TimestampMs,
    pub stability_days: f64,
    pub difficulty: f64,
    pub review_count: i64,
    pub lapse_count: i64,
    /// Whether this rating advanced the card; `false` when the due-ness
    /// gate blocked a stale repeat (schedule and review log untouched).
    pub advanced: bool,
}

/// Daily check-in snapshot for the current review day: goal, progress and
/// due count, with the completion flag derived from a locking snapshot
/// persisted in `learning_checkins`.
#[derive(Debug, Clone, Serialize)]
pub struct CheckinStatus {
    /// Local review day as YYYYMMDD.
    pub review_day: i64,
    /// Daily review goal snapshot (0 = clear-the-queue only).
    pub goal: i64,
    /// Reviews already submitted this review day.
    pub reviewed_count: i64,
    /// Cards currently due (`due_at <= now`), course + custom.
    pub due_count: i64,
    /// Whether the day is locked as completed (either condition met).
    pub completed: bool,
    /// Lock moment in UTC milliseconds when completed, else null.
    pub locked_at: Option<i64>,
}

/// Memory-health dashboard: one snapshot over the active card pool and the
/// review log, in review-day semantics (02:00 rollover).
#[derive(Debug, Clone, Serialize)]
pub struct MemoryHealthStats {
    /// Current local review day as YYYYMMDD.
    pub review_day: i64,
    pub tz_offset: i32,
    /// Active cards whose due time has already passed.
    pub overdue_count: i64,
    /// Due-card counts for the current and the next six review days.
    pub load_forecast: Vec<MemoryLoadDay>,
    /// Active-card pool split by memory state (never pushed / stability
    /// below 7 / below 30 / 30+ days).
    pub state_distribution: Vec<MemoryStateBucket>,
    /// True Retention over counted pushes (first push per card per review
    /// day, cards that carried a memory state); `None` without samples.
    pub true_retention: Option<MemoryTrueRetention>,
    /// FSRS prediction vs actual outcome, bucketed by predicted recall in
    /// five-percentage-point bins; only bins with samples are listed.
    pub calibration: Vec<MemoryCalibrationBin>,
    /// Actual vs predicted retention per elapsed-days point since the last
    /// push; only points with samples are listed.
    pub forgetting_curve: Vec<MemoryCurvePoint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryLoadDay {
    pub review_day: i64,
    pub due_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryStateBucket {
    /// `new` | `young` | `mature` | `master`.
    pub key: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryTrueRetention {
    pub passes: i64,
    pub fails: i64,
    /// passes / (passes + fails); `None` when nothing counted yet.
    pub rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryCalibrationBin {
    /// Bin index over predicted recall, `bucket / 20 .. (bucket + 1) / 20`.
    pub bucket: i64,
    pub min: f64,
    pub max: f64,
    pub predicted: f64,
    /// Share of pushes rated pass (rating >= 2) inside the bin.
    pub actual: Option<f64>,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryCurvePoint {
    /// Anchor of the elapsed-days bin (0, 1, 2, 3, 5, 7, 14 or 30).
    pub elapsed_days: i64,
    pub predicted: f64,
    pub actual: Option<f64>,
    pub count: i64,
}

/// One lesson completed on a review day (calendar aggregation detail).
#[derive(Debug, Clone, Serialize)]
pub struct CalendarLessonRef {
    pub lesson_id: String,
    pub title: String,
}

/// One course created on a review day (calendar aggregation detail).
#[derive(Debug, Clone, Serialize)]
pub struct CalendarCourseRef {
    pub course_id: String,
    pub title: String,
}

/// One review day inside the requested calendar range, zero-filled when the
/// user had no activity. `review_day` is the local YYYYMMDD of the review day
/// (02:00 rollover), matching check-in and streak semantics.
#[derive(Debug, Clone, Serialize)]
pub struct CalendarDayStats {
    pub review_day: i64,
    pub reviewed_count: i64,
    pub checkin_completed: bool,
    /// Cards due on this review day; overdue cards roll into the current
    /// day so the today cell matches the review banner's due queue.
    pub due_count: i64,
    pub completed_lessons: Vec<CalendarLessonRef>,
    pub created_courses: Vec<CalendarCourseRef>,
}

/// Calendar aggregation for the learning page: review-day bucketed activity
/// (review counts, check-in completion, completed lessons and created
/// courses) plus the current streak.
#[derive(Debug, Clone, Serialize)]
pub struct CalendarStats {
    pub year: i64,
    /// 1..=12 for the month view, null for the year view.
    pub month: Option<i64>,
    pub tz_offset: i32,
    /// Consecutive completed check-in days ending at the current review day;
    /// 0 when today is not yet completed.
    pub streak: i64,
    pub days: Vec<CalendarDayStats>,
}

/// One row of the question management table. Course questions come from
/// objective activities linked to concepts (review item optional: items
/// only exist after the lesson is completed); custom questions are
/// learner-authored and always carry their own schedule.
#[derive(Debug, Clone, Serialize)]
pub struct QuestionEntry {
    pub source: ReviewSource,
    /// Activity id for course questions, custom question id otherwise.
    pub question_id: String,
    pub review_item_id: Option<LearningReviewItemId>,
    /// `unlearned`, `new`, `due` or `scheduled`.
    pub state: String,
    pub course_id: Option<LearningCourseId>,
    pub course_title: Option<String>,
    pub concept_id: Option<LearningConceptId>,
    pub concept_title: Option<String>,
    pub question_kind: Option<ActivityKind>,
    pub prompt: Option<String>,
    pub options: Vec<String>,
    pub answer: Option<Value>,
    /// Near-synonym traps for fill_in_blank blanks, surfaced for display.
    #[serde(default)]
    pub distractors: Vec<String>,
    pub explanation: Option<String>,
    pub due_at: Option<TimestampMs>,
    pub overdue: bool,
    pub stability_days: f64,
    pub difficulty: f64,
    pub review_count: i64,
    pub lapse_count: i64,
    pub last_reviewed_at: Option<TimestampMs>,
    pub updated_at: TimestampMs,
    pub tags: Vec<String>,
    /// Marked "edit me later" from the review session; the note (optional)
    /// records what the learner intended to change.
    pub edit_pending: bool,
    pub edit_note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateQuestionRequest {
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<String>,
    pub answer: Value,
    #[serde(default)]
    pub explanation: String,
    #[serde(default)]
    pub distractors: Vec<String>,
}

/// Marks a review card as "edit me later"; the note is optional and purely
/// for the learner to recall the intended edit.
#[derive(Debug, Clone, Deserialize)]
pub struct MarkEditRequest {
    #[serde(default)]
    pub note: Option<String>,
}

/// Learner-authored question. Objective kinds (single choice, true/false,
/// fill in the blank) are supported; the optional concept links the question
/// back to an existing concept (including orphaned concepts from deleted
/// courses).
#[derive(Debug, Clone, Deserialize)]
pub struct CreateCustomQuestionRequest {
    pub kind: ActivityKind,
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<String>,
    pub answer: Value,
    #[serde(default)]
    pub explanation: String,
    #[serde(default)]
    pub concept_id: Option<LearningConceptId>,
    /// Near-synonym traps for fill_in_blank blanks (optional when the
    /// learner authors the question by hand).
    #[serde(default)]
    pub distractors: Vec<String>,
}

/// Manually appends an activity to an existing lesson. All four kinds are
/// accepted; when `concept_ids` is empty the activity binds to every concept
/// of the lesson, matching course-generation semantics.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateLessonActivityRequest {
    pub kind: ActivityKind,
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub answer: Value,
    #[serde(default)]
    pub explanation: String,
    /// Near-synonym traps for fill_in_blank blanks; empty for other kinds.
    #[serde(default)]
    pub distractors: Vec<String>,
    #[serde(default)]
    pub concept_ids: Vec<LearningConceptId>,
}

/// Asks the knowledge-backed generator for a single activity draft for an
/// existing lesson. The draft is returned for preview and never persisted.
#[derive(Debug, Clone, Deserialize)]
pub struct GenerateLessonActivityRequest {
    pub kind: ActivityKind,
    #[serde(default)]
    pub provider_id: Option<ProviderId>,
    #[serde(default)]
    pub model: Option<String>,
    /// Optional focus hint steering the question topic; empty means the
    /// generator picks the least-covered ground itself.
    #[serde(default)]
    pub focus: String,
}

/// AI-generated activity draft shown to the learner for preview before they
/// confirm adding it to the lesson.
#[derive(Debug, Clone, Serialize)]
pub struct GeneratedLessonActivity {
    pub kind: ActivityKind,
    pub prompt: String,
    pub options: Vec<String>,
    pub answer: Value,
    pub explanation: String,
    pub distractors: Vec<String>,
    /// Suggested concept bindings (the lesson's concepts by default).
    pub concept_ids: Vec<LearningConceptId>,
}

/// Concept offered in the custom question form: any concept the learner
/// has enrolled in, plus orphaned concepts still referenced by their
/// surviving review items.
#[derive(Debug, Clone, Serialize)]
pub struct ConceptRef {
    pub concept_id: LearningConceptId,
    pub title: String,
    pub course_title: Option<String>,
}

/// Replaces the tag set of a course or question. Unknown tag names are
/// created automatically; names are trimmed, empty values dropped and
/// duplicates collapsed before storing.
#[derive(Debug, Clone, Deserialize)]
pub struct SetTagsRequest {
    pub tags: Vec<String>,
    /// For courses only: also append every tag of the final set to each
    /// question under the course, keeping the questions' existing tags.
    #[serde(default)]
    pub apply_to_children: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeleteCourseRequest {
    /// Also remove the learner's review items, mastery, progress, attempts
    /// and enrollment for this course. When false the content stays in the
    /// database so orphaned concepts remain reviewable.
    #[serde(default)]
    pub delete_reviews: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredActivityConfig {
    pub options: Vec<String>,
    pub answer: Value,
    pub explanation: String,
    /// Near-synonym traps for fill_in_blank blanks; empty for other kinds.
    /// Old rows lack the column, so it defaults on read.
    #[serde(default)]
    pub distractors: Vec<String>,
    /// Accepted numeric deviation (|response − answer| ≤ tol); None = exact.
    /// Old rows lack it, so it defaults on read.
    #[serde(default)]
    pub tol: Option<f64>,
    /// Matching questions only: the right-column candidate values (the
    /// answer's own values, order preserved) sent to the UI so the learner
    /// can pick a match per left item without seeing the alignment.
    /// Old rows lack it, so it defaults on read.
    #[serde(default)]
    pub matches: Vec<String>,
    /// Difficulty tier 1-3 declared at quiz time (概念辨析/应用/综合陷阱).
    /// Old rows lack it, so it defaults on read.
    #[serde(default)]
    pub difficulty: Option<u8>,
}

#[cfg(test)]
mod contract_tests {
    //! 内容契约的反漂移钉子（ADR-0002 追加决策）：渲染产物必须由 owner
    //! 表格生成，门槛与预算必须同口径同源——数值改动只能发生在表格里。

    use super::*;

    /// 质检门下限与低档文字预算同源：提示词照任何档位的预算写，都不会
    /// 撞门（历史 bug：门 300 全字符 vs 低档预算 150 纯文字，自相矛盾）。
    #[test]
    fn prose_floor_is_low_tier_budget() {
        assert_eq!(
            SectionKind::Concept.min_body_chars(),
            ComplexityTier::Low.prose_budget()
        );
        assert_eq!(
            SectionKind::Demo.min_body_chars(),
            ComplexityTier::Low.prose_budget()
        );
        // 预算表本身必须单调，"低档是最小预算"才成立。
        assert!(ComplexityTier::Low.prose_budget() < ComplexityTier::Mid.prose_budget());
        assert!(ComplexityTier::Mid.prose_budget() < ComplexityTier::High.prose_budget());
    }

    /// 可视化为主三节型按纯文字口径计量：带大可视化块 + 低档预算正文即
    /// 过门；纯文字不足低档预算则拒。
    #[test]
    fn visual_first_kinds_measure_prose_only() {
        let figure = "```svg\n<svg viewBox=\"0 0 10 10\"><text x=\"1\" y=\"1\">unit circle figure with named points and ticks</text></svg>\n```";
        let prose = "正弦函数是单位圆上纵坐标的投影。".repeat(10); // 160 字 ≥ 低档预算 150
        let body = format!("## 概念：正弦函数\n\n{prose}\n\n{figure}");
        let pack = SectionPack {
            section_key: "s1".into(),
            kind: SectionKind::Concept,
            title: "概念：正弦函数".into(),
            points: String::new(),
            visual: "函数图".into(),
            body_md: body,
        };
        // 计量断言：纯文字恰好过门（全字符口径会因图块文本虚高）。
        let prose_chars = prose_char_count(&pack.body_md);
        assert!(prose_chars >= ComplexityTier::Low.prose_budget());
        assert!(pack.validate_body().is_ok(), "prose at budget passes: {prose_chars}");

        let thin = SectionPack {
            body_md: "## 概念：正弦函数\n\n太短。".into(),
            ..pack.clone()
        };
        let error = thin.validate_body().unwrap_err();
        assert!(error.contains("visualization blocks excluded"), "{error}");
    }

    /// 渲染产物由表格生成：改表格数值必然改渲染——反之，提示词里手写
    /// 数值字面量的新拼写点会被这里抓住（渲染不含表格外的数字）。
    #[test]
    fn rendered_rules_come_from_the_tables() {
        let ranges = section_range_rules();
        for tier in [ComplexityTier::Low, ComplexityTier::Mid, ComplexityTier::High] {
            let (min, max) = tier.section_range();
            assert!(ranges.contains(&format!("{} {min}-{max}", tier.label())), "{ranges}");
        }
        assert!(ranges.contains(&format!("硬上限 {SECTION_OUTLINE_CAP}")), "{ranges}");

        assert_eq!(visual_menu_text(), VISUAL_OPTIONS.join(" / "));

        let budgets = prose_budget_rules();
        for tier in [ComplexityTier::Low, ComplexityTier::Mid, ComplexityTier::High] {
            assert!(
                budgets.contains(&format!("{} {} 字", tier.label(), tier.prose_budget())),
                "{budgets}"
            );
        }
        assert!(
            budgets.contains(&format!("下限 {} 字", SectionKind::Concept.min_body_chars())),
            "{budgets}"
        );
    }
}
