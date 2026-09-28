//! Learning-graph growth engine (ADR-0009): a course is never pre-built as a
//! DAG. It starts from a handful of ENDPOINT ANCHORS (title + a one-line
//! degree statement, stored as marker lessons) and GROWS one small batch of
//! nodes at a time whenever the ready stock drops below the trigger level.
//!
//! Dependencies live entirely in the CONCEPT WEB: every node declares which
//! concepts it `teaches` and which it `assumes`, each at a TIER
//! (know < apply < teach). There are no prerequisite edges and — because the
//! structural gates reject any batch whose assumptions are not already
//! covered — no locked state in the UI: a published node is always ready
//! NOW. Cross-course readiness falls out of the global concept registry.
//!
//! The coach is a SINGLE LLM call (the whole roster + coverage gauge +
//! compass is injected into the prompt; a growth batch is ≤7 nodes, far too
//! small to justify the old multi-round draft loop). Deterministic gates
//! hold the decision power; one repair retry gets the gate report, then an
//! independent AI concept review runs (advisory-but-blocking once, degraded
//! to pass when unavailable).

use std::collections::{HashMap, HashSet};

use nomifun_common::AppError;
use serde::{Deserialize, Serialize};

use crate::completer::LearningCompleter;

/// 就绪节点的补货目标：每次生长把就绪存量补到这个数（learnhub 的「7」——
/// 门与提示词同源的唯一数字，ADR-0009）。
pub(crate) const READY_TARGET: usize = 7;

/// 自动触发的水位线：节点完成时检查，就绪存量低于该值才触发生长。
pub(crate) const READY_TRIGGER: usize = 3;

/// 单批新增节点上限（提示词与门同源，超过整批拒收）。
pub(crate) const MAX_BATCH_NODES: usize = 7;

// ── Tiers ──────────────────────────────────────────────────────────────────

/// 概念掌握档位：知道 < 会用 < 能教。DB 与线上均存 locale 无关代码；中文
/// 展示名（知道/会用/能教）在提示词与 i18n 里各有一份，语义以此处为准。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConceptTier {
    Know,
    Apply,
    Teach,
}

impl ConceptTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Know => "know",
            Self::Apply => "apply",
            Self::Teach => "teach",
        }
    }

    pub fn try_from_str(value: &str) -> Option<Self> {
        match value {
            "know" => Some(Self::Know),
            "apply" => Some(Self::Apply),
            "teach" => Some(Self::Teach),
            _ => None,
        }
    }
}

// ── Proposed batch (raw coach output) ──────────────────────────────────────

/// Coach 声明的一枚概念引用：名字按登记表（canonical 或别名）照抄，档位
/// 为本节点对该概念的要求/教学档位。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConceptRefInput {
    pub name: String,
    pub tier: ConceptTier,
}

/// Coach 提议的一个新节点。名字即标题（动作句），概念引用按名字解析。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedNode {
    pub title: String,
    #[serde(default)]
    pub purpose: String,
    /// 学习分钟预算；常规 5–30，极困难最多 60（门放行 1–60）。
    #[serde(default)]
    pub minutes: Option<u16>,
    #[serde(default)]
    pub teaches: Vec<ConceptRefInput>,
    #[serde(default)]
    pub assumes: Vec<ConceptRefInput>,
}

/// 随批铸名： coach 用到登记表里没有的概念时必须在此注册（canonical+
/// 别名联合唯一，别名可为空）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConceptMint {
    pub canonical: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub definition: String,
}

/// 教练对一条终点锚的裁决：置完成或重开（双向，误标可逆——ADR-0009
/// Amendment 1）。机器只落教练的裁决位，永不自动置位。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointVerdict {
    pub title: String,
    pub completed: bool,
}

/// 一批生长提案：节点 + 铸名 + 教练裁决的终点完成位 + 批注。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedBatch {
    #[serde(default)]
    pub nodes: Vec<ProposedNode>,
    #[serde(default)]
    pub mints: Vec<ConceptMint>,
    #[serde(default)]
    pub completed_endpoints: Vec<EndpointVerdict>,
    #[serde(default)]
    pub note: String,
}

/// 确定性结构门的拒绝项：`kind` 是稳定机器标签（测试与修复回灌按它键控）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateError {
    pub kind: &'static str,
    pub message: String,
}

/// 修复回灌用的全文报告。
pub fn format_gate_report(errors: &[GateError]) -> String {
    errors
        .iter()
        .map(|error| format!("- [{}] {}", error.kind, error.message))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 概念覆盖账：概念名（canonical，按登记表解析别名后）→ 已教最高档位。
pub type ConceptCoverage = HashMap<String, ConceptTier>;

/// 结构门（确定性，零模型调用）。任何一条违反 = 整批拒收（不部分落库），
/// 报告回灌给教练重裁。检查项：
/// - 批量上限（≤ [`MAX_BATCH_NODES`]）与标题合法性/唯一性；
/// - 铸名合法：canonical 非空、与登记表（含别名）及批内其他铸名不撞；
/// - 概念引用可解析：teaches/assumes 的名字必须是登记表现有概念（含别名）
///   或本批铸名；
/// - **就绪即发布**：每条 assumes 必须被（全局已教账 ∪ 本批兄弟 teaches）
///   以不低于所假定档位覆盖——不满足的节点不存在「入库后锁定」，直接打回；
/// - 终点保护：完成裁决的标题必须是真实终点；节点不得冒用终点标题。
pub fn validate_batch(
    batch: &ProposedBatch,
    coverage: &ConceptCoverage,
    registry_names: &HashSet<String>,
    endpoint_titles: &HashSet<String>,
    existing_titles: &HashSet<String>,
) -> Vec<GateError> {
    let mut errors = Vec::new();
    if batch.nodes.len() > MAX_BATCH_NODES {
        errors.push(GateError {
            kind: "batch_too_large",
            message: format!(
                "一批最多 {} 个新节点，本批 {} 个——拆批重交",
                MAX_BATCH_NODES,
                batch.nodes.len()
            ),
        });
    }

    // 批内铸名预备账：canonical → 该概念在本批可引用的名字集合。
    let mut mint_names: HashSet<String> = HashSet::new();
    let mut canonicals: Vec<String> = Vec::new();
    for mint in &batch.mints {
        let canonical = mint.canonical.trim();
        if canonical.is_empty() {
            errors.push(GateError {
                kind: "mint_invalid",
                message: "铸名的 canonical 不能为空".into(),
            });
            continue;
        }
        if registry_names.contains(&canonical.to_lowercase()) {
            errors.push(GateError {
                kind: "mint_collision",
                message: format!("铸名「{canonical}」与概念登记表现有名字冲突"),
            });
        }
        for canonical_before in &canonicals {
            if canonical_before == &canonical.to_lowercase() {
                errors.push(GateError {
                    kind: "mint_collision",
                    message: format!("批内铸名重复：「{canonical}」"),
                });
            }
        }
        canonicals.push(canonical.to_lowercase());
        mint_names.insert(canonical.to_lowercase());
        for alias in &mint.aliases {
            let alias = alias.trim();
            if alias.is_empty() {
                continue;
            }
            if registry_names.contains(&alias.to_lowercase()) || mint_names.contains(&alias.to_lowercase())
            {
                errors.push(GateError {
                    kind: "mint_collision",
                    message: format!("铸名「{alias}」（{canonical} 的别名）与现有名字冲突"),
                });
            }
            mint_names.insert(alias.to_lowercase());
        }
    }

    // 本批兄弟 teaches 账（名字 → 最高档位），供批内自足判定。
    let mut batch_taught: ConceptCoverage = HashMap::new();
    for node in &batch.nodes {
        for concept in &node.teaches {
            let key = concept.name.trim().to_lowercase();
            let entry = batch_taught.entry(key).or_insert(concept.tier);
            if concept.tier > *entry {
                *entry = concept.tier;
            }
        }
    }

    let mut seen_titles: HashSet<String> = HashSet::new();
    for node in &batch.nodes {
        let title = node.title.trim();
        if title.is_empty() {
            errors.push(GateError {
                kind: "node_invalid",
                message: "节点标题不能为空".into(),
            });
            continue;
        }
        if title.chars().count() > 80 {
            errors.push(GateError {
                kind: "node_invalid",
                message: format!("节点标题过长（>80 字）：{title}"),
            });
        }
        let key = title.to_lowercase();
        if !seen_titles.insert(key.clone()) {
            errors.push(GateError {
                kind: "title_duplicate",
                message: format!("批内节点标题重复：{title}"),
            });
        }
        if existing_titles.contains(&key) || endpoint_titles.contains(&key) {
            errors.push(GateError {
                kind: "title_duplicate",
                message: format!("节点标题与课程已有节点/终点重复：{title}"),
            });
        }
        let minutes = node.minutes.unwrap_or(10);
        if !(1..=60).contains(&minutes) {
            errors.push(GateError {
                kind: "node_minutes",
                message: format!("节点 {title} 的分钟预算必须在 1–60 之间：{minutes}"),
            });
        }

        for concept in node.teaches.iter().chain(node.assumes.iter()) {
            let name = concept.name.trim();
            if name.is_empty() {
                errors.push(GateError {
                    kind: "concept_unresolved",
                    message: format!("节点 {title} 的概念引用为空"),
                });
                continue;
            }
            let key = name.to_lowercase();
            let resolvable =
                registry_names.contains(&key) || mint_names.contains(&key) || batch_taught.contains_key(&key);
            if !resolvable {
                errors.push(GateError {
                    kind: "concept_unresolved",
                    message: format!(
                        "节点 {title} 引用的概念「{name}」既不在概念登记表，也未随本批铸名——先铸名或改用登记表中的名字"
                    ),
                });
            }
        }

        for assumption in &node.assumes {
            let key = assumption.name.trim().to_lowercase();
            let covered = coverage
                .get(&key)
                .is_some_and(|taught| *taught >= assumption.tier)
                || batch_taught
                    .get(&key)
                    .is_some_and(|taught| *taught >= assumption.tier);
            if !covered {
                errors.push(GateError {
                    kind: "assumes_uncovered",
                    message: format!(
                        "节点 {title} 假定「{name}」到「{assumed}」档位，但没有任何节点（含本批兄弟）教到该档位——{hint}",
                        name = assumption.name.trim(),
                        assumed = tier_zh(assumption.tier),
                        hint = "先补一个教该概念的铺垫节点，或把本节点的假定降档/移除",
                    ),
                });
            }
        }
    }

    for verdict in &batch.completed_endpoints {
        let key = verdict.title.trim().to_lowercase();
        if !endpoint_titles.contains(&key) {
            errors.push(GateError {
                kind: "endpoint_unknown",
                message: format!(
                    "完成裁决的终点「{title}」不存在——终点保护：只能裁决已声明的终点",
                    title = verdict.title.trim()
                ),
            });
        }
    }

    errors
}

/// 档位的中文显示名（提示词与门消息共用这一份，i18n 另有对应键）。
pub fn tier_zh(tier: ConceptTier) -> &'static str {
    match tier {
        ConceptTier::Know => "知道",
        ConceptTier::Apply => "会用",
        ConceptTier::Teach => "能教",
    }
}

