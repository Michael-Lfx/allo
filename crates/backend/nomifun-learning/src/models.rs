use nomifun_common::{
    AppError, KnowledgeBaseId, LearningActivityId, LearningAttemptId, LearningCourseId,
    LearningEnrollmentId, LearningLessonId, LearningModuleId, LearningReviewItemId, ProviderId,
    TimestampMs,
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
    /// 多知识库（t4）：来源库列表，首个 = 主来源（落库到
    /// `learning_courses.source_kb_id`）。列表非空时以列表为准，单
    /// `knowledge_base_id` 是旧客户端的单元素视图——两者同时给出时单值必须
    /// 是列表成员（否则 400，不静默丢来源）。上限
    /// [`COURSE_MAX_KNOWLEDGE_BASES`]。
    #[serde(default)]
    pub knowledge_base_ids: Vec<KnowledgeBaseId>,
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
    /// 课程类型（beta）：`learning_graph` 走学习图生长（描述即学习目标），
    /// 缺省为传统课程。
    #[serde(default)]
    pub course_kind: CourseKind,
    /// 学习图课程的程度声明（可选，g2 输入最小化）：达成后能独立做成什么；
    /// 缺省由结构生成依据目标扩写。
    #[serde(default)]
    pub proficiency_statement: Option<String>,
    /// 学习图课程的学习者基础声明（可选）：缺省按纯零基础画地形。
    #[serde(default)]
    pub learner_basis: Option<String>,
    /// 讲解风格（standard/socratic/feynman）；缺省 standard。
    #[serde(default)]
    pub teaching_style: Option<TeachingStyle>,
    /// 用户目标（传统课程的知识库流，2026-10-13 裁决）：整体文本即目标——
    /// 简单时即课程名称（课程名本身是高度抽象的目标），复杂时是目标描述/
    /// 长说明，不做形态判断。旧直连客户端与 agent 旧调用不传，缺省时主题
    /// 回退库名（不报错）。
    #[serde(default)]
    pub goal: Option<String>,
}

/// 用户目标的防御性限长（字符数）：整体文本即目标、要完整落库，因此只挡
/// 滥用式的超长输入，不承担展示截断——课程标题的截断口径见
/// [`goal_course_title`]。
pub const COURSE_GOAL_MAX_CHARS: usize = 2000;

/// 建课的来源库选择上限（t4）：按 t2 采样预算倒推——总量预算 40 文件在库间
/// 等分（每库保底 [`crate::generation::MULTI_KB_PER_BASE_MIN_FILES`]），5 库
/// 时每库 8 文件仍有完整的结构预算；再多的库只会把每库采样压到无意义的
/// 零头。超限 400（在请求面拒绝，不进生成）。
///
/// 三处同值的两个镜像（跨层无法共享一个常量，改动时三处同步）：
/// agent 引擎 `crates/agent/nomi-agent/src/learning_tools.rs::MAX_COURSE_BASES`
/// （工具层先拒）与前端 `ui/src/renderer/pages/learning/hooks/
/// useCourseCreation.ts::MAX_COURSE_KNOWLEDGE_BASES`（超限可见报错、不静默
/// 截断）；这里是权威面。
pub const COURSE_MAX_KNOWLEDGE_BASES: usize = 5;

/// 课程标题的展示截断口径：目标文本折叠为单行（连续空白压成一个空格）、
/// 去首尾空白，超过 [`COURSE_GOAL_TITLE_MAX_CHARS`] 字符时截断加省略号。
/// 目标短时标题即目标全文（裁决：课程名默认即该文本，不引入 LLM 派名）。
pub const COURSE_GOAL_TITLE_MAX_CHARS: usize = 60;

/// 课程标题推导：目标文本 → 单行标题（折叠空白 + 限长）。空目标返回空串
/// （调用方必须自行处理缺省回退），纯函数、确定性、可重放。
pub fn goal_course_title(goal: &str) -> String {
    let mut title = String::new();
    let mut pending_space = false;
    for ch in goal.trim().chars() {
        if ch.is_whitespace() {
            pending_space = !title.is_empty();
            continue;
        }
        if pending_space {
            title.push(' ');
            pending_space = false;
        }
        title.push(ch);
    }
    if title.chars().count() > COURSE_GOAL_TITLE_MAX_CHARS {
        let mut truncated: String = title.chars().take(COURSE_GOAL_TITLE_MAX_CHARS).collect();
        truncated.push('…');
        return truncated;
    }
    title
}

/// 课程生成状态（后台指示条/取消入口的数据源，学习图生长与大纲流共用）。
/// 生成在 HTTP 请求内同步执行（大纲）或后台任务执行（学习图生长），注册表
/// 让运行对外可发现、可取消。
#[derive(Debug, Serialize)]
pub struct LearningGraphGenerationStatus {
    pub running: bool,
    pub topic: Option<String>,
    pub elapsed_secs: Option<u64>,
}

impl GenerateCourseRequest {
    /// 归一后的用户目标（知识库流）：trim 后为空即 `None`（缺省回退库名），
    /// 超长按 [`COURSE_GOAL_MAX_CHARS`] 截断。整体文本即目标，不做形态判断。
    pub fn goal_text(&self) -> Option<String> {
        let goal = self.goal.as_deref()?.trim();
        if goal.is_empty() {
            return None;
        }
        Some(goal.chars().take(COURSE_GOAL_MAX_CHARS).collect())
    }

    /// 归一后的来源库列表（t4）：`knowledge_base_ids` 非空时以列表为准（按
    /// 首次出现去重、保序——顺序有意义，首个 = 主来源）；列表为空时回退单
    /// `knowledge_base_id` 的单元素视图（旧客户端逐字兼容）；都没有则空。
    pub fn knowledge_base_ids_normalized(&self) -> Vec<KnowledgeBaseId> {
        let mut ids: Vec<KnowledgeBaseId> = Vec::new();
        for id in &self.knowledge_base_ids {
            if !ids.iter().any(|existing| existing == id) {
                ids.push(id.clone());
            }
        }
        if ids.is_empty() {
            if let Some(id) = &self.knowledge_base_id {
                ids.push(id.clone());
            }
        }
        ids
    }