// ── Scope analysis（沿用：目标/基线/范围/大块概念）────────────────────────

/// Scope call: ONE light call that resolves what the goal description
/// actually covers. The output is REFERENCE material for the coach prompt
/// and the endpoint proposal; failure degrades to a scope-free start, never
/// to a hard error.
const SCOPE_SYSTEM: &str = r#"你负责在学习目标开始生长之前，先厘清这个目标到底覆盖什么。
只回复一个 JSON 对象，形状如下：
{
  "goal": "完成整个学习后应达到的最终状态——可检验的能力描述",
  "baseline": "学习者被假定的起点状态",
  "scope": "一句话界定该目标覆盖什么、从哪里开始",
  "blocks": ["大块概念一", "大块概念二"]
}
规则：
- "goal"：明确的学习目标。从学习目标描述中提炼学习者完成后能做到什么、理解到什么程度——写成可检验的能力陈述，而不是重复用户的原话。
- "baseline"：用户起点。当学习者基线不明且没有明确要求起点时，一律视作用户对目标相关领域彻底的一无所知；只有用户明确说出自己已具备的知识或技能时，才照实记录。
- "scope"：一句话划清目标的边界——起点、要达到的水平、主题广度，须与 goal 和 baseline 保持一致。
- "blocks"：该目标真正覆盖的大块概念，按从基础到高级排序，合起来必须铺满从 baseline 到 goal 的整条路径。数量不固定：复杂的目标多列，简单的目标少列。
- 用学习目标的语言书写。
- 只输出 JSON，不要 Markdown 代码块，不要任何解释。"#;

/// Resolved scope reference fed into the coach / endpoint proposal.
#[derive(Debug, Clone, Default)]
pub(crate) struct ScopeAnalysis {
    pub goal: String,
    pub baseline: String,
    pub scope: String,
    pub blocks: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawScope {
    #[serde(default)]
    goal: String,
    #[serde(default)]
    baseline: String,
    #[serde(default)]
    scope: String,
    #[serde(default, deserialize_with = "de_string_list")]
    blocks: Vec<String>,
}

fn parse_scope_reply(raw: &str) -> Option<ScopeAnalysis> {
    let parsed = crate::generation::parse_json_object::<RawScope>(raw).ok()?;
    Some(ScopeAnalysis {
        goal: parsed.goal.trim().to_owned(),
        baseline: parsed.baseline.trim().to_owned(),
        scope: parsed.scope.trim().to_owned(),
        blocks: parsed.blocks,
    })
}

/// One scope call, best-effort: any failure degrades to `None`.
pub(crate) async fn analyze_scope(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    topic: &str,
) -> Option<ScopeAnalysis> {
    let user = format!("Learning goal: {topic}");
    let raw = crate::generation::complete(
        completer,
        model_override,
        SCOPE_SYSTEM,
        &user,
        crate::generation::LEARNING_GRAPH_SCOPE_MAX_TOKENS,
    )
    .await
    .ok()?;
    parse_scope_reply(&raw)
}

// ── Endpoint proposal (course creation) ────────────────────────────────────

/// Coach 提议的一条终点：标题 + 一句程度声明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedEndpoint {
    pub title: String,
    #[serde(default)]
    pub goal_note: String,
}

const ENDPOINT_PROPOSAL_SYSTEM: &str = r#"你负责为一个新的学习目标提议 1-3 条「终点锚」。终点是学习的方向标记：学习者抵达终点时应当具备什么能力。
只回复一个 JSON 对象，形状如下：
{
  "endpoints": [
    { "title": "终点名（名词短语，如：独立完成一元二次方程的求解）", "goal_note": "一句程度声明：抵达时能做到什么、到什么程度" }
  ]
}
规则：
- 每条终点必须指向一个可检验的能力状态，而不是「学完某本书」这类过程描述。
- 目标单一清晰时提议 1 条即可；目标含多个相互独立的努力方向时才拆成多条（最多 3 条）。
- 终点标题用学习目标的语言书写，20 字以内。
- 只输出 JSON，不要 Markdown 代码块，不要任何解释。"#;

/// Propose initial endpoints for a new course (best-effort: failure returns
/// an empty list — the user types endpoints by hand in the wizard).
pub(crate) async fn propose_endpoints(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    topic: &str,
    scope: Option<&ScopeAnalysis>,
) -> Vec<ProposedEndpoint> {
    let mut user = format!("学习目标：{topic}");
    if let Some(scope) = scope {
        if !scope.goal.is_empty() {
            user.push_str(&format!("\n\n目标解析：{}", scope.goal));
        }
        if !scope.blocks.is_empty() {
            user.push_str(&format!("（覆盖：{}）", scope.blocks.join("、")));
        }
    }
    let Ok(raw) = crate::generation::complete(
        completer,
        model_override,
        ENDPOINT_PROPOSAL_SYSTEM,
        &user,
        crate::generation::LEARNING_GRAPH_SCOPE_MAX_TOKENS,
    )
    .await
    else {
        return Vec::new();
    };
    #[derive(Deserialize)]
    struct RawEndpoints {
        #[serde(default, deserialize_with = "de_option_list")]
        endpoints: Vec<RawEndpoint>,
    }
    #[derive(Deserialize)]
    struct RawEndpoint {
        #[serde(default)]
        title: String,
        #[serde(default)]
        goal_note: String,
    }
    crate::generation::parse_json_object::<RawEndpoints>(&raw)
        .ok()
        .map(|parsed| {
            parsed
                .endpoints
                .into_iter()
                .filter_map(|endpoint| {
                    let title = endpoint.title.trim().to_owned();
                    if title.is_empty() {
                        None
                    } else {
                        Some(ProposedEndpoint { title, goal_note: endpoint.goal_note.trim().to_owned() })
                    }
                })
                .take(3)
                .collect()
        })
        .unwrap_or_default()
}

// ── Compass ────────────────────────────────────────────────────────────────

const COMPASS_SYSTEM: &str = r#"你负责为一张正在生长的学习图重画「罗盘」：逐终点展开的剩余路线摘要。
输入是课程的学习目标与全部终点锚（含每条的程度声明），必要时附当前已教概念的清单。
输出 Markdown（不写代码块围栏），结构：
## <终点标题>
- 一句程度声明的复述（该终点抵达时学习者能做什么）
- 剩余路线：3-6 条并列能力条目，每条标注需要的档位（知道/会用/能教）；已明显达成的条目标「✓」
规则：
- 罗盘是方向标尺，不是完成判据：只描述「还差什么」，不评判学习者。
- 每条终点一节，标题与终点锚完全一致；不含终点之外的内容。
- 全文不超过 1200 字。"#;

/// Draw (or redraw) the compass for a course. Failure surfaces as an error —
/// the compass is regenerated in the background and retried on the next
/// endpoint change or growth.
pub(crate) async fn draw_compass(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    goal: &str,
    endpoints: &[(String, String)],
    taught_summary: Option<&str>,
) -> Result<String, AppError> {
    let mut user = format!("学习目标：{goal}\n\n终点锚：");
    if endpoints.is_empty() {
        user.push_str("（暂无）");
    }
    for (title, note) in endpoints {
        user.push_str(&format!("\n- {title}：{note}"));
    }
    if let Some(taught) = taught_summary {
        if !taught.is_empty() {
            user.push_str(&format!("\n\n已教概念：{taught}"));
        }
    }
    crate::generation::complete(
        completer,
        model_override,
        COMPASS_SYSTEM,
        &user,
        crate::generation::COMPASS_MAX_TOKENS,
    )
    .await
}

// ── Coach draft ────────────────────────────────────────────────────────────

/// 教练单次调用的系统提示词。结束条件只有一个数：把就绪补到
/// [`READY_TARGET`] 个；每批上限 [`MAX_BATCH_NODES`] 个。
pub(crate) const COACH_SYSTEM: &str = r#"你是一名学习图教练：课程不会预先铺好大纲，你按批次让课程「生长」——每次补充少量新节点，让学习者永远有一小批当下就能学的节点。
只回复一个 JSON 对象，形状如下：
{
  "nodes": [
    {
      "title": "节点标题（动作句：解/求/证明/推导/构造/比较/应用/辨析…）",
      "purpose": "一句话说明这个节点练什么、为什么现在学",
      "minutes": 20,
      "teaches": [ { "name": "概念名", "tier": "apply" } ],
      "assumes": [ { "name": "概念名", "tier": "know" } ]
    }
  ],
  "mints": [ { "canonical": "新概念名", "aliases": ["别名"], "definition": "一句话定义" } ],
  "completed_endpoints": [],
  "note": "一句话批注：本批为什么长这些节点"
}
【档位】
- know（知道）：能辨认、复述。apply（会用）：能解题、应用。teach（能教）：能讲解、纠错。
- teaches 标本课把该概念教到什么档位；assumes 标本课假定学习者已把该概念掌握到什么档位。
- assumes 必须被已教账（或本批兄弟的 teaches）以不低于所假定档位覆盖，否则整批被打回——不存在的「先锁后面再补」。
【节点纪律】
- 节点是单次学习会话（动作句），常规 5-30 分钟，极困难最多 60 分钟。
- 同一主题可螺旋重现（不同深度、不同动作词），标题不得完全相同。
- 从易到难：新节点优先长在当前就绪前沿的临近难度上，不突兀拔高。
【概念纪律】
- 引用的概念名必须照抄登记表/已教账中的既有名字，或随批 mints 铸名；名字撞车即整批被打回。
- teaches/assumes 宁少勿滥：真正承载本课内容的概念才列入；但 assumes 不得裁剪到失真。
- mints 只登记真正的新概念；与登记表现有概念同义时改用既有名字。
【行为信号】
- 行为摘要给出近窗正确率趋势、卡点节点与真实保留率：生长方向优先回应卡点——在卡点节点附近补铺垫或对比节点，而不是无视它继续铺新域。
- 但不要为单个卡点堆同质节点：一次至多一个针对性的回应节点。
【数量契约】
- 本次生长：把就绪节点补到 7 个。当前就绪 X 个、缺口 7-X 个，则本批新增节点 ≤ 7-X 个且至多 7 个；缺口为 0 时输出空 nodes（或仅裁决终点完成）。
- 终点完成裁决只基于覆盖读数：存量无待办（已学达标+已跳过=全部）才考虑把终点写进 completed_endpoints；已学达标为 0 时零学习证据，绝不构成完成。completed=false 表示重开。拿不准就留空。
【输出】
- 只输出 JSON，不要 Markdown 代码块，不要任何解释。note 与 purpose 用中文。"#;

/// Everything the coach needs, rendered once into the opening user message.
#[derive(Debug, Clone, Default)]
pub struct CoachContext {
    /// 学习目标（scope 解析出的 goal，回退课程标题）。
    pub goal: String,
    /// 基线/范围参考（scope 解析，可为空）。
    pub scope_reference: String,
    /// 罗盘全文（缺罗盘时先重画再生长，因此基本非空）。
    pub compass: String,
    /// 罗盘陈旧读数：罗盘重画之后又落了多少批（>0 时附陈旧标记）。
    pub compass_stale_batches: usize,
    /// 花名册行：「状态 | 标题 | 分钟 | teaches(概念@档) | assumes(概念@档)」；
    /// 卡点节点行带 ⚑ 前缀。
    pub roster: String,
    /// 已教概念账（canonical@最高档 的列表）。
    pub taught_summary: String,
    /// 终点锚清单（标题 + 程度声明）。
    pub endpoints: String,
    /// 覆盖读数（共用存量 met/skipped/pending + 概念档位足迹）。
    pub coverage_gauge: String,
    /// 行为摘要（近窗趋势/卡点/保留率；空窗为空串，整块省略）。
    pub behavior_digest: String,
    /// 当前就绪节点数（缺口 = READY_TARGET − ready）。
    pub ready_count: usize,
    /// 登记表相关切片：与课程已涉概念邻近的名字（防止撞名铸名）。
    pub registry_excerpt: String,
}