    /// 生成去重键（t4）：库 id 排序后以 `+` 拼接（单库保持 t2 的
    /// `kb:{id}` 形状）——乱序提交同一组库命中同一槽位，不会重复建课。无来源
    /// （描述流）为 `None`。
    pub fn knowledge_base_key(&self) -> Option<String> {
        let ids = self.knowledge_base_ids_normalized();
        if ids.is_empty() {
            return None;
        }
        let mut parts: Vec<&str> = ids.iter().map(KnowledgeBaseId::as_str).collect();
        parts.sort_unstable();
        Some(format!("kb:{}", parts.join("+")))
    }

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
        let kb_ids = self.knowledge_base_ids_normalized();
        // 两者同时给出时列表优先，但单值必须是列表成员——静默丢掉用户指定
        // 的来源等于从错误的库建课（t4 裁决）。
        if let (Some(single), false) = (
            self.knowledge_base_id.as_ref(),
            self.knowledge_base_ids.is_empty(),
        ) {
            if !kb_ids.iter().any(|candidate| candidate == single) {
                return Err(AppError::BadRequest(
                    "knowledge_base_id must be one of knowledge_base_ids".into(),
                ));
            }
        }
        if kb_ids.len() > COURSE_MAX_KNOWLEDGE_BASES {
            return Err(AppError::BadRequest(format!(
                "at most {COURSE_MAX_KNOWLEDGE_BASES} knowledge bases can ground one course, got {}",
                kb_ids.len()
            )));
        }
        if self.course_kind == CourseKind::LearningGraph {
            // 学习图课程只走描述流：描述即学习目标，知识库采样与
            // 模块/课时数都不参与生成。
            if !kb_ids.is_empty() {
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
        // 恰好一个来源：知识库（列表或单值归一后的列表）与描述互斥。
        if !kb_ids.is_empty() == self.description.is_some() {
            return Err(AppError::BadRequest(
                "exactly one of knowledge_base_id(s) or description must be provided".into(),
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
    /// 自由文本学习建议（ADR-0007）：单节重写时可选携带，为空即与既有
    /// 失败节重写行为一致（同分布重生成）。
    #[serde(default)]
    pub feedback: Option<String>,
}

/// 手动编辑节正文（ADR-0007）：仅覆盖 `body_md`，version+1、status 保持
/// 'ready'，不记录编辑来源。
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateLessonSectionBodyRequest {
    pub body_md: String,
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
    /// est：专注工作量尺寸锚（1 est 分钟 = 1 分钟专注）。键名统一 est
    /// （j01）；`estimated_minutes` 是导入旧包的兼容别名（双读），DB 列
    /// 保留旧名 `learning_lessons.estimated_minutes`。
    #[serde(default = "default_est", alias = "estimated_minutes")]
    pub est: i64,
    /// 主来源（兼容位）：落库时写进既有的 `source_path/start/end` 三列，
    /// 恒等于 [`LessonPack::sources`] 的首个。
    #[serde(default)]
    pub source: Option<SourceSpan>,
    /// 课时的全部引用来源（t4 评审补齐）：位序即引用顺序，首个 = 主来源。
    /// 空 = 只有兼容位 `source`（老形状导入），落库时归一。
    #[serde(default)]
    pub sources: Vec<SourceSpan>,
    #[serde(default)]
    pub activities: Vec<ActivityPack>,
    /// 分节正文（ADR-0002）。缺省为空 = 旧导入课程无分节，读取端回退整篇
    /// summary 渲染（双读）。
    #[serde(default)]
    pub sections: Vec<SectionPack>,
}

impl LessonPack {
    /// 课时的全部引用来源（归一）：`sources` 为准，为空时用兼容位升成
    /// 单元素列表——落库只读这一份清单。
    pub fn cited_sources(&self) -> Vec<&SourceSpan> {
        if self.sources.is_empty() {
            self.source.iter().collect()
        } else {
            self.sources.iter().collect()
        }
    }
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

/// 节清单的 visual 声明分布（声明形态 → 节数），生成进度披露与问题定位
/// 用（ADR-0008）：「无」独占整课即可疑的纯文字课时。
pub fn visual_distribution(sections: &[SectionPack]) -> std::collections::BTreeMap<&str, usize> {
    let mut distribution = std::collections::BTreeMap::new();
    for section in sections {
        *distribution.entry(section.visual.trim()).or_insert(0) += 1;
    }
    distribution
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

const fn default_est() -> i64 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSpan {
    pub path: String,
    #[serde(default)]
    pub start: Option<i64>,
    #[serde(default)]
    pub end: Option<i64>,
    /// 来源库（t4）：该 source 文件所属的知识库。多库课程的采样路径带
    /// `{库名}/rel` 命名空间，本字段是命名空间解析出的库 id（蓝图 source
    /// 与课时视图都写）；单库/旧课程的裸路径为 `None`，读侧回退课程的
    /// 主来源（`CourseSummary.source_kb_id`）。
    #[serde(default)]
    pub kb_id: Option<KnowledgeBaseId>,
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
    /// 题干证据回引（t2）：objective 题从来源节正文/引用摘录里逐字摘的
    /// 一小段（≤120 字），判底展示随 grade 下发。None = 未提供（读侧容缺
    /// ——flash 遵循是概率性的，不做硬门）。
    #[serde(default)]
    pub evidence: Option<String>,
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
/// （beta）从学习目标出发：先生成知识结构快照（参考地图），再按批次生长，
/// 依赖由概念网表达（ADR-0009/0010），
/// 课程大纲被「下一步推荐学习的节点」取代。
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
    /// 主来源（t4）：列表首个**现存**库；老课程/已删库为 `None`（单列视图
    /// 保留，读侧对已删库过滤后列表为空时即无来源）。
    pub source_kb_id: Option<KnowledgeBaseId>,
    /// 来源库列表（t4）：多库选择的全量，读侧已剔除已删库 id（不外泄悬空
    /// id）；空列表 = 描述流或老课程的单列回退视图为空。
    pub source_kb_ids: Vec<KnowledgeBaseId>,
    pub version: i64,
    pub enrolled: bool,
    pub total_lessons: i64,
    pub completed_lessons: i64,
    /// 课程级「现存且启用」口径（三轮 g3）：`false` = 已归档（可逆暂停），
    /// 安排面（到期队列/就绪推荐/生成入口）不再出现本课，历史面如实保留。
    pub enabled: bool,
    /// 概况读数（三轮 g3）：已结算或已完成的课时/节点数——XP 账本（j05）
    /// 落地前先行等于 `completed_lessons`，j05 切换为结算口径。
    pub settled_or_completed: i64,
    /// 概况读数：今日到期题数（本用户本课，与到期队列同口径）。
    pub due_today: i64,
    /// 概况读数：累计 XP——j05 结算账本落地前恒为 `None`（可空先行）。
    pub total_xp: Option<i64>,
    pub updated_at: TimestampMs,
    pub tags: Vec<String>,
    /// 用户目标（t1）：传统课程的知识库流目标文本；旧课程/描述流为空即
    /// `None`（列上是空串，语义为空）。
    pub learning_goal: Option<String>,
    /// 目标感知采样的披露（t2）：无目标路径/旧课程为 `None`。
    pub sampling: Option<CourseSamplingView>,
    /// 来源计划快照的摘要（t4）：列上 NULL（旧课程/描述流/未走新管线）
    /// 为 `None`——字段不出现;未知版本/坏形状 → `available=false` 降级,
    /// 不报错、不伪造。DTO 细化归 t5,这里只读计数。
    pub source_plan: Option<CourseSourcePlanView>,
}

/// 采样披露的对外视图（t2）：计数 + 低分未采样的少量示例。完整文件树只
/// 在库里（诊断用），列表/详情响应不背全量路径。`goal_fallback` = 目标与库
/// 无交集，采样退回默认顺序（披露不得冒充"按目标采样"）。
#[derive(Debug, Clone, Serialize)]
pub struct CourseSamplingView {
    pub goal: Option<String>,
    pub total_files: usize,
    pub sampled_files: usize,
    pub coherence_files: usize,
    pub goal_fallback: bool,
    pub low_score_files: usize,
    pub low_score_examples: Vec<String>,
    /// 多知识库（t4）：逐库归属与计数（单库/旧课程为空列表）。命名空间是
    /// 采样路径的前缀（`{namespace}/rel`），路径归属与「来源：库A、库B」
    /// 的展示都由它解析。
    pub bases: Vec<CourseSamplingBaseView>,
}

/// 单库的采样归属（t4 披露）：库 id + 库名 + 路径命名空间 + 采样计数。
#[derive(Debug, Clone, Serialize)]
pub struct CourseSamplingBaseView {
    pub kb_id: KnowledgeBaseId,
    pub name: String,
    pub namespace: String,
    pub total_files: usize,
    pub sampled_files: usize,
    /// 该库目标零命中：采样退回默认顺序（逐库如实披露，不冒充按目标采样）。
    pub goal_fallback: bool,
}

/// 来源计划快照的对外视图（t4 落地、t5 细化分面）：snapshot（版本/可用）+
/// triage（七角色计数/degraded/warnings）+ plan（章/概念/边/gaps + 章节
/// 来源映射摘要）+ coverage（topic 四态/前置练习去向/fallback）+ duplicates
/// （计数 + ≤20 示例）。完整快照只在库里（诊断按 `rel_path` 回 KB 读原文），
/// 列表/详情响应不背全量信封；`available=false` = 未知版本/坏形状（读侧
/// 降级，不崩溃不伪造）；列上 NULL 时本视图整体不出现。详情端的诊断明细
/// （全量 excluded/duplicates，限长）见 [`CourseSourcePlanDetailView`]。
#[derive(Debug, Clone, Default, Serialize)]
pub struct CourseSourcePlanView {
    pub snapshot: CourseSourcePlanSnapshotView,
    pub triage: CourseSourcePlanTriageView,
    pub plan: CourseSourcePlanPlanView,
    pub coverage: CourseSourcePlanCoverageView,
    pub duplicates: CourseSourcePlanDuplicatesView,
}

/// 快照信封的版本面：未知版本（读不出 version 的坏 JSON 同此）与 v1 形状
/// 损坏都降级 `available=false`，读侧不报错不伪造。
#[derive(Debug, Clone, Default, Serialize)]
pub struct CourseSourcePlanSnapshotView {
    pub version: Option<u64>,
    pub available: bool,
}

/// 快照里 t1 分诊的摘要面：七角色计数（总数守恒：各角色之和 =
/// `total_files`）+ degraded（规则分诊，未做 LLM 判定）+ 警告（限长）。
/// h1 新增补判/仍缺读数（可选序列化：零值不出现，旧读侧无感知）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct CourseSourcePlanTriageView {
    pub total_files: usize,
    pub core: usize,
    pub prerequisite: usize,
    pub practice: usize,
    pub mixed: usize,
    pub noise: usize,
    pub duplicate: usize,
    pub uncertain: usize,
    pub degraded: bool,
    /// 批补判次数（拆半/缺条目子集的重发 LLM 调用次数）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_repair_count: Option<usize>,
    /// 仍缺条目数（穷尽补判后保守 uncertain 的文件数）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub still_missing_count: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// h1 披露读数的零值抑制口径（单一实现，两处视图组装共用）：
/// `batch_repair_count`/`still_missing_count` 为 0 时字段不序列化
/// （`Option::is_none`，旧读侧无感知），非零才携带数值。
pub(crate) fn disclosed(count: usize) -> Option<usize> {
    (count > 0).then_some(count)
}

/// t2 计划的一章的来源映射摘要（章 → 概念数/课时数/来源文件数；零课时章
/// 显式 skipped + reason——「为什么没进课程」在章节级也要能回答）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct CourseSourcePlanChapterSummaryView {
    pub title: String,
    /// 该章覆盖的概念数（`PlannedChapter::covers`）。
    pub concepts: usize,
    /// 该章产出的课时数（来自 t3 覆盖报告的章 → 课时对账）。
    pub lessons: usize,
    pub skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
    /// 该章的去重来源文件数（`source_anchors` 的 rel_path 去重）。
    pub source_files: usize,
}

/// 快照里 t2 计划的摘要面：章/概念/边计数 + 证据 gaps + 章节来源映射
/// 摘要（限长）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct CourseSourcePlanPlanView {
    pub chapters: usize,
    pub concepts: usize,
    pub edges: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<String>,
    pub chapter_summaries: Vec<CourseSourcePlanChapterSummaryView>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// 一个被显式跳过的 topic（label + 理由）——覆盖缺口的可见示例。
#[derive(Debug, Clone, Default, Serialize)]
pub struct CourseSourcePlanSkippedTopicView {
    pub label: String,
    pub reason: Option<String>,
}

/// 快照里 t3 展开覆盖的摘要面：topic 四态计数（covered/skipped/uncertain）
/// + 前置/练习角色的 topic 去向 + anchor_uncertain 计数 + 跳过示例（≤20）+
/// 兜底课时数。文件级 role 只作诊断（详情端的明细视图），覆盖以 topic 为主。
#[derive(Debug, Clone, Default, Serialize)]
pub struct CourseSourcePlanCoverageView {
    pub topics_covered: usize,
    pub topics_skipped: usize,
    /// 前置角色文件上的 topic 进入课时的数量（「前置内容去了哪」）。
    pub prerequisite_topics: usize,
    /// 练习角色文件上的 topic 进入课时的数量（「练习内容去了哪」）。
    pub practice_topics: usize,
    /// 锚点无法精确定位的 topic 数（path 级兜底/标题对不上原文）。
    pub anchor_uncertain_topics: usize,
    /// 状态 uncertain 的 topic label（限长 ≤20）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub uncertain: Vec<String>,
    /// 被显式跳过的 topic 示例（限长 ≤20；详情可看全量）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped_examples: Vec<CourseSourcePlanSkippedTopicView>,
    /// H1/文件名兜底课的数量（不伪装成正常设计，醒目披露）。
    pub fallback_lessons: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// 一条重复/翻译副本的示例（path + 指向的 canonical + canonical 的入选
/// 理由）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct CourseSourcePlanDuplicateExampleView {
    pub path: String,
    pub duplicate_of: String,
    pub canonical_reason: String,
}

/// 快照里 t1 分诊的重复面：计数（全量）+ 示例（≤20）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct CourseSourcePlanDuplicatesView {
    pub count: usize,
    pub examples: Vec<CourseSourcePlanDuplicateExampleView>,
}

/// 详情端点的来源计划诊断明细（t5）：全量 `excluded`（path + role +
/// reason）与全量 `duplicates`，均限长（≤200）——列表摘要只给计数/示例，
/// 详情页看完整「为什么没进课程」。role 是 t1 的 snake_case 角色名
/// （`noise`/`duplicate`/…）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct CourseSourcePlanDetailView {
    pub excluded: Vec<CourseSourcePlanExcludedView>,
    pub duplicates: Vec<CourseSourcePlanDuplicateExampleView>,
}

/// 一条被排除出候选集的文件（`noise`/`duplicate`），必须能回答
/// 「为什么没进课程」。
#[derive(Debug, Clone, Default, Serialize)]
pub struct CourseSourcePlanExcludedView {
    pub path: String,
    pub role: String,
    pub reason: String,
}

/// t5 成本门确认阈值（票面定案,后端单源）:超过任一阈值,建课前必须展示
/// 估算并让用户确认;低于阈值自动跑。
pub const COURSE_ESTIMATE_CONFIRM_FILES: usize = 40;
pub const COURSE_ESTIMATE_CONFIRM_CALLS: usize = 20;

/// t5 成本门估算视图:提交建课前的一次**规则分诊**读数（零 LLM、
/// `triage.degraded=true`——「规则分诊,未做 LLM 判定」在估算里就如实标出）。
/// `needs_confirm` 由后端按 [`COURSE_ESTIMATE_CONFIRM_FILES`]/
/// [`COURSE_ESTIMATE_CONFIRM_CALLS`] 判定;描述流全零、不确认。
#[derive(Debug, Clone, Default, Serialize)]
pub struct CourseGenerationEstimateView {
    pub total_files: usize,
    pub estimated_batches: usize,
    pub estimated_llm_calls: usize,
    /// 粗估耗时（秒,按调用数 × 经验常数;仅供确认弹窗展示,不是承诺）。
    pub estimated_seconds: u64,
    pub needs_confirm: bool,
    pub triage: CourseSourcePlanTriageView,
    /// 规则判定的跳过摘要（噪声/重复,path + role + reason,限长 ≤10）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped_examples: Vec<CourseSourcePlanExcludedView>,
}

/// R 软闸的一条建议（ADR-0009 Amendment 1）：结构就绪但供给节点已遗忘
/// ——建议先复习供给节点（候选仍可从学习记录进入，软闸只降推荐优先级）。
#[derive(Debug, Clone, Serialize)]
pub struct GraphBlockedView {
    /// 被挡的就绪候选节点。
    pub lesson_id: LearningLessonId,
    pub title: String,
    /// 供给节点（教该候选所假定概念的节点，跨课程）。
    pub supplier_lesson_id: LearningLessonId,
    pub supplier_title: String,
    /// 供给节点代表预测回忆率（该节点全部复习卡的最小值）。
    pub r: f64,
    /// 供给节点当前到期题数。
    pub due_count: i64,
}

/// 学习结构读侧投影（`GET /courses/{id}/graph/structure`，wayfinder 票 t1）：
/// 200 恒答、`structure: null` = 未生成（Missing 合法空状态，生成中态由 WS
/// structure_* 事件表达）。投影运行时重算、永不落盘；**严格课程内**——只读
/// 本课 structure_json，不 join 概念登记表。j08 起带挂载读侧三态投影
/// （`mounts: null` = 从未跑过挂载回合）。
#[derive(Debug, Clone, Serialize)]
pub struct LearningStructureView {
    pub course_id: LearningCourseId,
    pub structure: Option<StructureView>,
    pub terrain: Option<TerrainView>,
    /// 快照生成时刻（ms）；None = 未生成。
    pub generated_at: Option<i64>,
    /// 挂载三态投影（j08）：`None` = 挂载表无行（从未评估）。
    pub mounts: Option<StructureMountsView>,
}

/// 结构产物的读侧视图（预计算面：主干分组 / 块名集 / 矩阵格 / 依赖分层 /
/// 枢纽出入度；层号 = 依赖深度，不是排期）。
#[derive(Debug, Clone, Serialize)]
pub struct StructureView {
    pub scalars: StructureScalarsView,
    pub domain: StructureDomainView,
    pub spines: Vec<SpineView>,
    pub blocks: Vec<BlockView>,
    pub edges: Vec<EdgeView>,
    pub gaps: Vec<GapView>,
    pub quality: Vec<QualityView>,
    /// 主干分组（块表序保持）。
    pub groups: Vec<SpineGroupView>,
    /// 块名集（升序）。
    pub block_names: Vec<String>,
    /// 主干×阶段矩阵格。
    pub matrix: Vec<MatrixRowView>,
    /// 枢纽 Top10（assumes + composes 出入度和降序）。
    pub hubs: Vec<HubView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StructureScalarsView {
    pub goal: String,
    pub goal_type: String,
    pub proficiency_statement: String,
    pub domain_type: String,
    pub form_reference: String,
    pub source_tier: String,
    pub generated_by: String,
    pub disclaimer: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StructureDomainView {
    pub core_definition: String,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub prerequisite_doors: Vec<String>,
    pub audience: String,
    pub scenarios: Vec<String>,
    pub granularity: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpineView {
    pub name: String,
    pub definition: String,
    pub key_points: Vec<String>,
    pub instance: String,
    pub thin_spine: Option<bool>,
    pub reason: String,
}

/// 块视图行：`layer` = assumes 前置链深度（L0 起算，层号不是排期）；
/// `no_prereq` = 本块不做任何 assumes 边之源（learnhub 读侧同款布尔）。
#[derive(Debug, Clone, Serialize)]
pub struct BlockView {
    pub name: String,
    pub slug: String,
    pub spine: String,
    pub stage: String,
    pub note: String,
    pub layer: usize,
    pub no_prereq: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EdgeView {
    pub source: String,
    pub target: String,
    pub edge_type: String,
    pub tier: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GapView {
    pub item: String,
    pub direction: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct QualityView {
    pub check: String,
    pub status: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpineGroupView {
    pub spine: String,
    pub block_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatrixRowView {
    pub spine: String,
    pub entryway: Vec<String>,
    pub intermediate: Vec<String>,
    pub advanced: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HubView {
    pub name: String,
    pub in_degree: usize,
    pub out_degree: usize,
}

/// 前置地形的读侧视图：门口列表 + 标量回执。
#[derive(Debug, Clone, Serialize)]
pub struct TerrainView {
    pub learner_basis_assumed: String,
    pub disclaimer: String,
    pub doors: Vec<TerrainDoorView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TerrainDoorView {
    pub id: String,
    pub criterion: String,
    pub kind: String,
}

/// 学习图课程的图视图（挂在 `CourseDetail.graph` 下）：就绪集推荐与水位
/// 读数。没有先修边、没有锁定态——发布即就绪（ADR-0009）；节点本体就是
/// 模块课时，图视图只承载生长语义。
#[derive(Debug, Clone, Serialize)]
pub struct LearningGraphView {
    /// 用户生成图时输入的学习目标。
    pub goal: String,
    /// 「下一步推荐学习的节点」（就绪集 ≤10，花名册序；被 R 软闸挡住的
    /// 候选不出现在这里，进 `blocked`）。
    pub recommended: Vec<LearningLessonId>,
    /// R 软闸建议（≤3 条，按供给节点回忆率升序）。
    pub blocked: Vec<GraphBlockedView>,
    /// 当前就绪存量与水位契约（补货目标 7 / 自动触发线 3）。
    pub ready_count: usize,
    pub ready_target: usize,
    pub ready_trigger: usize,
    /// 是否有生成/生长运行进行中（共享注册表口径）。
    pub growth_running: bool,
}

/// 学习记录视图：批次时间线（倒序）——生长史即课程史（ADR-0009）。
#[derive(Debug, Clone, Serialize)]
pub struct GraphHistoryView {
    pub batches: Vec<GraphBatchView>,
}

/// 一个生长批次的出生档案：序号/批注/时刻 + 节点行（含学习者进度）。
/// status 是批行状态机：pending=生长进行中（或中断，见 failed）、
/// applied=已落库、failed=生长失败（ADR-0009 Amendment 1）。
#[derive(Debug, Clone, Serialize)]
pub struct GraphBatchView {
    pub batch_id: String,
    pub seq: i64,
    pub status: String,
    pub note: String,
    pub created_at: i64,
    pub nodes: Vec<GraphNodeHistoryView>,
}

/// 批次内的一个节点行。
#[derive(Debug, Clone, Serialize)]
pub struct GraphNodeHistoryView {
    pub lesson_id: LearningLessonId,
    pub title: String,
    /// est：专注工作量尺寸锚（DB 列保留旧名 `estimated_minutes`）。
    pub est: i64,
    pub status: LessonStatus,
    pub completed_at: Option<i64>,
}

/// 概念表行：登记表概念 + 本课程的教/假定引用 + 跨课程来源标注。
#[derive(Debug, Clone, Serialize)]
pub struct GraphConceptRowView {
    pub concept_id: String,
    pub canonical: String,
    /// 活跃别名（j04 结构化记录：名字 + 投影定义 + 出处；退役名不列）。
    pub aliases: Vec<GraphConceptAliasView>,
    pub definition: String,
    /// 本课程内教/假定该概念的节点（带学习者进度状态）。
    pub refs: Vec<GraphConceptRefView>,
    /// 其他还在教该概念的课程标题（跨课程就绪的来源可见性）。
    pub other_courses: Vec<String>,
}

/// 一条活跃别名（j04）：名字 + 该名字的意义（投影角度定义，缺席 = 与概念
/// 定义同义）+ 出处（提名的 source）。
#[derive(Debug, Clone, Serialize)]
pub struct GraphConceptAliasView {
    pub name: String,
    pub definition: Option<String>,
    pub attestation: Option<String>,
}

/// 概念表行内的一条引用。
#[derive(Debug, Clone, Serialize)]
pub struct GraphConceptRefView {
    pub lesson_id: LearningLessonId,
    pub title: String,
    /// teaches | assumes。
    pub role: String,
    /// know | apply | teach。
    pub tier: String,
    /// 投影名（j04）：节点引用该概念时使用的名字；老行为 None（读侧回退
    /// 概念正名）。
    pub ref_name: Option<String>,
    pub status: LessonStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct CourseDetail {
    pub course: CourseSummary,
    pub enrollment_id: Option<LearningEnrollmentId>,
    pub modules: Vec<ModuleView>,
    pub next_lesson_id: Option<LearningLessonId>,
    pub due_review_count: i64,
    /// 仅 `learning_graph` 课程携带：就绪集推荐与水位读数（结构投影走独立接口）。
    pub graph: Option<LearningGraphView>,
    /// 来源计划诊断明细（t5）：全量 excluded/duplicates（限长）。只挂在
    /// 详情端点——列表摘要（`course.source_plan`）只给计数/示例；列上
    /// NULL（旧课程/描述流）时本字段不出现。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_plan_detail: Option<CourseSourcePlanDetailView>,
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
    /// est：专注工作量尺寸锚（1 est 分钟 = 1 分钟专注）。键名统一 est
    /// （j01）；DB 列保留旧名 `estimated_minutes`。
    pub est: i64,
    pub generated: bool,
    /// 主来源（兼容位）：恒等于 [`LessonView::sources`] 的首个；无引用为
    /// `None`（描述流/老课时无来源）。
    pub source: Option<SourceSpan>,
    /// 课时的全部引用来源（t4 评审补齐）：位序即引用顺序，首个 = 主来源；
    /// 每条都带解析出的库归属（`kb_id`）。空 = 无引用（描述流）。
    pub sources: Vec<SourceSpan>,
    pub status: LessonStatus,
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
    /// 大纲声明的本节讲解主体可视化（承诺事实源，迁移 051 起落库）；
    /// 历史行为空串（未声明）。
    pub visual: String,
    pub body_md: String,
    /// 当前正文是否为降级纯文字兜底（visual 承诺未兑现，迁移 053 起落库，
    /// ADR-0008）；重写成功兑现承诺或手动编辑后清除。
    pub degraded: bool,
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
    pub items: Vec<DiagnosticItem>,
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
    /// Review item coming from a course enrollment.
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

/// Daily check-in snapshot for the current review day, in the XP reading
/// (j05 retires the `dailyCheckinGoal` review-count gauge — j05 后端的打卡
/// 口径即 XP：goal 是日 XP 目标，`reviewed_count` 字段承载今日净入账，字段
/// 名保留到 j06 换 UI 数据源)。
#[derive(Debug, Clone, Serialize)]
pub struct CheckinStatus {
    /// Local review day as YYYYMMDD.
    pub review_day: i64,
    /// Daily XP goal (application setting `learning.dailyXpGoal`, default 40).
    pub goal: i64,
    /// Net XP earned so far this review day (was: review-event count).
    pub reviewed_count: i64,
    /// Cards currently due (`due_at <= now`), course + custom.
    pub due_count: i64,
    /// Whether the day is locked as completed (today's XP reached the goal).
    pub completed: bool,
    /// Lock moment in UTC milliseconds when completed, else null.
    pub locked_at: Option<i64>,
}

/// XP 读数（j05）：两把尺同源回放、互不换算——今日 XP/日目标（努力尺，
/// 回放 `learning_xp_events`）与结算进度（进度尺，分子分母按现存且启用同
/// 筛）。纯投影零新状态：ETA 由近 7 个学习日步伐派生，绝不回写。
#[derive(Debug, Clone, Serialize)]
pub struct XpReading {
    /// Current local review day as YYYYMMDD (02:00 rollover).
    pub review_day: i64,
    /// Net XP earned so far this review day (negative settlements included).
    pub today_xp: i64,
    /// Daily XP goal (application setting `learning.dailyXpGoal`).
    pub goal: i64,
    /// Whether today's XP has reached the goal.
    pub goal_achieved: bool,
    /// Progress gauge numerator: settled nodes within existing-and-enabled
    /// courses (same filter as the denominator).
    pub settled_nodes: i64,
    /// Progress gauge denominator: lessons of the user's existing-and-enabled
    /// courses.
    pub total_nodes: i64,
    /// Remaining est minutes of unsettled lessons in the same scope.
    pub remaining_est: i64,
    /// Days to finish at the trailing-7-review-day pace; `null` when there is
    /// no pace signal yet (or nothing remains → 0). Never written back.
    pub eta_days: Option<i64>,
}

/// XP 日历的单日桶（学习日 02:00 翻转）：净入账，负账如实，含零桶。
#[derive(Debug, Clone, Serialize)]
pub struct XpCalendarDay {
    /// Local review day as YYYYMMDD.
    pub review_day: i64,
    /// Net XP settled that review day (can be negative).
    pub xp: i64,
}

/// XP 日历（j05）：恒 `weeks × 7` 桶、末桶 = 今日，不过课程筛。
#[derive(Debug, Clone, Serialize)]
pub struct XpCalendar {
    /// Requested window length in weeks (1..=53).
    pub weeks: i64,
    /// tz offset (minutes east of UTC) the buckets were computed with.
    pub tz_offset: i32,
    /// Exactly `weeks * 7` day buckets, oldest first, today last.
    pub days: Vec<XpCalendarDay>,
}

/// quiz 触发进度读数（q1，读数面非门禁）：每课程 N/K——N = 距该课程上次
/// quiz 结算之后的学新结算数（复习/quiz 账不推进；零存储，账本回放派生），
/// K = 触发阈值。
#[derive(Debug, Clone, Serialize)]
pub struct QuizProgressView {
    /// 学新结算计数 N（quiz 结算即清零重新积累）。
    pub settled_count: i64,
    /// 触发阈值 K（起步值 6，待校准）。
    pub trigger_count: i64,
    /// N ≥ K = 到期（测量面读数，非门禁）。
    pub due: bool,
}

/// quiz 回合单题投影（q2，**判底零字段**：无 answer/explanation——解析与
/// 答案只随 [`QuizGradeFeedback`] 判分后下发；learnhub SessionQuestionView
/// 同红线）。
#[derive(Debug, Clone, Serialize)]
pub struct QuizRoundQuestionView {
    /// 题 ULID（grade/complete 的引用键）。
    pub activity_id: LearningActivityId,
    pub kind: ActivityKind,
    pub prompt: String,
    /// 选择题选项（非选择型为空）。
    pub options: Vec<String>,
    /// 匹配题右列候选（打乱后；非匹配型为空）。
    #[serde(default)]
    pub matches: Vec<String>,
    /// 题目所属学习节点（quiz 跨节点会话逐题携带）。
    pub node_id: LearningLessonId,
    pub node_name: String,
}

/// quiz 回合视图（q2 start 出参：情境快照 + 脱敏题单）。
#[derive(Debug, Clone, Serialize)]
pub struct QuizRoundView {
    /// 回合 ULID（grade/complete 的回合键）。
    pub round_id: String,
    pub course_id: LearningCourseId,
    /// active | completed | abandoned。
    pub status: String,
    /// 触发时刻的 N/K 读数（结算后自然清零，读数面非门禁）。
    pub settled_count: i64,
    pub trigger_count: i64,
    /// 题量上限读数（q1 常量集中表）。
    pub budget: usize,
    pub questions: Vec<QuizRoundQuestionView>,
}

/// 单题判分反馈（q2：解析与判底揭示的唯一口；`evaluate` 确定性判分零 LLM，
/// 纯判分不落账——结算在 complete 全卷面）。learnhub GradeFeedback 同形。
#[derive(Debug, Clone, Serialize)]
pub struct QuizGradeFeedback {
    pub activity_id: LearningActivityId,
    /// correct | incorrect。
    pub correct: bool,
    /// 判底揭示（判分已定，揭示不破持答案红线）。
    pub answer: Value,
    /// 解析（判分后下发）。
    pub explanation: String,
    /// 题干证据回引（t2 判底展示面；题源未提供则 None）。
    #[serde(default)]
    pub evidence: Option<String>,
}

/// quiz 回合逐节点对账行（q2；q3 扩逐题校准点）。
#[derive(Debug, Clone, Serialize)]
pub struct QuizRoundNodeReport {
    pub node_id: LearningLessonId,
    pub node_name: String,
    pub answered: i64,
    pub correct: i64,
    /// 本节点净入账；`None` = 同日已结算折叠（coverage 保持不重复计薪，
    /// learnhub quiz_settled_on_day 语义）。
    pub earned_xp: Option<i64>,
    /// 逐题校准点（q3：出题时刻的预测 R vs 实际对错；`serde(default)` 容
    /// q2 时代旧档无此字段）。
    #[serde(default)]
    pub questions: Vec<QuizCalibrationPoint>,
}

/// 单题校准点（q3，喂校准读面的数据源；learnhub CalibrationPoint 同形——
/// 无预测面不入预测均值）。
#[derive(Debug, Clone, serde::Deserialize, Serialize)]
pub struct QuizCalibrationPoint {
    pub activity_id: LearningActivityId,
    pub correct: bool,
    /// 作答时点该题卡的 FSRS 预测可提取性；无卡（变体/未建卡）= `None`。
    pub predicted_r: Option<f64>,
}

/// quiz 回合结算报告（q2 complete 出参）。
#[derive(Debug, Clone, Serialize)]
pub struct QuizRoundReport {
    pub round_id: String,
    pub course_id: LearningCourseId,
    /// 全卷净入账（Σ 逐节点；折叠不计薪）。
    pub earned_xp: i64,
    pub covered: Vec<QuizRoundNodeReport>,
}

/// 挂载回合摘要（j08）：status = mounted（有待挂并已落盘）/ skipped
/// （待挂集空——零 LLM 零写）。挂载是导航事实：计数面只有条目统计。
#[derive(Debug, Clone, Serialize)]
pub struct MountRoundSummary {
    /// "mounted" | "skipped".
    pub status: String,
    /// 本轮评估的待挂节点数（skipped 时为 0）。
    pub pending_nodes: usize,
    /// 本轮挂入块的节点数。
    pub mounted_nodes: usize,
    /// 本轮声明结构外的节点数。
    pub outside_nodes: usize,
    /// 挂载行当前总条目数（含既有保留）。
    pub total_entries: usize,
    /// 回合开始时挂载是否处于失配（结构变过导致全量重评）。
    pub stale: bool,
    /// 落盘锚定的结构 hash（FNV-1a 64，有符号整型承载）。
    pub structure_hash: i64,
}

/// 节点挂载三态（j08 单源派生）：Mounted（挂入可解析的块）/ Outside
/// （已评估、声明结构外）/ NotEvaluated（未评估，或条目悬挂降级）。
#[derive(Debug, Clone, Serialize)]
pub struct NodeMountStateView {
    pub node: String,
    /// "mounted" | "outside" | "not_evaluated".
    pub state: String,
}

/// 一条挂载事实的读侧视图：条目携带块引用清单；悬挂 slug（块已不在当前
/// 结构里）逐条标 `dangling = true` 降级，不使整体 Broken。
#[derive(Debug, Clone, Serialize)]
pub struct MountEntryView {
    pub node: String,
    /// 挂入的块引用（空 = 已评估、声明结构外）。
    pub blocks: Vec<MountBlockRefView>,
    pub reason: String,
}

/// 单个块引用的解析结果。
#[derive(Debug, Clone, Serialize)]
pub struct MountBlockRefView {
    pub slug: String,
    /// 块名（slug 在当前结构可解析时回填；悬挂为 None）。
    pub block_name: Option<String>,
    pub dangling: bool,
}

/// mount_index 单元：块 slug → 已挂节点清单（只含可解析块）。
#[derive(Debug, Clone, Serialize)]
pub struct MountIndexEntryView {
    pub block: String,
    pub block_name: Option<String>,
    pub nodes: Vec<String>,
}

/// 挂载观察（修补输入源之一，纯读派生不打点）：outside-node / empty-block
/// / overloaded-block（≥5 节点）。观察不构成编辑义务。
#[derive(Debug, Clone, Serialize)]
pub struct MountObservationView {
    pub kind: String,
    pub detail: String,
}

/// 挂载读侧投影（挂载行 + 结构快照 + 现存节点的纯函数合成面）。
#[derive(Debug, Clone, Serialize)]
pub struct StructureMountsView {
    /// 结构 hash 失配（挂载后结构又变过）——挂载整体显影为待重评。
    pub stale: bool,
    pub entries: Vec<MountEntryView>,
    pub index: Vec<MountIndexEntryView>,
    pub states: Vec<NodeMountStateView>,
    pub observations: Vec<MountObservationView>,
    pub mounted_count: usize,
    pub outside_count: usize,
    pub not_evaluated_count: usize,
}

/// 修补回合摘要（j09）：status = applied（有 ops 且已落盘）/ noop（ops 空，
/// LLM 已跑不写盘）/ skipped（findings 与挂载观察皆空——零 LLM 零写）。
#[derive(Debug, Clone, Serialize)]
pub struct StructureRepairSummary {
    /// "applied" | "noop" | "skipped".
    pub status: String,
    /// 回合前的结构 hash（FNV-1a 64，有符号整型承载）。
    pub old_hash: i64,
    /// 回合后的结构 hash（noop/skipped = old）。
    pub new_hash: i64,
    /// 应用的 op 人面标签（依应用序）。
    pub applied: Vec<String>,
    /// 级联注记（改名跟改/拆块边退场/并块去重去自环）。
    pub cascade: Vec<String>,
    /// 结构已改 = 既有挂载待重评估（本回合不动挂载，下回合全量重挂）。
    pub mounts_stale: bool,
    pub noop: bool,
    pub skipped: bool,
}

/// 维护需求的纯读判定（k02，learnhub ADR-0028 `maintenance_needs` 同构）：
/// 零 LLM——只回答「这一课要不要修补/挂载」。归档/软删/非图课程/无结构 =
/// `eligible = false`（不在扫描面）。
#[derive(Debug, Clone, Serialize)]
pub struct MaintenanceNeedsView {
    pub eligible: bool,
    /// 未消费 findings 条数（水位之后的留痕）。
    pub unconsumed_findings: i64,
    /// 待挂节点数（未评估 ∪ 失效 slug；stale 时 = 全部现存节点）。
    pub pending_mounts: usize,
    /// 结构 hash 与挂载行不一致（无挂载行 = stale）。
    pub mounts_stale: bool,
    pub needs_repair: bool,
    pub needs_mount: bool,
}

impl MaintenanceNeedsView {
    /// 无需求（零工作）——调度器据此跳过该课，零任务零 LLM。
    pub fn is_idle(&self) -> bool {
        !self.needs_repair && !self.needs_mount
    }
}

/// 一课在一个维护回合里的结果（k02）。
#[derive(Debug, Clone, Serialize)]
pub struct MaintenanceCourseOutcome {
    pub course_id: String,
    /// 回合前判定（修补后重新判定一次，见 `recheck`）。
    pub needs: MaintenanceNeedsView,
    /// 修补回合终态（applied/noop/skipped）；未跑 = None。
    pub repair: Option<String>,
    /// 挂载回合终态（mounted/skipped）；未跑 = None。
    pub mount: Option<String>,
    /// 修补改了结构 → 本回合内重判定后补挂。
    pub recheck: Option<MaintenanceNeedsView>,
    /// 因生成槽在途/失败跳过该课（含挂载）。
    pub skipped_busy: bool,
    /// 失败如实记（不吞）。
    pub error: Option<String>,
}

/// 一次自动维护回合的报告（k02）：零工作 = courses 空。
#[derive(Debug, Clone, Default, Serialize)]
pub struct MaintenanceTickReport {
    /// 扫描的合格课程数（enabled + 未删 + 图课程 + 有结构）。
    pub scanned: usize,
    /// 无需求被跳过的课程数（零工作）。
    pub idle: usize,
    pub courses: Vec<MaintenanceCourseOutcome>,
}

/// 维护设置（k02）：缺省开、间隔 360 分钟；越界/坏值读侧回退缺省。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MaintenanceSettings {
    pub enabled: bool,
    pub interval_minutes: i64,
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
/// objective activities (review item optional: items only exist after the
/// lesson is completed); custom questions are learner-authored and always
/// carry their own schedule.
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
/// fill in the blank) are supported; each carries its own FSRS schedule.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateCustomQuestionRequest {
    pub kind: ActivityKind,
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<String>,
    pub answer: Value,
    #[serde(default)]
    pub explanation: String,
    /// Near-synonym traps for fill_in_blank blanks (optional when the
    /// learner authors the question by hand).
    #[serde(default)]
    pub distractors: Vec<String>,
}

/// Manually appends an activity to an existing lesson. All kinds are
/// accepted.
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

/// 课程改名请求（三轮 g3）：只动显示名 `title`——目标原句/结构快照/学习
/// 数据一个不动。
#[derive(Debug, Clone, Deserialize)]
pub struct RenameCourseRequest {
    pub title: String,
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
    /// 题干证据回引（t2，判底展示面）：出题阶段从来源正文/摘录逐字摘的
    /// 一小段。旧行缺列，读侧默认 None。
    #[serde(default)]
    pub evidence: Option<String>,
}

#[cfg(test)]
mod goal_tests {
    //! 目标输入（t1）的归一化契约：整体文本即目标，缺省宽容、超长防御性
    //! 截断；课程标题的截断是展示口径（单行 + 省略号），与落库全文分开。

    use super::*;

    fn request(goal: Option<&str>) -> GenerateCourseRequest {
        GenerateCourseRequest {
            knowledge_base_id: Some(
                KnowledgeBaseId::parse("0190f5fe-7c00-7a00-8abc-000000000001").unwrap(),
            ),
            knowledge_base_ids: Vec::new(),
            description: None,
            domain: None,
            provider_id: None,
            model: None,
            mode: CourseGenerationMode::OnDemand,
            course_kind: CourseKind::Traditional,
            teaching_style: None,
            goal: goal.map(ToOwned::to_owned),
        proficiency_statement: None,
        learner_basis: None,
        }
    }

    #[test]
    fn goal_text_trims_and_treats_blank_as_absent() {
        assert_eq!(request(None).goal_text(), None);
        assert_eq!(request(Some("   ")).goal_text(), None);
        assert_eq!(
            request(Some("  零基础学 Python 数据分析 ")).goal_text(),
            Some("零基础学 Python 数据分析".to_owned())
        );
    }

    #[test]
    fn goal_text_keeps_multiline_text_and_bounds_length() {
        let long_goal = "零基础学 Python 数据分析。".repeat(400);
        let normalized = request(Some(&long_goal)).goal_text().unwrap();
        assert_eq!(normalized.chars().count(), COURSE_GOAL_MAX_CHARS);

        // 多行长目标：换行原样保留（落库全文），不做单行折叠。
        let multiline = "目标：学会 Python 数据分析\n受众：零基础\n产出：能独立完成一份分析报告";
        assert_eq!(request(Some(multiline)).goal_text().unwrap(), multiline);
    }

    #[test]
    fn goal_absent_keeps_the_legacy_kb_flow_valid() {
        // 旧客户端路径（无 goal）必须仍然合法：主题回退库名，不报错。
        assert!(request(None).validate().is_ok());
    }

    #[test]
    fn goal_course_title_collapses_to_one_line_and_truncates() {
        assert_eq!(goal_course_title("零基础学 Python 数据分析"), "零基础学 Python 数据分析");
        assert_eq!(
            goal_course_title("  目标：学会数据分析\n\n  受众：零基础  "),
            "目标：学会数据分析 受众：零基础"
        );
        let long = "零基础学 Python 数据分析".repeat(10);
        let title = goal_course_title(&long);
        let expected: String = long.chars().take(COURSE_GOAL_TITLE_MAX_CHARS).collect();
        assert_eq!(title, format!("{expected}…"));
        assert_eq!(goal_course_title("   "), "");
    }
}

#[cfg(test)]
mod multi_kb_tests {
    //! 多知识库选择（t4）的请求面契约：列表与单值并存时的优先级、去重、
    //! 选择上限（按 t2 采样预算倒推）与去重键的乱序不敏感性。

    use super::*;

    fn kb(id: &str) -> KnowledgeBaseId {
        KnowledgeBaseId::parse(id).unwrap()
    }

    fn request(single: Option<&str>, list: &[&str]) -> GenerateCourseRequest {
        GenerateCourseRequest {
            knowledge_base_id: single.map(kb),
            knowledge_base_ids: list.iter().map(|id| kb(id)).collect(),
            description: None,
            domain: None,
            provider_id: None,
            model: None,
            mode: CourseGenerationMode::OnDemand,
            course_kind: CourseKind::Traditional,
            teaching_style: None,
            goal: None,
        proficiency_statement: None,
        learner_basis: None,
        }
    }

    const A: &str = "0190f5fe-7c00-7a00-8abc-00000000000a";
    const B: &str = "0190f5fe-7c00-7a00-8abc-00000000000b";
    const C: &str = "0190f5fe-7c00-7a00-8abc-00000000000c";

    /// 单值 / 列表两条输入路径归一成同一条列表：列表非空时以列表为准
    /// （保序去重），列表为空时回退单值的单元素视图（旧客户端零变化）。
    #[test]
    fn multi_kb_list_normalizes_and_dedupes_in_order() {
        assert_eq!(request(Some(A), &[]).knowledge_base_ids_normalized(), vec![kb(A)]);
        assert_eq!(
            request(None, &[B, A]).knowledge_base_ids_normalized(),
            vec![kb(B), kb(A)],
            "列表顺序原样保留（首个 = 主来源）"
        );
        assert_eq!(
            request(Some(A), &[A, B, A]).knowledge_base_ids_normalized(),
            vec![kb(A), kb(B)],
            "重复项按首次出现去重"
        );
    }

    /// 上限 5（t2 采样预算倒推）：第 6 个库 400，五个库合法。
    #[test]
    fn multi_kb_selection_is_capped_at_five() {
        assert_eq!(COURSE_MAX_KNOWLEDGE_BASES, 5);
        let five: Vec<String> = (0..5)
            .map(|index| format!("0190f5fe-7c00-7a00-8abc-00000000001{index}"))
            .collect();
        let five_refs: Vec<&str> = five.iter().map(String::as_str).collect();
        assert!(request(None, &five_refs).validate().is_ok());

        let six: Vec<String> = (0..6)
            .map(|index| format!("0190f5fe-7c00-7a00-8abc-00000000001{index}"))
            .collect();
        let six_refs: Vec<&str> = six.iter().map(String::as_str).collect();
        let error = request(None, &six_refs).validate().unwrap_err();
        assert!(error.to_string().contains('5'), "{error}");
    }

    /// 两者同时给出：单值必须是列表成员（列表优先，不静默丢来源）。
    #[test]
    fn multi_kb_and_single_field_must_agree() {
        assert!(request(Some(A), &[A, B]).validate().is_ok(), "单值 ∈ 列表：列表生效");
        let error = request(Some(C), &[A, B]).validate().unwrap_err();
        assert!(error.to_string().contains("knowledge_base_ids"), "{error}");
    }

    /// 列表是与 description 互斥的知识库来源；学习图课程仍只走描述流。
    #[test]
    fn multi_kb_list_is_a_knowledge_base_source() {
        let mut with_description = request(None, &[A, B]);
        with_description.description = Some("brief".into());
        assert!(with_description.validate().is_err(), "列表与描述互斥");

        let mut graph = request(None, &[A, B]);
        graph.course_kind = CourseKind::LearningGraph;
        graph.description = Some("学会 Python".into());
        let error = graph.validate().unwrap_err();
        assert!(
            error.to_string().contains("description only"),
            "学习图课程不接库: {error}"
        );
    }

    /// 去重键（生成槽位）：id 排序后拼接——乱序提交同一组库命中同一槽位；
    /// 单库保持 t2 的 `kb:{id}` 形状。
    #[test]
    fn multi_kb_key_is_order_insensitive_and_single_compatible() {
        assert_eq!(request(Some(A), &[]).knowledge_base_key().as_deref(), Some(&*format!("kb:{A}")));
        assert_eq!(
            request(None, &[A, B]).knowledge_base_key().as_deref(),
            Some(&*format!("kb:{A}+{B}"))
        );
        assert_eq!(
            request(None, &[B, A]).knowledge_base_key().as_deref(),
            Some(&*format!("kb:{A}+{B}")),
            "乱序提交同一组库 → 同一槽位"
        );
        assert_eq!(request(None, &[]).knowledge_base_key(), None);
    }
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

#[cfg(test)]
mod est_key_tests {
    //! est 键名统一（j01）的双读契约：导入包新键 `est` 为准，旧键
    //! `estimated_minutes` 是兼容别名——老导入文件不回归（缺省回退才可判）。

    use super::*;

    #[test]
    fn lesson_pack_reads_new_key_and_legacy_alias() {
        let new_key: LessonPack = serde_json::from_str(
            r#"{"title": "t", "est": 42}"#,
        )
        .expect("new key parses");
        assert_eq!(new_key.est, 42);

        let legacy: LessonPack = serde_json::from_str(
            r#"{"title": "t", "estimated_minutes": 25}"#,
        )
        .expect("legacy import packs keep parsing");
        assert_eq!(legacy.est, 25);

        let absent: LessonPack =
            serde_json::from_str(r#"{"title": "t"}"#).expect("absent falls back");
        assert_eq!(absent.est, 10, "缺省 10 不变");
    }
}