impl CoachContext {
    pub(crate) fn render(&self) -> String {
        let gap = READY_TARGET.saturating_sub(self.ready_count);
        let mut text = String::new();
        text.push_str(&format!("【学习目标】\n{}\n", self.goal));
        if !self.scope_reference.is_empty() {
            text.push_str(&format!("\n【范围参考】\n{}\n", self.scope_reference));
        }
        if !self.compass.is_empty() {
            text.push_str(&format!("\n【罗盘】\n{}\n", self.compass));
            if self.compass_stale_batches > 0 {
                text.push_str(&format!(
                    "（注意：该罗盘画于 {} 批之前，可能过时——以下方已教概念账与覆盖读数为当前事实源）\n",
                    self.compass_stale_batches
                ));
            }
        }
        text.push_str(&format!(
            "\n【当前就绪】{ready} 个（目标 7 个，缺口 {gap} 个）\n",
            ready = self.ready_count
        ));
        text.push_str(&format!("\n【花名册】\n{}\n", if self.roster.is_empty() { "（课程还是空的——本批是第一批，从最基础的铺垫节点起步）" } else { &self.roster }));
        text.push_str(&format!(
            "\n【已教概念账】\n{}\n",
            if self.taught_summary.is_empty() { "（空——第一批节点只能 assume 零基可及的概念，或随批铸名后由兄弟节点教）" } else { &self.taught_summary }
        ));
        text.push_str(&format!("\n【终点锚】\n{}\n", if self.endpoints.is_empty() { "（暂无——只长节点，不要裁决任何终点）" } else { &self.endpoints }));
        if !self.coverage_gauge.is_empty() {
            text.push_str(&format!("\n【覆盖读数（存量——完成裁决的依据）】\n{}\n", self.coverage_gauge));
        }
        if !self.behavior_digest.is_empty() {
            text.push_str(&format!("\n【行为摘要】\n{}\n", self.behavior_digest));
        }
        if !self.registry_excerpt.is_empty() {
            text.push_str(&format!("\n【概念登记表（邻近切片，铸名前先查撞名）】\n{}\n", self.registry_excerpt));
        }
        text
    }
}

/// Tolerant raw batch parse (a single string where an array is expected
/// degrades to empty, never fails the whole reply).
#[derive(Debug, Clone, Default, Deserialize)]
struct RawBatch {
    #[serde(default)]
    nodes: Vec<RawNode>,
    #[serde(default)]
    mints: Vec<RawMint>,
    #[serde(default, deserialize_with = "de_endpoint_verdicts")]
    completed_endpoints: Vec<EndpointVerdict>,
    #[serde(default)]
    note: String,
}

/// Tolerate the two verdict shapes the model emits: `{"title","completed"}`
/// objects (documented) or bare title strings (missing field means "mark
/// complete" — the common direction).
fn de_endpoint_verdicts<'de, D>(deserializer: D) -> Result<Vec<EndpointVerdict>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum RawVerdict {
        Title(String),
        Full {
            #[serde(default)]
            title: String,
            #[serde(default = "default_completed")]
            completed: bool,
        },
    }
    fn default_completed() -> bool {
        true
    }
    let parsed = match Option::<Vec<RawVerdict>>::deserialize(deserializer)? {
        Some(verdicts) => verdicts,
        None => Vec::new(),
    };
    Ok(parsed
        .into_iter()
        .filter_map(|verdict| match verdict {
            RawVerdict::Title(title) => {
                let title = title.trim().to_owned();
                (!title.is_empty()).then(|| EndpointVerdict { title, completed: true })
            }
            RawVerdict::Full { title, completed } => {
                let title = title.trim().to_owned();
                (!title.is_empty()).then(|| EndpointVerdict { title, completed })
            }
        })
        .collect())
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawNode {
    #[serde(default)]
    title: String,
    #[serde(default)]
    purpose: String,
    #[serde(default, deserialize_with = "de_optional_minutes")]
    minutes: Option<u16>,
    #[serde(default, deserialize_with = "de_optional_concepts")]
    teaches: Vec<ConceptRefInput>,
    #[serde(default, deserialize_with = "de_optional_concepts")]
    assumes: Vec<ConceptRefInput>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawMint {
    #[serde(default)]
    canonical: String,
    #[serde(default, deserialize_with = "de_string_list")]
    aliases: Vec<String>,
    #[serde(default)]
    definition: String,
}

/// Tolerate `"minutes": "15"` / absence / garbage.
fn de_optional_minutes<'de, D>(deserializer: D) -> Result<Option<u16>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Num(u16),
        Str(String),
    }
    Ok(match Option::<NumOrStr>::deserialize(deserializer)? {
        Some(NumOrStr::Num(n)) => Some(n),
        Some(NumOrStr::Str(s)) => s.trim().parse::<u16>().ok(),
        None => None,
    })
}

/// Tolerate a single `{"name","tier"}` object where an array is expected.
fn de_optional_concepts<'de, D>(deserializer: D) -> Result<Vec<ConceptRefInput>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(RawConceptRef),
        Many(Vec<RawConceptRef>),
    }
    #[derive(Deserialize)]
    struct RawConceptRef {
        #[serde(default)]
        name: String,
        tier: RawTier,
    }
    // Tier arrives in any of the known shapes; an unknown value degrades to
    // `know` (the lowest bar) rather than failing the batch — the gates
    // re-check coverage either way.
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum RawTier {
        Code(String),
    }
    fn normalize_tier(raw: &str) -> Option<ConceptTier> {
        match raw.trim().to_lowercase().as_str() {
            "know" | "知道" => Some(ConceptTier::Know),
            "apply" | "会用" => Some(ConceptTier::Apply),
            "teach" | "能教" => Some(ConceptTier::Teach),
            _ => None,
        }
    }
    let parsed = match Option::<OneOrMany>::deserialize(deserializer)? {
        Some(OneOrMany::One(one)) => vec![one],
        Some(OneOrMany::Many(many)) => many,
        None => Vec::new(),
    };
    Ok(parsed
        .into_iter()
        .filter(|reference| !reference.name.trim().is_empty())
        .map(|reference| {
            let tier_text = match &reference.tier {
                RawTier::Code(code) => code.as_str(),
            };
            ConceptRefInput {
                name: reference.name.trim().to_owned(),
                tier: normalize_tier(tier_text).unwrap_or(ConceptTier::Know),
            }
        })
        .collect())
}

fn de_string_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match Option::<OneOrMany>::deserialize(deserializer)? {
        Some(OneOrMany::One(one)) => vec![one],
        Some(OneOrMany::Many(many)) => many,
        None => Vec::new(),
    })
}

fn de_option_list<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

/// Parse the coach reply; tolerant of the usual model habits. An
/// unparseable reply is an error (the caller retries once with a repair
/// prompt, then gives up).
pub(crate) fn parse_coach_reply(raw: &str) -> Result<ProposedBatch, AppError> {
    let parsed = crate::generation::parse_json_object::<RawBatch>(raw).map_err(|error| {
        AppError::Internal(format!("unparseable coach reply: {error}"))
    })?;
    let batch = ProposedBatch {
        nodes: parsed
            .nodes
            .into_iter()
            .filter(|node| !node.title.trim().is_empty())
            .map(|node| ProposedNode {
                title: node.title.trim().to_owned(),
                purpose: node.purpose.trim().to_owned(),
                minutes: node.minutes,
                teaches: node.teaches,
                assumes: node.assumes,
            })
            .collect(),
        mints: parsed
            .mints
            .into_iter()
            .filter(|mint| !mint.canonical.trim().is_empty())
            .map(|mint| ConceptMint {
                canonical: mint.canonical.trim().to_owned(),
                aliases: mint
                    .aliases
                    .into_iter()
                    .map(|alias| alias.trim().to_owned())
                    .filter(|alias| !alias.is_empty())
                    .collect(),
                definition: mint.definition.trim().to_owned(),
            })
            .collect(),
        completed_endpoints: parsed.completed_endpoints,
        note: parsed.note.trim().to_owned(),
    };
    Ok(batch)
}

/// One coach call → a proposed batch.
pub(crate) async fn coach_draft(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    context: &CoachContext,
) -> Result<ProposedBatch, AppError> {
    let raw = crate::generation::complete(
        completer,
        model_override,
        COACH_SYSTEM,
        &context.render(),
        crate::generation::COACH_MAX_TOKENS,
    )
    .await?;
    parse_coach_reply(&raw)
}

/// One repair call: the gate report (or review problems) is the only input
/// on top of the original context — the coach re-emits the WHOLE batch,
/// fixed.
pub(crate) async fn coach_redraft(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    context: &CoachContext,
    problems: &str,
) -> Result<ProposedBatch, AppError> {
    let user = format!(
        "{}\n\n【上一批被打回，以下问题必须逐条修复后重新输出完整批次】\n{problems}",
        context.render()
    );
    let raw = crate::generation::complete(
        completer,
        model_override,
        COACH_SYSTEM,
        &user,
        crate::generation::COACH_MAX_TOKENS,
    )
    .await?;
    parse_coach_reply(&raw)
}

// ── AI concept review (advisory-but-blocking once) ─────────────────────────

/// 概念评审的一条问题：定位（节点/字段）+ 描述 + 建议。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewProblem {
    pub node: String,
    pub field: String,
    pub issue: String,
    #[serde(default)]
    pub advice: String,
}

const CONCEPT_REVIEW_SYSTEM: &str = r#"你是一名概念评审员：在结构门之后对生长批次做最后一次语义兜底审查。只检查结构门查不了的四类问题：
1. 挂号充分：节点标题/说明里出现的每个领域名词，要么已教、要么被假定、要么随批铸名——不应有凭空冒用却无出处的概念。
2. 前提足够：assumes 是否真的足以支撑节点内容（缺关键前提 = 打回）。
3. 切分原子：节点是否是一个可单次完成的学习会话，而非复合大杂烩。
4. 命名一致：概念名与既有名字是否指同一物（同义不同名应复用既有名）。
回复严格 JSON：
- 通过：{ "verdict": "pass" }
- 打回：{ "verdict": "reject", "problems": [ { "node": "节点标题", "field": "teaches|assumes|mints|title", "issue": "问题描述", "advice": "修改建议" } ] }
最多 8 条问题，按严重度降序。只输出 JSON，不要任何解释。"#;

/// Run the AI concept review over a gated batch. Any failure (no completer,
/// call error, unparseable reply) degrades to PASS — the review never blocks
/// a structurally valid batch when it cannot run.
pub(crate) async fn concept_review(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    batch: &ProposedBatch,
    context: &CoachContext,
) -> Vec<ReviewProblem> {
    #[derive(Deserialize)]
    struct RawVerdict {
        #[serde(default)]
        verdict: String,
        #[serde(default)]
        problems: Vec<ReviewProblem>,
    }
    let batch_json = serde_json::to_string_pretty(batch).unwrap_or_default();
    let user = format!(
        "{}\n\n【待审批次】\n{batch_json}",
        context.render()
    );
    let raw = match crate::generation::complete(
        completer,
        model_override,
        CONCEPT_REVIEW_SYSTEM,
        &user,
        crate::generation::CONCEPT_REVIEW_MAX_TOKENS,
    )
    .await
    {
        Ok(raw) => raw,
        Err(_) => return Vec::new(),
    };
    crate::generation::parse_json_object::<RawVerdict>(&raw)
        .map(|verdict| {
            if verdict.verdict == "pass" {
                Vec::new()
            } else {
                verdict.problems.into_iter().take(8).collect()
            }
        })
        .unwrap_or_default()
}

/// Render the review problems for the redraft prompt.
pub fn format_review_problems(problems: &[ReviewProblem]) -> String {
    problems
        .iter()
        .map(|problem| {
            format!(
                "- [{field}] {node}: {issue}（建议：{advice}）",
                field = problem.field,
                node = problem.node,
                issue = problem.issue,
                advice = problem.advice,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Ready-set (pure) ───────────────────────────────────────────────────────

/// 生长任务的后台输入包：课程、触发者与可选模型偏好。
#[derive(Debug, Clone)]
pub struct GrowthRunner {
    pub course_id: nomifun_common::LearningCourseId,
    pub user_id: nomifun_common::UserId,
    pub model_override: Option<(nomifun_common::ProviderId, String)>,
}

impl GrowthRunner {
    pub fn model_override(&self) -> Option<(&nomifun_common::ProviderId, &str)> {
        self.model_override.as_ref().map(|(provider, model)| (provider, model.as_str()))
    }
}

/// One course node as the readiness predicate sees it.
#[derive(Debug, Clone)]
pub struct ReadyCandidate {
    pub lesson_id: String,
    pub title: String,
    /// assumes (concept_id, tier) resolved against the registry.
    pub assumes: Vec<(String, ConceptTier)>,
    /// True when the learner already satisfied the node (completed/skipped).
    pub satisfied: bool,
}

/// Fold the global concept web into the taught ledger: concept_id → highest
/// tier any lesson (in any course) teaches it at.
pub fn taught_ledger(
    teaches: impl IntoIterator<Item = (String, ConceptTier)>,
) -> ConceptCoverage {
    let mut ledger: ConceptCoverage = HashMap::new();
    for (concept_id, tier) in teaches {
        let entry = ledger.entry(concept_id).or_insert(tier);
        if tier > *entry {
            *entry = tier;
        }
    }
    ledger
}

/// The ready set: unsatisfied candidates whose every assumption is covered
/// by the (cross-course) taught ledger at the assumed tier or above.
/// Order follows the input (lesson position), which doubles as the
/// recommendation order.
pub fn ready_set(candidates: &[ReadyCandidate], ledger: &ConceptCoverage) -> Vec<String> {
    candidates
        .iter()
        .filter(|candidate| {
            !candidate.satisfied
                && candidate.assumes.iter().all(|(concept_id, tier)| {
                    ledger.get(concept_id).is_some_and(|taught| taught >= tier)
                })
        })
        .map(|candidate| candidate.lesson_id.clone())
        .collect()
}

// ── Behavior digest & coverage gauge (pure folds, ADR-0009 Amendment 1) ───

/// 行为摘要窗口：最近 7 个学习日或 10 个节点，双满足才收窗（learnhub
/// DIGEST_WINDOW 同款——窗口太小看不见趋势，太大稀释近期信号）。
pub const DIGEST_WINDOW_DAYS: usize = 7;
pub const DIGEST_WINDOW_NODES: usize = 10;

/// 折叠进行为摘要的一次作答（service 侧已联到课时并换算学习日）。
#[derive(Debug, Clone)]
pub struct DigestAttempt {
    pub lesson_id: String,
    /// 学习日（本地 02:00 翻日的 YYYYMMDD，与复习调度同口径）。
    pub day: i64,
    pub correct: bool,
}

/// 折叠进行为摘要的一次到期推进（真实保留率口径：auto/self 评分，
/// rating≥2 记成功、1 记失败）。
#[derive(Debug, Clone, Copy)]
pub struct DigestPush {
    pub day: i64,
    pub passed: bool,
}

/// 一条卡点节点：窗内答错集中 + 停滞天数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestBlocker {
    pub lesson_id: String,
    pub wrong_count: usize,
    /// 今天距最近一次答对（从未答对则距首次作答）的学习日数。
    pub stagnant_days: i64,
}

/// 正确率趋势：前后半窗各至少 2 次作答才出方向，否则样本不足。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AccuracyTrend {
    Up,
    Down,
    Flat,
    NotEnough,
}

/// 行为摘要折叠结果（service 渲染进教练上下文）。
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorDigest {
    pub window_days: usize,
    pub window_nodes: usize,
    /// 窗口超过 7 学习日（10 节点条款生效）。
    pub extended: bool,
    pub first_half: Option<(usize, f64)>,
    pub second_half: Option<(usize, f64)>,
    pub trend: AccuracyTrend,
    pub blockers: Vec<DigestBlocker>,
    pub retention: Option<(usize, usize)>,
}

impl BehaviorDigest {
    /// 渲染进教练上下文的文本；空窗返回空串（整块省略）。
    pub fn render(&self, lesson_titles: &HashMap<String, String>) -> String {
        if self.window_nodes == 0 {
            return String::new();
        }
        let mut lines = Vec::new();
        let window_note = if self.extended {
            format!(
                "窗口：最近 {} 个学习日 / {} 个节点（超出 7 日，双条款生效）",
                self.window_days, self.window_nodes
            )
        } else {
            format!("窗口：最近 {} 个学习日 / {} 个节点", self.window_days, self.window_nodes)
        };
        lines.push(window_note);
        match (self.first_half, self.second_half, self.trend) {
            (Some((n1, a1)), Some((n2, a2)), trend) => {
                let direction = match trend {
                    AccuracyTrend::Up => "上升",
                    AccuracyTrend::Down => "下降",
                    AccuracyTrend::Flat => "持平",
                    AccuracyTrend::NotEnough => "样本不足",
                };
                lines.push(format!(
                    "- 正确率趋势：{direction}（前半窗 {:.0}%，{} 题 → 后半窗 {:.0}%，{} 题）",
                    a1 * 100.0,
                    n1,
                    a2 * 100.0,
                    n2
                ));
            }
            _ => lines.push("- 正确率趋势：样本不足".to_owned()),
        }
        if self.blockers.is_empty() {
            lines.push("- 卡点节点：无".to_owned());
        } else {
            let items = self
                .blockers
                .iter()
                .map(|blocker| {
                    let title = lesson_titles
                        .get(&blocker.lesson_id)
                        .map(String::as_str)
                        .unwrap_or(&blocker.lesson_id);
                    format!(
                        "「{title}」停滞 {days} 天（窗内答错 {wrong} 次）",
                        days = blocker.stagnant_days,
                        wrong = blocker.wrong_count
                    )
                })
                .collect::<Vec<_>>()
                .join("、");
            lines.push(format!("- 卡点节点：{items}"));
        }
        match self.retention {
            Some((passes, fails)) if passes + fails > 0 => {
                let rate = passes as f64 / (passes + fails) as f64;
                lines.push(format!(
                    "- 真实保留率：{:.0}%（{} / {} 次到期推进）",
                    rate * 100.0,
                    passes,
                    passes + fails
                ));
            }
            _ => lines.push("- 真实保留率：窗口内无到期推进".to_owned()),
        }
        lines.join("\n")
    }
}

/// 折叠行为摘要。`stagnation_base` 给出每个课时「停滞从哪天起算」：
/// 最近一次答对的学习日，从未答对则首次作答日（service 侧全历史查询）。
/// `today` 是当前学习日。
pub fn fold_behavior_digest(
    attempts: &[DigestAttempt],
    pushes: &[DigestPush],
    today: i64,
    stagnation_base: &HashMap<String, i64>,
) -> BehaviorDigest {
    // 窗口：学习日降序累计，直到同时满足 7 日且 10 节（learnhub 双条款）。
    let mut days_desc: Vec<i64> = Vec::new();
    let mut nodes: HashSet<&str> = HashSet::new();
    let mut extended = false;
    let mut day_set: Vec<i64> = attempts.iter().map(|attempt| attempt.day).collect();
    day_set.sort_unstable();
    day_set.dedup();
    day_set.reverse();
    for &day in &day_set {
        days_desc.push(day);
        for attempt in attempts.iter().filter(|attempt| attempt.day == day) {
            nodes.insert(attempt.lesson_id.as_str());
        }
        if days_desc.len() >= DIGEST_WINDOW_DAYS && nodes.len() >= DIGEST_WINDOW_NODES {
            extended = days_desc.len() > DIGEST_WINDOW_DAYS;
            break;
        }
    }
    if days_desc.is_empty() {
        return BehaviorDigest {
            window_days: 0,
            window_nodes: 0,
            extended: false,
            first_half: None,
            second_half: None,
            trend: AccuracyTrend::NotEnough,
            blockers: Vec::new(),
            retention: None,
        };
    }
    let window: HashSet<i64> = days_desc.iter().copied().collect();
    let in_window: Vec<&DigestAttempt> = attempts
        .iter()
        .filter(|attempt| window.contains(&attempt.day))
        .collect();

    // 正确率趋势：按学习日升序对半分窗比较。
    let mut days_asc = days_desc.clone();
    days_asc.reverse();
    let half = days_asc.len() / 2;
    let (first_days, second_days) = days_asc.split_at(half);
    let half_accuracy = |days: &[i64]| {
        let picked: Vec<&DigestAttempt> = in_window
            .iter()
            .copied()
            .filter(|attempt| days.contains(&attempt.day))
            .collect();
        let total = picked.len();
        let correct = picked.iter().filter(|attempt| attempt.correct).count();
        (
            total,
            if total == 0 {
                0.0
            } else {
                correct as f64 / total as f64
            },
        )
    };
    let (first_half, second_half) = (half_accuracy(first_days), half_accuracy(second_days));
    let trend = match (first_half, second_half) {
        ((n1, a1), (n2, a2)) if n1 >= 2 && n2 >= 2 => {
            if a2 - a1 > 0.05 {
                AccuracyTrend::Up
            } else if a1 - a2 > 0.05 {
                AccuracyTrend::Down
            } else {
                AccuracyTrend::Flat
            }
        }
        _ => AccuracyTrend::NotEnough,
    };

    // 卡点节点：窗内答错计数 + 停滞天数，按错次降序取前 5。
    let mut wrong_by_lesson: HashMap<&str, usize> = HashMap::new();
    for attempt in &in_window {
        if !attempt.correct {
            *wrong_by_lesson.entry(attempt.lesson_id.as_str()).or_default() += 1;
        }
    }
    let mut blockers: Vec<DigestBlocker> = wrong_by_lesson
        .into_iter()
        .map(|(lesson_id, wrong_count)| DigestBlocker {
            lesson_id: lesson_id.to_owned(),
            wrong_count,
            stagnant_days: (today - stagnation_base.get(lesson_id).copied().unwrap_or(today)).max(0),
        })
        .collect();
    blockers.sort_by(|a, b| {
        b.wrong_count
            .cmp(&a.wrong_count)
            .then(b.stagnant_days.cmp(&a.stagnant_days))
            .then(a.lesson_id.cmp(&b.lesson_id))
    });
    blockers.truncate(5);

    // 真实保留率：窗内到期推进的通过占比。
    let pushes_in_window: Vec<&DigestPush> = pushes
        .iter()
        .filter(|push| window.contains(&push.day))
        .collect();
    let passes = pushes_in_window.iter().filter(|push| push.passed).count();
    let retention = if pushes_in_window.is_empty() {
        None
    } else {
        Some((passes, pushes_in_window.len() - passes))
    };

    BehaviorDigest {
        window_days: days_desc.len(),
        window_nodes: nodes.len(),
        extended,
        first_half: Some(first_half),
        second_half: Some(second_half),
        trend,
        blockers,
        retention,
    }
}

/// 终点覆盖读数（共用存量，ADR-0009 Amendment 1）：所有终点共用同一份
/// 「已学达标 / 已跳过 / 未达标」+ 概念档位足迹。完成裁决的决断输入，
/// 永不自动置位完成。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoverageReading {
    pub total: usize,
    pub met: usize,
    pub skipped: usize,
    /// 未达标节点的标题（渲染时截断）。
    pub pending_titles: Vec<String>,
    /// 课程已教概念的档位足迹（档位降序，只收非零档）。
    pub tier_footprint: Vec<(ConceptTier, usize)>,
}

/// 渲染覆盖读数；存量清空时给出「继续生长 or 标记完成」的决断提示
/// （learnhub cleared 同款语义——决断输入，非完成判据）。
pub fn render_coverage_gauge(reading: &CoverageReading) -> String {
    let pending = reading.total.saturating_sub(reading.met + reading.skipped);
    let footprint = if reading.tier_footprint.is_empty() {
        "概念足迹 0 枚".to_owned()
    } else {
        let items = reading
            .tier_footprint
            .iter()
            .map(|(tier, count)| format!("{} {}", tier_zh(*tier), count))
            .collect::<Vec<_>>()
            .join(" / ");
        format!(
            "概念足迹 {} 枚（{items}）",
            reading.tier_footprint.iter().map(|(_, count)| count).sum::<usize>()
        )
    };
    if pending == 0 {
        return format!(
            "存量无待办（已学达标 {}、已跳过 {}；跳过≠学会；{footprint}）——考虑继续生长新节点，或把覆盖了这些内容的终点标记完成（已学达标为 0 时不构成完成证据）。",
            reading.met, reading.skipped
        );
    }
    let pending_list = if reading.pending_titles.is_empty() {
        String::new()
    } else {
        let titles: Vec<String> = reading
            .pending_titles
            .iter()
            .take(8)
            .map(|title| format!("「{}」", title.trim()))
            .collect();
        let more = reading
            .pending_titles
            .len()
            .saturating_sub(8);
        if more > 0 {
            format!("未达标：{} 等 {more} 个", titles.join("、"))
        } else {
            format!("未达标：{}", titles.join("、"))
        }
    };
    format!(
        "存量 {total}：已学达标 {met}、已跳过 {skipped}、未达标 {pending}{list}｜{footprint}",
        total = reading.total,
        met = reading.met,
        skipped = reading.skipped,
        pending = pending,
        list = if pending_list.is_empty() {
            String::new()
        } else {
            format!("（{pending_list}）")
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(name: &str, tier: ConceptTier) -> ConceptRefInput {
        ConceptRefInput { name: name.to_owned(), tier }
    }

    fn node(title: &str, teaches: Vec<ConceptRefInput>, assumes: Vec<ConceptRefInput>) -> ProposedNode {
        ProposedNode { title: title.to_owned(), purpose: String::new(), minutes: Some(20), teaches, assumes }
    }

    fn batch_of(nodes: Vec<ProposedNode>) -> ProposedBatch {
        ProposedBatch { nodes, mints: Vec::new(), completed_endpoints: Vec::new(), note: String::new() }
    }

    #[test]
    fn tier_ordering_is_know_apply_teach() {
        assert!(ConceptTier::Know < ConceptTier::Apply);
        assert!(ConceptTier::Apply < ConceptTier::Teach);
        assert_eq!(ConceptTier::try_from_str("apply"), Some(ConceptTier::Apply));
        assert_eq!(ConceptTier::try_from_str("nope"), None);
    }

    #[test]
    fn validate_batch_passes_a_self_sufficient_batch() {
        // 因式分解 is minted AND taught by the first node; the second node
        // assumes it at a tier the sibling covers.
        let batch = ProposedBatch {
            nodes: vec![
                node("用提公因式法化简多项式", vec![reference("因式分解", ConceptTier::Know)], vec![]),
                node("用公式法解因式分解题", vec![reference("公式法", ConceptTier::Know)], vec![reference("因式分解", ConceptTier::Know)]),
            ],
            mints: vec![ConceptMint { canonical: "因式分解".into(), aliases: vec!["因式拆解".into()], definition: String::new() }],
            completed_endpoints: vec![],
            note: String::new(),
        };
        let errors = validate_batch(&batch, &ConceptCoverage::new(), &HashSet::new(), &HashSet::new(), &HashSet::new());
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn validate_batch_rejects_uncovered_assumption_instead_of_locking() {
        // 会用-level assumption with only a 知道-level taught coverage: the
        // gate bounces the whole batch (no locked state may ever publish).
        // 该概念已在登记表（生产不变量：coverage 内的概念必然已登记），
        // 唯一打回理由就是假定未被覆盖。
        let batch = batch_of(vec![node(
            "应用勾股定理解题",
            vec![],
            vec![reference("勾股定理", ConceptTier::Apply)],
        )]);
        let mut coverage = ConceptCoverage::new();
        coverage.insert("勾股定理".to_owned(), ConceptTier::Know);
        let mut registry = HashSet::new();
        registry.insert("勾股定理".to_owned());
        let errors = validate_batch(&batch, &coverage, &registry, &HashSet::new(), &HashSet::new());
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, "assumes_uncovered");

        // Same batch passes when some node teaches the concept to the
        // assumed tier (batch self-sufficiency).
        let batch = batch_of(vec![
            node("推导勾股定理", vec![reference("勾股定理", ConceptTier::Apply)], vec![]),
            node("应用勾股定理解题", vec![], vec![reference("勾股定理", ConceptTier::Apply)]),
        ]);
        let errors = validate_batch(&batch, &ConceptCoverage::new(), &HashSet::new(), &HashSet::new(), &HashSet::new());
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn validate_batch_enforces_batch_size_title_and_endpoint_protection() {
        let nodes: Vec<ProposedNode> = (0..8)
            .map(|index| node(&format!("节点{index}"), vec![], vec![]))
            .collect();
        let errors = validate_batch(
            &batch_of(nodes),
            &ConceptCoverage::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(errors.iter().any(|error| error.kind == "batch_too_large"));

        let mut existing = HashSet::new();
        existing.insert("已有节点".to_owned());
        let errors = validate_batch(
            &batch_of(vec![node("已有节点", vec![], vec![])]),
            &ConceptCoverage::new(),
            &HashSet::new(),
            &HashSet::new(),
            &existing,
        );
        assert!(errors.iter().any(|error| error.kind == "title_duplicate"));

        let mut endpoints = HashSet::new();
        endpoints.insert("真实终点".to_owned());
        let mut batch = batch_of(vec![node("真实终点", vec![], vec![])]);
        batch.completed_endpoints = vec![EndpointVerdict {
            title: "不存在的终点".into(),
            completed: true,
        }];
        let errors = validate_batch(&batch, &ConceptCoverage::new(), &HashSet::new(), &endpoints, &HashSet::new());
        assert!(errors.iter().any(|error| error.kind == "endpoint_unknown"));
        assert!(errors.iter().any(|error| error.kind == "title_duplicate"));
    }

    #[test]
    fn validate_batch_rejects_unresolved_concepts_and_mint_collisions() {
        let mut registry = HashSet::new();
        registry.insert("既有概念".to_owned());
        let batch = ProposedBatch {
            nodes: vec![node("新节点", vec![reference("既有概念", ConceptTier::Know)], vec![reference("幽灵概念", ConceptTier::Know)])],
            mints: vec![ConceptMint { canonical: "既有概念".into(), aliases: vec![], definition: String::new() }],
            completed_endpoints: vec![],
            note: String::new(),
        };
        let errors = validate_batch(&batch, &ConceptCoverage::new(), &registry, &HashSet::new(), &HashSet::new());
        assert!(errors.iter().any(|error| error.kind == "mint_collision"));
        assert!(errors.iter().any(|error| error.kind == "concept_unresolved"));
    }

    #[test]
    fn ready_set_requires_tier_coverage_and_skips_satisfied_nodes() {
        let candidates = vec![
            ReadyCandidate {
                lesson_id: "uncovered".into(),
                title: "n1".into(),
                assumes: vec![("c1".into(), ConceptTier::Apply)],
                satisfied: false,
            },
            ReadyCandidate {
                lesson_id: "covered".into(),
                title: "n2".into(),
                assumes: vec![("c1".into(), ConceptTier::Know)],
                satisfied: false,
            },
            ReadyCandidate {
                lesson_id: "done".into(),
                title: "n3".into(),
                assumes: vec![],
                satisfied: true,
            },
        ];
        let mut ledger = taught_ledger(vec![("c1".to_owned(), ConceptTier::Know)]);
        assert_eq!(ready_set(&candidates, &ledger), vec!["covered".to_owned()]);
        // Cross-course growth: a higher tier taught elsewhere lifts coverage.
        ledger.insert("c1".to_owned(), ConceptTier::Teach);
        assert_eq!(
            ready_set(&candidates, &ledger),
            vec!["uncovered".to_owned(), "covered".to_owned()]
        );
    }

    #[test]
    fn parse_coach_reply_tolerates_string_minutes_and_chinese_tiers() {
        let raw = r#"{
            "nodes": [
                { "title": "用配方法解方程", "purpose": "p", "minutes": "15",
                  "teaches": [{"name": "配方法", "tier": "会用"}],
                  "assumes": [{"name": "一元二次方程", "tier": "知道"}] }
            ],
            "mints": [],
            "completed_endpoints": [],
            "note": "n"
        }"#;
        let batch = parse_coach_reply(raw).unwrap();
        assert_eq!(batch.nodes.len(), 1);
        assert_eq!(batch.nodes[0].minutes, Some(15));
        assert_eq!(batch.nodes[0].teaches[0].tier, ConceptTier::Apply);
        assert_eq!(batch.nodes[0].assumes[0].tier, ConceptTier::Know);
    }

    #[test]
    fn parse_coach_reply_rejects_garbage() {
        assert!(parse_coach_reply("sure, here is the plan...").is_err());
    }

    /// 终点裁决的两种容错形状：文档化的 {title, completed} 对象与裸标题
    /// 字符串（缺字段视为置完成——常见方向）。
    #[test]
    fn parse_coach_reply_tolerates_verdict_shapes() {
        let raw = r#"{
            "nodes": [],
            "mints": [],
            "completed_endpoints": [
                { "title": "独立交易", "completed": true },
                { "title": "读懧行情", "completed": false },
                "裸标题终点"
            ],
            "note": ""
        }"#;
        let batch = parse_coach_reply(raw).unwrap();
        assert_eq!(
            batch.completed_endpoints,
            vec![
                EndpointVerdict { title: "独立交易".into(), completed: true },
                EndpointVerdict { title: "读懧行情".into(), completed: false },
                EndpointVerdict { title: "裸标题终点".into(), completed: true },
            ]
        );
    }

    fn attempt(lesson: &str, day: i64, correct: bool) -> DigestAttempt {
        DigestAttempt { lesson_id: lesson.to_owned(), day, correct }
    }

    /// 窗口双条款：7 学习日或 10 节点取大；扩展窗口标 extended。
    #[test]
    fn behavior_digest_window_takes_the_larger_clause() {
        // 5 个学习日、20 个节点 → 节点条款生效，5 日全收。
        let mut attempts = Vec::new();
        for day in 1..=5 {
            for index in 0..4 {
                attempts.push(attempt(&format!("n{day}-{index}"), day, true));
            }
        }
        let digest = fold_behavior_digest(&attempts, &[], 6, &HashMap::new());
        assert_eq!(digest.window_nodes, 20);
        assert_eq!(digest.window_days, 5);
        assert!(!digest.extended);

        // 12 个学习日、每天 1 个节点 → 日条款先满，第 10 日收窗，extended。
        let attempts: Vec<DigestAttempt> = (1..=12)
            .map(|day| attempt(&format!("n{day}"), day, true))
            .collect();
        let digest = fold_behavior_digest(&attempts, &[], 13, &HashMap::new());
        assert_eq!(digest.window_days, 10);
        assert_eq!(digest.window_nodes, 10);
        assert!(digest.extended);
    }

    /// 趋势 + 卡点 + 保留率的主体折叠。
    #[test]
    fn behavior_digest_folds_trend_blockers_and_retention() {
        let mut attempts = Vec::new();
        // 前半窗（日 1-2）全对，后半窗（日 3-4）全错 → 下降。
        for day in 1..=2 {
            for index in 0..5 {
                attempts.push(attempt(&format!("n{day}-{index}"), day, true));
            }
        }
        for day in 3..=4 {
            for index in 1..=5 {
                attempts.push(attempt(&format!("n{day}-{index}"), day, false));
            }
        }
        // 卡点：n3-1 停滞 5 天（最近答对从未发生——基线取首次作答日 3，今天 8）。
        let digest = fold_behavior_digest(
            &attempts,
            &[DigestPush { day: 3, passed: true }, DigestPush { day: 4, passed: false }],
            8,
            &HashMap::from([("n3-1".to_owned(), 3)]),
        );
        assert_eq!(digest.trend, AccuracyTrend::Down);
        assert!(digest.blockers.iter().any(|blocker| blocker.lesson_id == "n3-1"));
        assert_eq!(digest.retention, Some((1, 1)));
        let rendered = digest.render(&HashMap::new());
        assert!(rendered.contains("下降"), "{rendered}");
        assert!(rendered.contains("真实保留率：50%"), "{rendered}");
        assert!(rendered.contains("停滞 5 天"), "{rendered}");
    }

    /// 空窗（无作答）渲染为空串，整块省略。
    #[test]
    fn behavior_digest_empty_window_renders_empty() {
        let digest = fold_behavior_digest(&[], &[], 100, &HashMap::new());
        assert!(digest.render(&HashMap::new()).is_empty());
    }

    /// 覆盖读数：存量构成 + 档位足迹 + 清空态的决断提示。
    #[test]
    fn coverage_gauge_renders_shared_stock_and_cleared_state() {
        let reading = CoverageReading {
            total: 12,
            met: 4,
            skipped: 2,
            pending_titles: (0..10).map(|index| format!("节点{index}")).collect(),
            tier_footprint: vec![(ConceptTier::Apply, 5), (ConceptTier::Know, 3)],
        };
        let text = render_coverage_gauge(&reading);
        assert!(text.contains("存量 12：已学达标 4、已跳过 2、未达标 6"), "{text}");
        assert!(text.contains("未达标：「节点0」"), "{text}");
        assert!(text.contains("等 2 个"), "10 个只列 8 个: {text}");
        assert!(text.contains("会用 5"), "{text}");
        assert!(text.contains("知道 3"), "{text}");

        let cleared = render_coverage_gauge(&CoverageReading {
            total: 5,
            met: 4,
            skipped: 1,
            pending_titles: vec![],
            tier_footprint: vec![],
        });
        assert!(cleared.contains("存量无待办"), "{cleared}");
        assert!(cleared.contains("已学达标为 0 时不构成完成证据"), "{cleared}");
    }

    /// 罗盘陈旧标记：重画后又落了批时，渲染块附陈旧提示。
    #[test]
    fn coach_context_marks_stale_compass() {
        let fresh = CoachContext {
            ready_count: 2,
            compass: "罗盘正文".into(),
            compass_stale_batches: 0,
            ..Default::default()
        };
        assert!(!fresh.render().contains("可能过时"));
        let stale = CoachContext {
            ready_count: 2,
            compass: "罗盘正文".into(),
            compass_stale_batches: 3,
            ..Default::default()
        };
        let rendered = stale.render();
        assert!(rendered.contains("画于 3 批之前"), "{rendered}");
        assert!(rendered.contains("已教概念账与覆盖读数为当前事实源"), "{rendered}");
    }

    #[test]
    fn coach_context_renders_the_gap_contract() {
        let context = CoachContext { ready_count: 2, ..Default::default() };
        let rendered = context.render();
        assert!(rendered.contains("缺口 5 个"), "{rendered}");
        assert!(rendered.contains("把就绪节点补到 7 个") == false, "the number contract lives in the system prompt");
    }
}
