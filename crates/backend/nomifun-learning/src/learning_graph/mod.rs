//! Learning-graph growth engine (ADR-0009): a course is never pre-built as a
//! DAG. It starts from a single learning goal and GROWS one small batch of
//! nodes at a time whenever the ready stock drops below the trigger level.
//! Growth has no automatic terminal state — the learner closes a course by
//! archiving it, and the machine never judges「长完了」(ADR-0010).
//!
//! Dependencies live entirely in the CONCEPT WEB: every node declares which
//! concepts it `teaches` and which it `assumes`, each at a TIER
//! (know < apply < teach). There are no prerequisite edges and — because the
//! structural gates reject any batch whose assumptions are not already
//! covered — no locked state in the UI: a published node is always ready
//! NOW. Cross-course readiness falls out of the global concept registry.
//!
//! The coach drafts a batch per call (json seam, thinking ON) over the 9-block
//! context (goal + declarations, the full structure reference material, the
//! terrain doors, ready count, roster, taught ledger, coverage gauge, behavior
//! digest, registry excerpt). Deterministic gates hold the decision power:
//! shape violations (ng.*), semantic gate rejections and concept-review
//! rejections each inject their clause report back into the next attempt
//! (≤3 rounds, the whole batch re-emitted); a structurally valid batch then
//! gets one independent AI concept review (advisory-but-blocking, degraded to
//! pass when unavailable).
//!
//! The knowledge STRUCTURE artifact (spines/blocks/edges snapshot, gd.*
//! contract) lives in [`structure_contract`] and stays reference-only: it
//! feeds no gate, holds no content, and never touches the concept web.

pub mod advice_alias;
pub mod alias_records;
pub mod definition_audit;
pub mod defnear;
pub mod growth_contract;
pub mod growth_retry;
pub mod lexicon;
pub mod mounting;
pub mod provenance;
pub mod structure_contract;
pub mod structure_gen;
pub mod structure_ops;
pub mod structure_view;
pub mod structure_prompts;

use std::collections::{HashMap, HashSet};

use nomifun_common::AppError;
use serde::{Deserialize, Serialize};

use crate::completer::LearningCompleter;

/// 就绪节点的补货目标：每次生长把就绪存量补到这个数（learnhub 的「7」——
/// 门与提示词同源的唯一数字，ADR-0009）。
pub(crate) const READY_TARGET: usize = 5;

/// 自动触发的水位线：节点完成时检查，就绪存量低于该值才触发生长。
pub(crate) const READY_TRIGGER: usize = 3;

/// 单批新增节点上限（提示词与门同源，超过整批拒收）。
pub(crate) const MAX_BATCH_NODES: usize = 5;

// ── Auto Refill（自动补批判定，m10）────────────────────────────────────────

/// 自动补批判定三态（learnhub 同名域中立化，m6 定案）：阈值仍是
/// [`READY_TRIGGER`]（无新量纲）、无暂停线。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoRefillVerdict {
    /// 面不够：就绪存量低于触发水位，补一批。
    Due,
    /// 水位以上：就绪存量不低于触发水位，无需补批。
    BelowThreshold,
    /// 开关关闭：设置开关与环境闸任一关闭（合力 false）即 Disabled。
    Disabled,
}

impl AutoRefillVerdict {
    /// 读数面的机器名（裁决端点与 smoke 读数行同源）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Due => "due",
            Self::BelowThreshold => "below_threshold",
            Self::Disabled => "disabled",
        }
    }
}

/// 自动补批判定纯函数（裁决纯读零 LLM）：输入就绪存量与开关合力
/// （设置 ∧ 环境闸）。裁决日志必带 ready 数字（一行）。
pub fn auto_refill_verdict(ready_count: usize, enabled: bool) -> AutoRefillVerdict {
    let verdict = if !enabled {
        AutoRefillVerdict::Disabled
    } else if ready_count < READY_TRIGGER {
        AutoRefillVerdict::Due
    } else {
        AutoRefillVerdict::BelowThreshold
    };
    tracing::info!("auto_refill: ready={ready_count} verdict={}", verdict.as_str());
    verdict
}

/// 环境闸读数（`NOMI_LEARNING_AUTO_REFILL`）：off/0/false = 关（verdict 恒
/// Disabled）；未设或其它取值 = 开（设置开关正常起作用）。
pub(crate) fn auto_refill_env_enabled() -> bool {
    std::env::var(crate::switches::AUTO_REFILL_SWITCH_ENV)
        .map(|value| !crate::switches::is_auto_refill_disabled_value(&value))
        .unwrap_or(true)
}

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
    /// est：专注工作量尺寸锚（量纲宣言：1 est 分钟 = 1 分钟专注学习）；
    /// 常规 5–30，极困难最多 60（门放行 1–60）。DB 列保留旧名
    /// `learning_lessons.estimated_minutes`（列名无用户面，语义 = est）。
    #[serde(default)]
    pub est: Option<u16>,
    /// 难度 1–5（标尺见 COACH_SYSTEM）：呈现序（结构参考材料排序）与花名册
    /// 仍消费；**深度水位确定性门已退役**（g3：档位比较是精度剧场）——难度
    /// 诚实性由 AI 概念评审门作语义兜底。缺省按 2（入门档）处理。
    #[serde(default)]
    pub difficulty: Option<u8>,
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
    /// 铸名的 attestation 出处（t8：「教练提名：……」——谁在哪个语境铸的
    /// 名；纯簿记不进门禁，缺省合法。不做书目引用——无教材数据源，写了
    /// 必是编造）。
    #[serde(default)]
    pub source: Option<String>,
}

/// new_aliases 通道（三轮 g2）：给**既有**概念提名新名的专职入口——
/// 三字段非空。处理链：定义归一化撞既有判据 = 直定宿主（不送裁决）；
/// 否则进近邻裁决且**必须裁为复用**（裁新铸 = 整批打回指点改走 mints）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConceptNewAlias {
    pub name: String,
    pub definition: String,
    pub source: String,
}

/// 一批生长提案：节点 + 铸名 + 别名提名 + 批注。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedBatch {
    #[serde(default)]
    pub nodes: Vec<ProposedNode>,
    #[serde(default)]
    pub mints: Vec<ConceptMint>,
    #[serde(default)]
    pub new_aliases: Vec<ConceptNewAlias>,
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
/// 条款化报告（ng.* 编号，g4）回灌给教练重选。**形状条款**（键白名单/词表/
/// 量级/批内唯一）住 [`growth_contract::validate_proposal`]；本函数只做需要
/// DB 事实的**语义条款**：
/// - 铸名不撞登记表（含**活跃**别名，大小写折叠；退役名不占池——g2/g3
///   口径；撞名消息带宿主指引）；
/// - 概念引用可解析：teaches/assumes 的名字必须是登记表现有概念（含别名）
///   或本批铸名；**唯一近形自动归一**（learnhub「系统会核对它属于哪个既有
///   概念」的宿主侧对应，g3 近形机器同源）： containment / 受限编辑距离
///   命中唯一宿主时就地改写并落 warn 留痕（`ng.refs.nearform`），歧义或
///   无匹配才红条打回（歧义消息列候选名）——字级名漂移不该烧掉整批；
/// - **就绪即发布**：每条 assumes 必须被全局已教账（既往批次已落库的
///   teaches）以不低于所假定档位覆盖——同批兄弟的 teaches 与本批铸名不构成
///   供给：同批节点没有学习先后、必须同时就绪，谁也不依赖谁。不满足的节点
///   不存在「入库后锁定」，直接打回；
/// - 课程内节点标题唯一。
///
/// 返回（红条,近形归一 warn）：红条非空 = 整批拒收；warn 仅留痕，批照常落库。
pub fn validate_batch(
    batch: &mut ProposedBatch,
    coverage: &ConceptCoverage,
    registry_names: &HashSet<String>,
    name_owner: &HashMap<String, String>,
    existing_titles: &HashSet<String>,
) -> (Vec<GateError>, Vec<GateError>) {
    let collision_message = |name: &str| match name_owner.get(name) {
        Some(owner) => format!(
            "铸名「{name}」与概念登记表现有名字冲突——该名字已属概念「{owner}」，直接引用即可，不要重复铸名"
        ),
        None => format!("铸名「{name}」与概念登记表现有名字冲突"),
    };
    let mut errors = Vec::new();
    
    // 近形归一解域（自有所有权，避免与节点改写的可变借用交叠）：
    // （小写名, 原样名, 宿主 canonical）。new_alias 提名不进近形解域——宿主
    // 评审未裁，只准精确引用（既有口径）。
    let mut universe: Vec<(String, String, String)> = Vec::new();
    for (lower, owner) in name_owner {
        universe.push((lower.clone(), owner.clone(), owner.clone()));
    }
    for mint in &batch.mints {
        let canonical = mint.canonical.trim();
        if canonical.is_empty() {
            continue;
        }
        universe.push((canonical.to_lowercase(), canonical.to_owned(), canonical.to_owned()));
        for alias in &mint.aliases {
            let alias = alias.trim();
            if !alias.is_empty() {
                universe.push((alias.to_lowercase(), alias.to_owned(), canonical.to_owned()));
            }
        }
    }

    // 标题重复的机械摘除（第三个非打回处置）：重提课程已有标题 = 重教已满
    // 足的节点（同批不重教；螺旋重现合法但「标题不得完全相同」——提示词自
    // 律），批内撞车 = 同一角度同深度写了两遍。两者都摘除后出现的那一个、
    // 留痕照常落库，不烧重裁轮。
    let mut seen_titles: HashSet<String> = existing_titles.clone();
    let mut warns: Vec<GateError> = Vec::new();
    let mut kept_nodes: Vec<ProposedNode> = Vec::with_capacity(batch.nodes.len());
    for node in batch.nodes.drain(..) {
        let key = node.title.trim().to_lowercase();
        let course_duplicate = existing_titles.contains(&key);
        if seen_titles.insert(key.clone()) {
            kept_nodes.push(node);
        } else {
            warns.push(GateError {
                kind: "ng.title.pruned",
                message: format!(
                    "节点标题「{}」与{}重复，已从本批摘除——重提已满足的节点不重复落库；螺旋重现合法但请换角度并改标题",
                    node.title.trim(),
                    if course_duplicate { "课程已有节点" } else { "本批在先节点" }
                ),
            });
        }
    }
    batch.nodes = kept_nodes;

    // 批内铸名预备账：canonical → 该概念在本批可引用的名字集合（批内唯一性
    // 已由形状契约保证）。new_aliases 提名（j04）同入可解析域——同批提名
    // 的别名立即可被 teaches/assumes 引用，宿主在评审裁决后落位。
    let mut mint_names: HashSet<String> = HashSet::new();
    for mint in &batch.mints {
        let canonical = mint.canonical.trim();
        if registry_names.contains(&canonical.to_lowercase()) {
            errors.push(GateError {
                kind: "ng.mints.collision",
                message: collision_message(canonical),
            });
        }
        mint_names.insert(canonical.to_lowercase());
        for alias in &mint.aliases {
            let alias = alias.trim();
            if alias.is_empty() {
                continue;
            }
            if registry_names.contains(&alias.to_lowercase()) {
                errors.push(GateError {
                    kind: "ng.mints.collision",
                    message: collision_message(alias),
                });
            }
            mint_names.insert(alias.to_lowercase());
        }
    }
    for nomination in &batch.new_aliases {
        mint_names.insert(nomination.name.trim().to_lowercase());
    }

    for node in &mut batch.nodes {
        let title = node.title.trim().to_owned();

        for concept in node.teaches.iter_mut().chain(node.assumes.iter_mut()) {
            let name = concept.name.trim().to_owned();
            let key = name.to_lowercase();
            let resolvable = registry_names.contains(&key) || mint_names.contains(&key);
            if resolvable {
                continue;
            }
            // 近形归一（learnhub「系统会核对它属于哪个既有概念」）：唯一宿主
            // 就地改写 + warn 留痕；歧义红条带候选名，无匹配红条维持原口径。
            let mut hosts: Vec<(String, String)> = Vec::new();
            for (lower, original, owner) in &universe {
                let close = defnear::normalize(&name) == defnear::normalize(lower)
                    || defnear::name_near_form(&name, original)
                    || defnear::bounded_edit_close(&name, original);
                if close && !hosts.iter().any(|(host, _)| host == owner) {
                    hosts.push((owner.clone(), original.clone()));
                }
            }
            match hosts.len() {
                1 => {
                    let (host, _) = &hosts[0];
                    warns.push(GateError {
                        kind: "ng.refs.nearform",
                        message: format!(
                            "节点 {title} 引用的「{name}」不在概念登记表，已按唯一近形归一到「{host}」——后续批次请照抄「{host}」"
                        ),
                    });
                    concept.name = host.clone();
                }
                0 => errors.push(GateError {
                    kind: "ng.refs.resolvable",
                    message: format!(
                        "节点 {title} 引用的概念「{name}」既不在概念登记表，也未随本批铸名——先铸名或改用登记表中的名字"
                    ),
                }),
                _ => {
                    let candidates = hosts
                        .iter()
                        .take(3)
                        .map(|(host, _)| format!("「{host}」"))
                        .collect::<Vec<_>>()
                        .join("、");
                    errors.push(GateError {
                        kind: "ng.refs.resolvable",
                        message: format!(
                            "节点 {title} 引用的概念「{name}」既不在概念登记表，也未随本批铸名；有多个近形候选无法自动判定：{candidates}——请逐字照抄其中一个，或先铸名"
                        ),
                    });
                }
            }
        }

        for assumption in &node.assumes {
            let key = assumption.name.trim().to_lowercase();
            let covered = coverage.get(&key).is_some_and(|taught| *taught >= assumption.tier);
            if !covered {
                errors.push(GateError {
                    kind: "ng.assumes.uncovered",
                    message: format!(
                        "节点 {title} 假定「{name}」到「{assumed}」档位，但既往批次没有节点把它教到该档位——{hint}",
                        name = assumption.name.trim(),
                        assumed = assumption.tier.as_str(),
                        hint = "同批兄弟的 teaches 与本批铸名不算供给（同批无先后、必须同时就绪）：把本节点留到下一批，本批可先长教该概念的铺垫节点",
                    ),
                });
            }
        }
    }

    (errors, warns)
}
/// 档位的中文显示名（提示词与门消息共用这一份，i18n 另有对应键）。
pub fn tier_zh(tier: ConceptTier) -> &'static str {
    match tier {
        ConceptTier::Know => "知道",
        ConceptTier::Apply => "会用",
        ConceptTier::Teach => "能教",
    }
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
      "purpose": "练什么 + 从哪段课程外日常经验切入（座位表定位/给物品分类/分食物看温度一类生活场景）；找不到日常经验切口的节点不合格",
      "est": 20,
      "difficulty": 2,
      "teaches": [ { "name": "概念名", "tier": "apply" } ],
      "assumes": [ { "name": "概念名（必须已被既往批次教到该档位；本批兄弟不算）", "tier": "know" } ]
    }
  ],
  "mints": [ { "canonical": "新概念名（只登记名字，铸名≠教）", "aliases": ["别名"], "definition": "一句话定义", "source": "铸名出处（可省）：谁在哪个语境铸的名，如「教练提名：低位加法竖式的位权解释」——不是书目引用，禁止编造教材出处" } ],
  "new_aliases": [ { "name": "既有概念的新名字", "definition": "该名字下的意义（投影角度定义）", "source": "提名出处（哪个角度需要它）" } ],
  "note": "一句话批注：本批为什么长这些节点"
}
【档位】
- know（知道）：能辨认、复述。apply（会用）：能解题、应用。teach（能教）：能讲解、纠错。
- teaches 标本课把该概念教到什么档位；assumes 标本课假定学习者已把该概念掌握到什么档位。
- teaches 与 assumes 都写「本次」的投影：teaches 写本节点这次把概念教到哪一档，assumes 按本节点这次实际依赖的深度写档位——同一概念可以多次教、一次比一次深，档位跟着本次走，不按概念的「最终形态」虚报。
- assumes 必须被已教账（既往批次已落库的 teaches）以不低于所假定档位覆盖，否则整批被打回——不存在的「先锁后面再补」。同批兄弟的 teaches 与本批铸名不构成供给：同批节点没有学习先后、必须同时就绪，谁也不依赖谁。
- 档位自省以**教学深度本身**为准，不对照动词字面（动词只是示例，包括但不限于）：节点整体是操作训练（如计算/化简/求/作/写出一类动作句）而 teaches 全标 know 时，先自省是否低估——能按程序做出来，就不只是 know（知道是什么），至少该是 apply（会用）；反过来，首教形式化这类「写」只是认识步的节点，标 know 也可能是如实的。
【起点纪律】
- 学习者的真实起点 = 【学习目标】里的基础声明（未声明即彻底零基础）。判断一个节点是不是合格入口，看基线，不看它在教材里的位置：「简单」「基础」「入门」「第一章」都不等于零基础可起步。
- 零基础校验：入口节点直接用到的每个概念，要么本节点就教它（写进 teaches、随批铸名），要么属于课程外通用常识（时间、方位、日常活动一类）；既像常识又像本课概念的两可词，按概念对待。校验范围不止「用到」：节点文本（定义、操作、实例、易错提醒）里引用或预设的每个课程内概念同样算依赖——零基础学习者读到那句文本时，该概念要么已教、要么本节点就地教它。
- 「入门教材第一章」不等于零基础入口：第一章默认读者已带着该领域的日常底子，零基础学习者没有——这些概念只是离目标更近的台阶，不是入口。例：目标「业余段位围棋」的真入口是「数出棋盘上一块棋的气」，而不是「用大雪崩定式处理小目挂角」；目标「家庭手冲咖啡」的真入口是「按粉水比称出一次冲煮的粉与水」，而不是「用三段注水矫正萃取率」。
- 入口素材从学习目标「往下问」得来：目标只是路线的上层，它依赖的通用语言——操作工具、认读记号、基本量与单位、分类与命名、简单判断与比较——才是入口层的素材。对目标逐层往下追问「基线学习者现在连什么都不会，第一步从哪算起」，把最底层那一格摆出来；起点够不够低的唯一标准是基线学习者今天拿到节点就能开始，而不是它长得像该方向的入门课。
- 方向感优先级：学习者当下最缺的台阶优先——贴着概念网的最前沿往目标方向长，不直线冲终点。为逼近目标而长出基线够不着的节点，是比停摆更坏的错误。
- 慢就是快：横向补实与向下补基同为推进——发现已教概念脚下悬空（教了它，但它依赖的通用语言没人教过）时，补教那个底座也是朝目标的实质推进；坡道的质量与坡道的长度同等重要，不以前进牺牲认知依赖的完整性。
【节点纪律】
- 节点是单次学习会话（动作句），est（专注工作量，1 est 分钟 = 1 分钟专注）常规 5-30，极困难最多 60。
- 日常经验切口：每个节点的 purpose 必须点出从哪段日常经验引入——但切口只是引入方式，节点内容永远是学科概念：教的是把日常能力升级为学科语言的那一格（例：围棋课把「把什么归为一类」的分类经验升级为棋子的死活与归属判断，手冲咖啡课把「看茶叶沉浮」的观察经验升级为萃取状态的判读）。成人已会的纯生活动作没有教学价值，不合格；「该领域教材的第一章」默认读者已带领域底子，越过学科底层直奔第一章同样不合格。
- 道位配平：道（本质理解）与术（可执行）是一对标尺——一条线如果只有「怎么做」的操作台阶，要在合适的节点补一个「为什么成立」的台阶（说理、推导或辨析），让学习者不但能做，还能讲出它为什么可靠。
- 悬空技巧不合格：每批装箱前先问「学习者此刻真正缺什么」——技巧依赖的更一般技能先立地基；只有操作没有理解的地方补「为什么成立」的台阶；学习目标点名而还没开线的能力，尽早开线。
- 难度标尺（difficulty 1-5，如实申报）：1=生活经验直接上手；2=单概念入门（认读、模仿、单步操作）；3=单概念应用（多步计算、变式练习）；4=多概念综合/形式化引入（记号体系、复合结构）；5=严格证明与抽象推理。形式化/证明类内容不得压档。
- 一次会话能消化的新东西有限：新授概念是会话的稀缺资源，一个节点通常只引入 1 个核心概念、至多 2 个，标题相应只写一个动作；会话时长用练习与变式撑，不用多讲几个概念撑——把一章的新概念塞进一个节点不是自含，是过载，正路是拆成台阶。
- 动作词按内容的自然形态取，不设词形要求；同批与相邻批不全是同一型（清一色「计算」或清一色「求解」说明装箱偷懒），也不必刻意求变。
- 同一主题可螺旋重现，且反复教本身合法：不同档位螺旋加深、同档位换角度变式皆有价值——只有同一角度同一深度的重复没有价值，标题不得完全相同。
- 弧线跨批：课程整体是多线弧线，每次生成只交付其中一片；先后承接只发生在批与批之间——相邻的两步分装相邻两批，互不依赖的并进变体可以同批。概念节点先行，它的直接应用节点下一批再以 assumes 挂接；前置与后继不装同一批。
- 对照限同轴：对照/对比节点比较的必须是同一维度或同一手法的两个取值（两种方法解同一类题、两个相近概念的一线之差）；两个不同概念的首次引入不是对照，是抱团——分开设节点，不硬装进一个「对比」框。
- 从易到难：新节点优先长在当前就绪前沿的临近难度上，不突兀拔高。
【概念纪律】
- 名字出处：teaches/assumes 引用的每个概念名，要么照抄登记表/已教账/别名池中的既有名字，要么随批 mints 铸名，要么随批 new_aliases 提名；四处皆无 = 整批被打回。与登记表现有概念同义时改用既有名字，不重复铸名。
- 选名裁量：引用已教概念时，从别名池里选最贴合本节点角度的名字（投影即命名）；一个新角度确实值得命名时，用 new_aliases 给该概念提名新名（附该名字下的意义与出处）——评审裁决复用后它保留为该概念的活跃别名，下次可直接引用。不要为同一角度造第二个名字。
- new_aliases 只收「既有概念的新名字」：登记表里没有的东西走 mints 铸名。提名的概念若被裁决为新铸，整批打回——把该提名改写成 mints 再交。
- 提名定义一物一义（定义原子性）：一条提名只定义一个对象——定义里不得顺带给第二个概念下定义；定义里预设的构件概念，要么单列提名、要么改写绕开。自检验收：把提名拆成两半，各半仍说得出独立定义的 = 捆扎条目，必须拆条。概念严格原子，学习节点允许复合：节点标题综合多个对象合法，但 teaches 的每个概念各自单列提名，不得捆成一个概念条目。
- 铸名≠供给：mints 只是把名字登记进概念表。本批节点可以 teach 新铸概念（这是铸名的唯一用途），但任何节点都不得 assume 只有本批（兄弟 teaches 或本批铸名）才供给的概念——同批无先后、必须同时就绪，那种依赖整批打回。
- 缺前提的正路是分层长：节点内容用到的概念既往批次没教过，本批就先长教它的铺垫节点（铸名写进该节点的 teaches），把依赖它的节点留到下一批——而不是削掉前提、空泛化内容，或把依赖链塞进同一批。
【第一批（花名册为空时）】
- 本课尚无节点：第一批全是入口节点——每条 assumes 只能假定基线学习者已会的东西：课程外通用常识，或已教概念账里已有的概念（账跨课程，学习者在其他课程学的同样算会）；两者皆无的前提概念留到下一批，本批先长教它的铺垫节点。逐节点按【起点纪律】校验。一步一层，宁可少长一层。
- 首批特则：首批节点以单一概念或单一最小操作为度（比常规批的「1 个核心概念至多 2 个」更紧）——入口层是地基，一格一格铺，不求进度。
【行为信号】
- 行为摘要给出近窗正确率趋势、卡点节点与真实保留率：生长方向优先回应卡点——在卡点节点附近补铺垫或对比节点，而不是无视它继续铺新域。
- 但不要为单个卡点堆同质节点：一次至多一个针对性的回应节点。
【数量契约】
- 本次生长：把就绪节点补到 {READY_TARGET} 个。当前就绪 X 个、缺口 {READY_TARGET}-X 个，则本批新增节点 ≤ {READY_TARGET}-X 个且至多 {MAX_BATCH_NODES} 个；缺口为 0 时输出空 nodes。
- 批容量红线：一批（含打回后的修订稿）绝不超过 {MAX_BATCH_NODES} 个节点——修订稿超限时宁可将这批拆小重交，不得超限。
- 缺口就是本批的铺开面，通常铺得满：入口层天然是宽的——目标之下往往有比预期更多的底层方向，每个方向的最底层台阶各摆一个入口节点，直到铺满 {READY_TARGET} 个；真穷尽了互不依赖的独立方向才允许少长，并在 note 里逐一点名已铺的方向（举证不是偷懒）。例：目标「独立做出一款小游戏」，入口层可以同时铺编程语言、平面绘图、关卡设计、操作手感、叙事节奏、音效、发布渠道——底层方向常常比预想还多。
【提交前自查】输出前逐条核对，任何一条不满足就先改再输出：
1. 每个 teaches/assumes 的名字都能在登记表、已教账或本批 mints 里找到；
2. 每条 assumes 的概念都已由既往批次教到了不低于所假定档位（本批兄弟的 teaches 与本批铸名不算数）；
3. mints 与登记表、批内彼此都不撞名；纯登记、无人引用的新名字删掉；
4. 本批新增节点数 ≤ 缺口数；
5. 每个节点的起点假设都在基线或已教账内——没有把「教材第一章」当零基础入口，也没有为了覆盖目标方向长出基线够不着的节点；
6. 每个节点的 purpose 都有日常经验切口，且教的是还需要教的学科概念——纯生活动作（成人已会的）和写不出切口的中层内容，都换掉。
7. 每个节点的 difficulty 都按标尺如实申报（形式化/证明类不得压档）。
【输出】
- 只输出 JSON，不要 Markdown 代码块，不要任何解释。note 与 purpose 用中文。"#;

/// [`COACH_SYSTEM`] 的装配面（t7）：数量契约段的 `{READY_TARGET}`/
/// `{MAX_BATCH_NODES}` 占位符以常量单源注入——改批容量只改常量，提示词
/// 不再手写数字。
pub(crate) fn coach_system() -> String {
    COACH_SYSTEM
        .replace("{READY_TARGET}", &READY_TARGET.to_string())
        .replace("{MAX_BATCH_NODES}", &MAX_BATCH_NODES.to_string())
}

/// Everything the coach needs, rendered once into the opening user message.
/// 字段序不是渲染序——渲染按 g3 裁定的 9 块注意力序（见 [`CoachContext::render`]）。
#[derive(Debug, Clone, Default)]
pub struct CoachContext {
    /// 块 1：学习目标（课程行 learning_goal，回退标题）。
    pub goal: String,
    /// 块 1 附注：程度声明（结构产物标量回执；空 = 未提供）。
    pub proficiency_statement: String,
    /// 块 1 附注：基础声明（地形产物回执；空 = 纯零基础假设）。
    pub learner_basis: String,
    /// 块 2：结构参考材料——全量块摊开（learnhub reference.rs 行格式，纯结构
    /// 事实、不裁剪候选面、不限批范围；教练可围绕参考装箱也可自拟）。
    pub structure_reference: String,
    /// 块 3：前置地形门口（`- [G1]（硬前置）判据` 行列表；首批入口重点参照、
    /// 其后软参照；空 = 无结构快照，教练自行盘点）。
    pub terrain_doors: String,
    /// 块 5：花名册行：「状态 | 标题 | teaches(概念@档) assumes(概念@档)」；
    /// 卡点节点行带 ⚑ 前缀。
    pub roster: String,
    /// 块 6：已教概念账（canonical@最高档 的列表）。
    pub taught_summary: String,
    /// 块 7：覆盖读数（共用存量 met/skipped/pending + 概念档位足迹；纯展示）。
    pub coverage_gauge: String,
    /// 块 8：行为摘要（近窗趋势/卡点/保留率；空窗为空串，整块省略）。
    pub behavior_digest: String,
    /// 块 4：当前就绪节点数（缺口 = READY_TARGET − ready）。
    pub ready_count: usize,
    /// 块 9：登记表相关切片：与课程已涉概念邻近的名字（防止撞名铸名）。
    pub registry_excerpt: String,
}

impl CoachContext {
    /// 9 块渲染，顺序即注意力序（g3 裁决）：学习目标（+程度/基础声明）→
    /// 结构参考材料 → 地形门口 → 当前就绪 → 花名册 → 已教概念账 →
    /// 覆盖读数（纯展示）→ 行为摘要 → 登记表切片。
    pub(crate) fn render(&self) -> String {
        let gap = READY_TARGET.saturating_sub(self.ready_count);
        let mut text = String::new();
        // 块 1：学习目标（+程度/基础声明附注）。
        text.push_str(&format!("【学习目标】\n{}\n", self.goal));
        if !self.proficiency_statement.is_empty() || !self.learner_basis.is_empty() {
            text.push_str(&format!(
                "（程度声明：{}\n基础声明：{}\n）\n",
                if self.proficiency_statement.is_empty() { "未声明" } else { &self.proficiency_statement },
                if self.learner_basis.is_empty() { "未声明（按纯零基础处理）" } else { &self.learner_basis },
            ));
        }
        // 块 2：结构参考材料（全量块摊开；纯参考，不裁剪候选面、不做判定）。
        if !self.structure_reference.is_empty() {
            text.push_str(&format!(
                "\n【结构参考材料（全量块摊开：课程内参考地图，只回答「这门课有哪些知识地形」；它是参考不是限制——围绕它装箱或自拟都可，学习顺序权威在你）】\n{}\n",
                self.structure_reference
            ));
        }
        // 块 3：前置地形门口（首批入口重点参照；其后软参照）。
        if !self.terrain_doors.is_empty() {
            let framing = if self.roster.is_empty() {
                "首批入口重点参照：本批节点逐门落在门口判据的底层台阶上——每个入口节点 = 门口之下最底层的、还需要教学才会的那一格，措辞可微调、可拆分，但不得换成更高层的对象；标「可自检」的门口已具备的格子不重教"
            } else {
                "软参照：门口以下是假设已会的前置，不是本课教学单元"
            };
            text.push_str(&format!(
                "\n【前置地形门口（{framing}）】\n{}\n",
                self.terrain_doors
            ));
        }
        // 首批凝练纪律（g3：删 6 条规则长文，收缩为结构材料摊开 + 门口参考 +
        // 两条纪律 + 空账本机械地板）。已教账非空时（老学习者开新课）机械地板
        // 让位给跨课程口径：账内概念可进 assumes（就绪判定本就跨课程计数）。
        if self.roster.is_empty() {
            let ledger_line = if self.taught_summary.is_empty() {
                "- 已教概念账为空 ⇒ 本批 assumes 必须全空（机械地板：空账不供给任何概念）。\n"
            } else {
                "- 已教概念账是跨课程全局账（学习者在其他课程学会的也在里面，非空是常态）：本批 assumes 可假定账内已有概念；账内没有的才需要本批铺垫。\n"
            };
            text.push_str(&format!(
                "\n【首批纪律（花名册为空）】\n\
                 - 入口从结构参考的入门档起步，逐格往下问「基线学习者今天拿起什么就能开始」；「简单、基础、入门、教材第一章」都不等于零基础可起步。\n\
                 - 动作句 + 单一行为单元；用到的东西要么本节点教它、要么是常识——简单≠常识。\n\
                 {}", ledger_line));
        }
        // 块 4：当前就绪。
        text.push_str(&format!(
            "\n【当前就绪】{ready} 个（目标 {READY_TARGET} 个，缺口 {gap} 个）\n",
            ready = self.ready_count
        ));
        // 块 5：花名册。
        let roster_note = if self.roster.is_empty() {
            if self.taught_summary.is_empty() {
                "（课程还是空的——本批是第一批：全是入口节点，各自自带引入所教概念（teaches + 铸名），assumes 必须全空）"
            } else {
                "（本课还是空的——本批是本课第一批。已教概念账是跨课程全局账：账内已有概念可进 assumes，账内没有的由本批节点自带引入（teaches + 铸名））"
            }
        } else {
            &self.roster
        };
        text.push_str(&format!("\n【花名册】\n{}\n", roster_note));
        // 块 6：已教概念账（跨课程全局口径显式声明——评审/教练不再把
        // 「账非空 × 花名册为空」读成材料矛盾）。
        text.push_str(&format!(
            "\n【已教概念账（跨课程全局：含学习者在其他课程所学）】\n{}\n",
            if self.taught_summary.is_empty() { "（空——本批节点不得依赖本批产出的概念：assumes 只能留空或假定课程外常识，需要前提的概念留到下一批）" } else { &self.taught_summary }
        ));
        // 块 7：覆盖读数（纯展示；本课程存量口径，与全局账数字无关）。
        if !self.coverage_gauge.is_empty() {
            text.push_str(&format!("\n【覆盖读数（本课程存量口径；纯展示，不构成本批依据）】\n{}\n", self.coverage_gauge));
        }
        // 块 8：行为摘要。
        if !self.behavior_digest.is_empty() {
            text.push_str(&format!("\n【行为摘要】\n{}\n", self.behavior_digest));
        }
        // 块 9：概念登记表切片。
        if !self.registry_excerpt.is_empty() {
            text.push_str(&format!("\n【概念登记表（邻近切片，铸名前先查撞名）】\n{}\n", self.registry_excerpt));
        }
        text
    }
}

/// Parse a coach reply into a proposed batch, strictly (g4: 容错 deserializer
/// 已退役): the reply must be a single JSON object satisfying
/// [`growth_contract::validate_proposal`]; any clause violation surfaces as an
/// error carrying the clause report (`ng.*`), which the growth loop injects
/// back as retry feedback. Values are TRIMMED into the canonical struct only
/// after the shape contract passes.
pub(crate) fn parse_coach_reply(raw: &str) -> Result<ProposedBatch, AppError> {
    let mut value = crate::generation::parse_json_object::<serde_json::Value>(raw).map_err(|error| {
        tracing::warn!(
            error = %error,
            raw_len = raw.len(),
            raw_head = %raw.chars().take(800).collect::<String>(),
            "unparseable coach reply"
        );
        AppError::UnprocessableEntity(format!("- [ng.parse] 教练回包不是单个 JSON 对象：{error}"))
    })?;
    // 机械归一（t7）：中文档位/est 字符串在契约核验前重写（可机械判定的
    // 不送模型——真机 round 4 死链的引信）；词表/范围门照旧。
    let rewrites = growth_contract::normalize_proposal(&mut value);
    if rewrites > 0 {
        tracing::warn!(
            rewrites,
            "coach reply mechanically normalized before the shape contract (中文档位/est 字符串)"
        );
    }
    let violations: Vec<structure_contract::ContractViolation> =
        growth_contract::validate_proposal(&value)
            .into_iter()
            .filter(|violation| !violation.warning)
            .collect();
    if !violations.is_empty() {
        return Err(AppError::UnprocessableEntity(format!(
            "教练批次违反生长契约:\n{}",
            structure_contract::format_violations(&violations)
        )));
    }
    let mut batch: ProposedBatch = serde_json::from_value(value).map_err(|error| {
        // 形状契约已过仍反序列化失败 = 形状契约与结构漂移（缺陷），照实报。
        AppError::Internal(format!("coach reply type drift after shape contract: {error}"))
    })?;
    // 归一：trim（契约只保证非空，不含首尾空白语义）。
    for node in &mut batch.nodes {
        node.title = node.title.trim().to_owned();
        node.purpose = node.purpose.trim().to_owned();
        for reference in node.teaches.iter_mut().chain(node.assumes.iter_mut()) {
            reference.name = reference.name.trim().to_owned();
        }
    }
    for mint in &mut batch.mints {
        mint.canonical = mint.canonical.trim().to_owned();
        mint.aliases = mint
            .aliases
            .iter()
            .map(|alias| alias.trim().to_owned())
            .filter(|alias| !alias.is_empty())
            .collect();
        mint.definition = mint.definition.trim().to_owned();
    }
    batch.note = batch.note.trim().to_owned();
    Ok(batch)
}

/// One coach call → a proposed batch. JSON seam + thinking ON (via
/// [`crate::generation::complete_json_coach`]): the coach must survey entry
/// domains and descend the prerequisite ladder — deliberation work, not
/// lookup. `feedback` 非空 = 上一轮的逐条款报告（ng.*），注入装配引导重选。
pub(crate) async fn coach_draft(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    context: &CoachContext,
    feedback: &str,
) -> Result<ProposedBatch, AppError> {
    let mut user = context.render();
    if !feedback.trim().is_empty() {
        user.push_str(&format!(
            "\n\n【上一轮打回与逐条修正】\n{feedback}\n\n【修订纪律】\n- 逐条修正被点名的条款；未被点名的部分不要重写。\n- 重新输出完整批次 JSON。"
        ));
    }
    let raw = crate::generation::complete_json_coach(
        completer,
        model_override,
        &coach_system(),
        &user,
        crate::generation::COACH_MAX_TOKENS,
        None,
    )
    .await?;
    parse_coach_reply(&raw)
}

/// One repair call: the gate report (or review problems) plus the PREVIOUS
/// draft go back on top of the original context — the coach re-emits the
/// WHOLE batch, fixed. The previous draft matters: these calls are stateless,
/// and a coach that only sees the problem list rebuilds the batch from
/// scratch — dropping scaffolding, re-churning names, and referencing mints
/// from the rejected draft that never reached the registry (the 2026-09-29
/// math-course burn: draft 3 assumed six names it never minted, 16 gate
/// errors). The repair discipline is point-editing, not re-planning.
pub(crate) async fn coach_redraft(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    context: &CoachContext,
    previous: &ProposedBatch,
    problems: &str,
) -> Result<ProposedBatch, AppError> {
    let previous_json = serde_json::to_string_pretty(previous).unwrap_or_default();
    let user = format!(
        "{}\n\n【上一稿（整批被打回、未落库：其 mints 也未进登记表，本稿要引用就得随批重新铸名）】\n{previous_json}\n\n【打回问题，逐条修复】\n{problems}\n\n【修订纪律】\n- 点修不是重写：未被点名的节点、概念引用与 est 原样保留，只改被点名的地方。\n- 打回问题让你补充的每个新概念：进本批某节点的 teaches 就随批 mints 铸名；要被 assume 的只有已教账里已有的才合法——既往批次没教过的，把依赖它的节点移出本批（留到下一批），本批改长教它的铺垫节点。\n- 这批修不动就缩小它：减少节点数，先教最缺的台阶——缩小比重写更可能过门。\n- 重新输出完整批次 JSON。",
        context.render()
    );
    let raw = crate::generation::complete_json_coach(
        completer,
        model_override,
        &coach_system(),
        &user,
        crate::generation::COACH_MAX_TOKENS,
        None,
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

/// 评审提示词的装配面（k07）：判据①按开关取新版（挂号子查）或旧版（off
/// 臂）；其余段落共用一份常量——零第二份全文。
const CONCEPT_REVIEW_HEAD: &str = r#"你是一名概念评审员：在结构门之后对生长批次做最后一次语义兜底审查。结构门（机器，已通过）保证过：名字都能解析、每条 assumes 都被既往批次的已教账覆盖（同批兄弟的 teaches 与本批铸名不算供给）、批量与标题合法。你只查结构门查不了的概念面问题。

【先读材料，再下结论】
- 本批节点同批落地、没有学习先后，因此批内互不依赖：节点只许依赖既往批次已教的概念，学习者可能从本批任意节点起步。凡以「批内先后顺序」为由的问题一律不成立——同批无顺序，但同批也无互相依赖。
- 口径分层：【已教概念账】是跨课程全局账——学习者在其他课程学会的概念也在里面；【花名册】与【覆盖读数】是本课程口径。账非空而花名册为空是常态（老学习者开新课），不是材料矛盾，不要报；账内概念可以合法地被首批 assumes 引用。
- 事实以材料为准：【已教概念账】【花名册】与待批 JSON 里的 mints 是全部事实源——名单里有的就是有，没有的才是没有；不要凭印象断言「某概念没人教过」。

【判据——逐节点核对四条】
"#;

/// 判据①（k07 新版，learnhub #127 e1f111c 的挂号子查结构化）：先框定
/// 「登记表外名词」，再逐词核对出处三源（allo 四入口：含本节点 assumes），
/// 点名批内提名不算出处与内嵌上位词在审；含糊词按概念处理。
const CRITERION_ONE_V2: &str = r#"1. 挂号充分（子查：登记表外名词）：
   - **框定**：节点标题与 purpose，以及随批 mints 的 canonical 与 new_aliases 的名字及定义里出现的每个领域名词，都是待查的「登记表外名词」——名单里已有的名字不算登记表外。
   - **出处核对（四入口）**：每个登记表外名词，须能落到以下之一——①已教（已教账里的正名或别名）、②本节点 teaches（本节点自教）、③本节点 assumes、④课程外通用常识。「简单」不等于常识——课程内的基础概念也要挂号；既像常识又像本课概念的两可词，按概念处理（要求挂号）。四处皆无 = 挂号缺失，打回并指名该名词。
   - **批内提名本身不是出处**：某名词只有本批其它节点的 teaches 或本批铸名供给、而本节点不自教、也不在已教账/assumes 里，就是隐藏的批内先后（同批互不供给），照打——不要接受「批内兄弟会教」作为出处。
   - **概念名内嵌的上位词同样在审**：提名叫「研磨度的校准」，「研磨度」自己未在册、无人教、又非常识 = 挂号缺失，与显式名词同罪；名字里的每个构成词都要各自落到出处。
   - purpose 里「为后续 X 打基础/铺路」式的方向性提及不算无出处——它描述的是课程路线，不是本节点的内容依赖；本节点内容真正用到 X 才要求挂号。
   - **修法（与覆盖法同款）**：缺的前提可补挂 assumes（已教可覆盖）；本节点能承载就并入 teaches 自教；都不可行则移出本批，下批再装。
"#;

/// 判据①（k07 旧版，off 臂原样）：改版前的并句措辞，逐字保留。
const CRITERION_ONE_V1: &str = r#"1. 挂号充分：节点标题与 purpose 里出现的每个领域名词，要么已教（已教账或本批 teaches）、要么已假定（本节点 assumes）、要么随批铸名、要么属于课程外的通用常识。「简单」不等于常识——课程内的基础概念也要挂号；三处皆无的名词，打回并指名。purpose 里「为后续 X 打基础/铺路」式的方向性提及不算无出处——它描述的是课程路线，不是本节点的内容依赖；本节点内容真正用到 X 才要求挂号。
"#;

/// 挂号表的程序句（m3）：判据①改为「按表核对」——CRITERION_ONE_V2 的
/// 框定/四入口措辞保留为表的语义说明，判据的程序出口是结论里的
/// `term_provenance` 表（learnhub #169「判据在场 ≠ 判据被执行」的程序化
/// 收口：原则形态下评审员可无声放行，程序形态下缺表/行伪至少留痕）。
const CRITERION_ONE_TABLE_CLAUSE: &str = r#"   - **逐词落行（按表核对，输出硬约束）**：把上面四入口的核对结果逐词落成结论里的 `term_provenance` 挂号表——本批每条 mints 铸名与 new_aliases 提名的**定义**里引用或预设的领域名词逐个一行，行形状与来源四值见下方【挂号表 term_provenance】段。评审接受前机械校验整表：缺表、漏行或行出处对不上 = 结论无效，带死因重试恰一次，仍败按 Degraded 保守放行并留痕。
"#;

/// 挂号表的输出契约段（m3，织入评审系统提示词尾部）：行形状 + 来源四值 +
/// 逐字规则 + 同形词盲区注记（#176 真机「次数」案：机械核行只认字面）+
/// 机械执法语义。词表与 `provenance::TERM_SOURCES` 互钉，改词表须同步。
const TABLE_OUTPUT_CONTRACT: &str = r#"

【挂号表 term_provenance——输出硬约束】
- 通过与打回的结论都必须携带 `term_provenance` 数组：批内每条提名至少一行，缺表或某条提名零行 = 结论无效。
- 每行形状：{ "concept": "提名名", "term": "该提名定义里引用或预设的领域名词", "source": "已教|自教|常识|缺失", "ref": "出处概念名" }。
- 报**已教**：`ref` 必填，逐字照抄已教账/登记表的正名或别名；ref 写了本批铸名或提名的名字 = 无效——本批产出不是已教出处。
- 报**自教**：`ref` 必填，逐字照抄本批某节点 teaches 里的概念名；ref 指向本条提名自身 = 循环自证，无效（定义预设的词不由定义自身供给）；提名的名字本身不算 teaches。
- 报**常识**：不写 `ref`；term 是课程外、无需教学、不承载本课知识功能的通用背景词，「简单」不等于常识。
- 报**缺失**：查无出处时如实报缺失——不写 `ref`，且该结论必须 pass=false 并在 problems 里指名该名词；标缺失却 pass = 自相矛盾，结论无效。缺失是合法的自认通道，绝不要把查无出处的缺口洗成常识。
- **常识/缺失行的 term 必须逐字取自该提名定义原文**（可带尾随括注，如「数（直线上点对应的数）」）；已教/自教行写的是定义所预设的概念名，不要求逐字出现在定义里。
- **同形词盲区注记**：机械核行只认字面、无词义维度——常识行的 term 逐字命中已教账/登记表名池即判事实矛盾，哪怕词义不同；确属同形异义时按已教行挂该名字的 `ref`（机械面绿），词义出入写进 findings 留痕供数据面复核。
"#;

const CONCEPT_REVIEW_TAIL: &str = r#"2. 前提足够：把「已教账 ∪ 节点声明的 assumes」当作学习者此刻已知的全部（本批 teaches 不算——同批互不依赖），逐节点问：凭这些，学得了这条节点的内容吗？内容真实依赖、却既无出处也未声明的对象/方法/记号，就是跳步——打回，并指名缺的是哪一个。**禁则：批内兄弟节点教出的概念不算已教，不得以「批内兄弟会教」为由建议任何节点挂 assumes**——那类建议与结构门直接矛盾（同批无先后、必须同时就绪），报了就是让教练自造机械矛盾；真缺前提的正路是建议把依赖方移出本批。
3. 命名与内涵一致：铸名的定义说的就是名字所指的东西；同一概念用了两个不同名字（应复用既有名）时，**不要打回**——按【复用裁决】写进 `reuse` 数组（concept_id 用评审材料里出现过的真实登记表 id：本提名的候选，或相关提名候选里指向同一概念的那个），该名字会落为既有概念的活跃别名，代码侧机械改写、不折返模型；只有重叠对象在材料里没有任何 concept_id 可指时才打回，并在 issue 里点名疑似宿主。
4. 难度诚实与阶段/档位相符：节点申报的 difficulty 与内容实际相符——形式化/证明类内容（记号体系、严格推理）不得压 1-2 档；节点内容与【结构参考材料】里同名块的档位明显背离时点名。内容过载（一个节点塞进多个核心概念）可附拆分建议。

【复用裁决（j04：每条提名必裁，机械面已备好候选集）】
- 【提名候选集】为每条铸名（mints 的 canonical）与别名提名（new_aliases）列出登记表近邻候选（含 concept_id 与入选理由）。逐条裁决「复用 | 新铸」：
- 裁**复用** = 该名字其实是既有概念的另一投影：把它写进输出顶层 `reuse` 数组（`{ "name": "<提名名>", "concept_id": "<登记表概念 id>" }`）；concept_id 必须是评审材料里出现过的真实 id——优先本提名的候选，相关提名候选里指向同一概念的值同样可用；编造即违约。复用的概念不铸新行，提名名落为该概念的活跃别名。
- 裁**新铸** = 提名确实是新概念：不写进 reuse 即可（缺省新铸）。
- **new_aliases 提名的语义就是「既有概念的新名字」——必须裁复用**；你认为它该是新概念时不要裁决，整批会被打回并指点它改走 mints。
- 判据：候选集的定义近邻/名字近形只是机械线索；拿不准时按「同一概念用了两个不同名字应复用既有名」（判据 3）与候选定义比对裁决。

【不算问题，一律不报】
- est 长短、每批节点数量与措辞偏好、教学法优化建议——这些是教练职权（拆分建议写在难度诚实条的 advice 里，不单独报）。
- 重教/复访已教概念：同一概念再次 teaches 完全合法（螺旋加深、换角度变式——概念不可学完）；已教账里有该概念不构成问题，除非同一角度同一深度的重复（标题与既有节点完全相同——结构门已拦）。
- 「可以更好」「建议增加」类意见。
- 你从材料确认不了的：宁放勿错。

【findings——数据缺陷留痕（不打回，只留痕）】
- 数据缺陷观察（如：材料里疑似缺某基础概念、结构材料可能过时、命名与材料用法可能有出入）
  不构成打回硬伤时，写进输出顶层可选 `findings` 数组：`{ "node": "<节点标题或空>",
  "field": "<字段或空>", "issue": "<缺什么/疑点什么>", "advice": "<一条修补方向>" }`。
- findings 是线索不是第二真相：与 problems 并行——该打回的仍走 problems；拿不准的
  观察、全局性疑点放 findings。没有观察就不写这个键。

回复严格 JSON：
- 通过：{ "verdict": "pass" }（有复用裁决时附带 "reuse" 数组，见【复用裁决】；有数据缺陷观察时附带 "findings" 数组）
- 打回：{ "verdict": "reject", "problems": [ { "node": "节点标题", "field": "teaches|assumes|mints|title|difficulty", "issue": "问题是什么，指名具体概念", "advice": "一条可执行的修复提示" } ] }（数据缺陷观察同样可附 "findings" 数组）
问题按严重度降序，至多 8 条；每条都必须是你能从材料确认的硬伤，issue 写清缺的是什么概念；一条硬伤都没有就 pass，绝不为了「显得尽责」凑问题。只输出 JSON，不要任何解释。"#;

/// 评审判据①新版的运维开关（k07）：置 off/0/false 退回改版前的并句措辞
/// （off 臂；判据②③④与 findings 通道两臂相同）。
pub const REVIEW_V2_SWITCH_ENV: &str = "NOMI_LEARNING_REVIEW_V2";

/// 开关取值判定（off/0/false；大小写与首尾空白宽容）。
pub fn is_review_v2_disabled_value(value: &str) -> bool {
    crate::is_off_switch_value(value)
}

/// 判据①新版是否开启（未设/其它值 = 开启）。
fn review_v2_enabled() -> bool {
    std::env::var(REVIEW_V2_SWITCH_ENV)
        .map(|value| !is_review_v2_disabled_value(&value))
        .unwrap_or(true)
}

/// 评审挂号表的运维开关（m3）：置 off/0/false = 不要求 `term_provenance`
/// 表、不做机械校验（= k07 后行为，off 臂）；未设/其它值 = 开启。开启时
/// 评审提示词织入表格条款（判据①按表核对）且评审结论接受前机械核对整表。
pub const REVIEW_PROVENANCE_SWITCH_ENV: &str = "NOMI_LEARNING_REVIEW_PROVENANCE";

/// 挂号表开关取值判定（off/0/false；大小写与首尾空白宽容）。
pub fn is_review_provenance_disabled_value(value: &str) -> bool {
    crate::is_off_switch_value(value)
}

/// 挂号表是否开启（未设/其它值 = 开启）。
fn review_provenance_enabled() -> bool {
    std::env::var(REVIEW_PROVENANCE_SWITCH_ENV)
        .map(|value| !is_review_provenance_disabled_value(&value))
        .unwrap_or(true)
}

/// 挂号表评审整路尝试上限（含首轮；「重试恰一次」= 2，learnhub
/// REVIEW_MAX_ATTEMPTS 同款）。只作用于挂号表机械校验判「结论不可用」的
/// 路径（表缺失/行伪/表键解析失败）；调用失败与整体解析失败的降级口径
/// 维持现状（保守放行，当场生效）——外层 j03 预算与指纹早停不动。
const REVIEW_PROVENANCE_ATTEMPTS: usize = 2;

/// 评审系统提示词装配（k07 判据①两臂 + m3 挂号表臂）：挂号表开 = V2 框定
/// 措辞 + 按表核对程序句 + 表输出契约段（V2 off 不改表臂——V1 并句把「随批
/// 铸名」算出处，与表的「批内提名不是出处」矛盾）；挂号表关 = k07 后行为
/// 原样（表条款与机械校验都不在）。
pub(crate) fn concept_review_system() -> String {
    if review_provenance_enabled() {
        return format!(
            "{CONCEPT_REVIEW_HEAD}{CRITERION_ONE_V2}{CRITERION_ONE_TABLE_CLAUSE}{CONCEPT_REVIEW_TAIL}{TABLE_OUTPUT_CONTRACT}"
        );
    }
    let criterion = if review_v2_enabled() {
        CRITERION_ONE_V2
    } else {
        CRITERION_ONE_V1
    };
    format!("{CONCEPT_REVIEW_HEAD}{criterion}{CONCEPT_REVIEW_TAIL}")
}

/// 评审产出（j04 扩权）：问题条 + 复用裁决。fails-open 降级 = 两者皆空。
/// j09 findings warn 通道：数据缺陷观察不打回只留痕，与 problems 阻断通道
/// 并行（调用方落 `learning_structure_findings`，修补回合的输入源）。
/// `provenance` = 挂号表机械校验读数（smoke rows/invalid/retried/degraded
/// 的数据源；读数面非门禁）。
#[derive(Debug, Clone, Default)]
pub(crate) struct ReviewOutcome {
    pub problems: Vec<ReviewProblem>,
    /// 复用裁决（形状已验；候选集归属由调用方机械核验）。
    pub reuse: Vec<ReuseRuling>,
    pub findings: Vec<ReviewFinding>,
    /// 挂号表机械校验读数（off 臂恒零值）。
    pub provenance: provenance::ProvenanceReadout,
}

/// 一条 findings 观察：留痕是线索不是第二真相。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReviewFinding {
    pub node: String,
    pub field: String,
    pub issue: String,
    pub advice: String,
}

/// 一条复用裁决：提名名 → 登记表中的宿主概念（候选集归属只作日志注记）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReuseRuling {
    pub name: String,
    pub concept_id: String,
}

/// 结构门对一条提名的机械装配（j04 复用通道）：直定宿主（定义归一化逐字
/// 相等，绕过评审）+ 近邻候选集（评审裁决的指向面）。
#[derive(Debug, Clone)]
pub struct NominationPlan {
    /// 提名名（trim 后）。
    pub name: String,
    /// 定义归一化直定宿主（Option；Some = 绕过评审直接复用该概念）。
    pub direct_host: Option<String>,
    /// 近邻候选集（词面补位保证非空——登记表非空时）。
    pub candidates: Vec<defnear::NeighborCandidate>,
}

/// 结构门产出：语义门错误 + 提名候选装配。
#[derive(Debug, Clone)]
pub struct GateOutcome {
    pub errors: Vec<GateError>,
    pub nominations: Vec<NominationPlan>,
    /// 登记表现存概念 id 全集（评审复用裁决的事实边界：候选集只是可见面，
    /// 裁决宿主只要求在这里存在）。
    pub registry_concept_ids: HashSet<String>,
    /// 登记表活跃名（小写）→ (concept_id, canonical)：advice 机械转化
    /// （t4）的宿主解析面。歧义名（多名撞库）不在此册。
    pub registry_name_hosts: HashMap<String, (String, String)>,
    /// 铸名撞登记表现有名的机械复用（t4 扩展）：提名名（小写）→ 宿主
    /// concept_id。撞池下复用是唯一不重命名的合法出路，直接走 j04 落池，
    /// 对应 `ng.mints.collision` 红条已被门面吸收。
    pub auto_reusals: HashMap<String, String>,
    /// 评审挂号表机械核对的已知名池（m3；小写折叠）：已教账 ∪ 登记表活跃
    /// 名+别名 ∪ 本批铸名/提名——learnhub 门面同构（known 含本批产出：已教
    /// 行指向本批提名由校验器单独红）。
    pub provenance_known: HashSet<String>,
}

/// Run the AI concept review over a gated batch. Any failure (no completer,
/// call error, unparseable reply) degrades to an empty outcome — the review
/// never blocks a structurally valid batch when it cannot run. `candidates`
/// 是【提名候选集】的渲染文本（j04 复用裁决的可见面）。`known_names` =
/// 挂号表机械核对的已知名池（gate_batch 装配：已教账 ∪ 登记表活跃名+别名
/// ∪ 本批铸名/提名；小写折叠），off 臂不用。
///
/// 挂号表臂（m3）：结论接受前机械核对 `term_provenance` 整表——表缺失/
/// 行伪/表键解析失败 = 结论不可用，**恰一次**带死因重试评审；仍败 =
/// Degraded 保守放行（评审器故障 ≠ 批缺陷），降级留痕走 findings（调用方
/// 落 j09 通道）。重试只重掷评审（死因进评审反馈），不动外层 j03 预算。
pub(crate) async fn concept_review(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    batch: &ProposedBatch,
    context: &CoachContext,
    candidates: &str,
    known_names: &[String],
) -> ReviewOutcome {
    let provenance_on = review_provenance_enabled();
    let batch_json = serde_json::to_string_pretty(batch).unwrap_or_default();
    let candidates_block = if candidates.trim().is_empty() {
        String::new()
    } else {
        format!("\n\n【提名候选集（复用裁决的机械面；concept_id 用材料里出现过的真实登记表 id）】\n{candidates}")
    };
    // 输出契约（ng.review.*）：verdict 词表 + problems/reuse 形状 + 挂号表
    // 行形状（serde default 容旧包/存量录制；必填性 = 提示词 + 重试执法，
    // 不走 serde 必填——解析单源容旧）。
    #[derive(Deserialize)]
    struct Verdict {
        #[serde(default)]
        verdict: String,
        #[serde(default)]
        problems: Vec<ReviewProblem>,
        #[serde(default)]
        reuse: Vec<ReuseRuling>,
        #[serde(default)]
        findings: Vec<ReviewFinding>,
        /// 挂号表（m3）：缺席 = 空（off 臂与存量形态兼容）。
        #[serde(default)]
        term_provenance: Vec<provenance::TermProvenance>,
    }
    fn verdict_outcome(verdict: Verdict, readout: provenance::ProvenanceReadout) -> ReviewOutcome {
        let accept = |problems: Vec<ReviewProblem>| ReviewOutcome {
            problems,
            reuse: verdict.reuse,
            findings: verdict.findings,
            provenance: readout,
        };
        if verdict.verdict == "reject" {
            // reject 也可能附 reuse（模型把近义铸名裁进 reuse、把其余硬伤放
            // 进 problems——2026-10-19 真机实录）。裁决由调用方先行消化：能
            // 被裁决覆盖的问题剔除，剩余才打回；这里丢弃 reuse 会把模型的
            // 正确裁决白白烧进重裁循环。
            accept(verdict.problems.into_iter().take(8).collect())
        } else {
            accept(Vec::new())
        }
    }
    /// Degraded 保守放行（learnhub ADR-0026 同款）：评审器两连败 ≠ 批缺陷
    /// ——零 problems，降级留痕进 findings（j09 通道 → k02 水位消费）。
    fn degraded_outcome(death: &str, readout: provenance::ProvenanceReadout) -> ReviewOutcome {
        ReviewOutcome {
            problems: Vec::new(),
            reuse: Vec::new(),
            findings: vec![ReviewFinding {
                node: String::new(),
                field: "term_provenance".into(),
                issue: format!(
                    "评审挂号表机械校验两连败，结论不可用——按 Degraded 保守放行\
                     （评审器故障 ≠ 批缺陷）；末次死因：{death}"
                ),
                advice: "本批未获挂号表评审背书，留痕供修补回合关注；连续 Degraded 时先查评审模型可用性"
                    .into(),
            }],
            provenance: readout,
        }
    }
    let known: HashSet<String> = known_names.iter().map(|name| name.trim().to_lowercase()).collect();
    let mut feedback = String::new();
    let mut readout = provenance::ProvenanceReadout::default();
    let max_attempts = if provenance_on { REVIEW_PROVENANCE_ATTEMPTS } else { 1 };
    for attempt in 1..=max_attempts {
        let mut user = format!(
            "{}\n\n【待审批次】\n{batch_json}{candidates_block}",
            context.render()
        );
        if !feedback.is_empty() {
            user.push_str(&format!(
                "\n\n【上一轮评审结论不可用——挂号表机械校验未过，恰一次带死因重试】\n{feedback}\n\
                 【重试纪律】逐条修正被点名的挂号表行（缺行补行、错出处改出处）；\
                 problems/reuse/findings 等其余裁决保持稳定，重新输出完整结论 JSON。"
            ));
        }
        // JSON 接缝 + thinking ON（g4：全部生成面统一）。
        let system = concept_review_system();
        let raw = match crate::generation::complete_json_coach(
            completer,
            model_override,
            &system,
            &user,
            crate::generation::CONCEPT_REVIEW_MAX_TOKENS,
            None,
        )
        .await
        {
            Ok(raw) => raw,
            Err(_) => return ReviewOutcome { provenance: readout, ..ReviewOutcome::default() },
        };
        // 评审自身不可用（调用失败或违约）→ 保守放行（语义不变：评审不阻断
        // 结构合法的批次；reuse 随之丢失 = 退化现状）。
        let Ok(value) = crate::generation::parse_json_object::<serde_json::Value>(&raw) else {
            tracing::warn!(
                raw_head = %raw.chars().take(400).collect::<String>(),
                "concept review reply is not a single JSON object — degrading to PASS"
            );
            return ReviewOutcome { provenance: readout, ..ReviewOutcome::default() };
        };
        if growth_contract::validate_review(&value)
            .into_iter()
            .any(|violation| !violation.warning)
        {
            tracing::warn!(
                raw_head = %raw.chars().take(400).collect::<String>(),
                "concept review reply violated its contract — degrading to PASS"
            );
            return ReviewOutcome { provenance: readout, ..ReviewOutcome::default() };
        }
        let verdict = match serde_json::from_value::<Verdict>(value) {
            Ok(verdict) => verdict,
            Err(error) => {
                if !provenance_on {
                    tracing::warn!(
                        error = %error,
                        "concept review reply shape drifted — degrading to PASS"
                    );
                    return ReviewOutcome { provenance: readout, ..ReviewOutcome::default() };
                }
                // 词表外 source 等形状缺陷 = 机械校验看不到的表伪形态，与
                // 行伪同路：恰一次带死因重试（learnhub「结论不可解析」同款）。
                readout.invalid += 1;
                let death = format!("挂号表不可用：结论不可解析（表形状缺陷）：{error}");
                if attempt < max_attempts {
                    readout.retried = 1;
                    feedback = death;
                    tracing::warn!(attempt, "provenance table unparseable — retrying the review once");
                    continue;
                }
                tracing::warn!(death = %death, "provenance table unusable twice — degraded pass");
                readout.degraded = true;
                return degraded_outcome(&death, readout);
            }
        };
        if !provenance_on {
            return verdict_outcome(verdict, readout);
        }
        match provenance::validate_term_provenance(
            &verdict.term_provenance,
            verdict.verdict == "pass",
            batch,
            &known,
        ) {
            Ok(rows) => {
                readout.rows = rows;
                return verdict_outcome(verdict, readout);
            }
            Err(reason) => {
                readout.invalid += 1;
                let death = format!("挂号表不可用:{reason}");
                if attempt < max_attempts {
                    readout.retried = 1;
                    feedback = death;
                    tracing::warn!(
                        attempt,
                        reason = %reason,
                        "provenance validation failed — retrying the review once with the death cause"
                    );
                    continue;
                }
                tracing::warn!(death = %death, "provenance table unusable twice — degraded pass");
                readout.degraded = true;
                return degraded_outcome(&death, readout);
            }
        }
    }
    unreachable!("review attempts loop always returns inside the body")
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

/// 覆盖读数（共用存量，ADR-0009 Amendment 1）：所有节点共用的
/// 「已学达标 / 已跳过 / 未达标」+ 概念档位足迹。纯展示徽章，永不作判定
/// （ADR-0010：终点裁决退役后它不再进任何门）。
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

/// 渲染覆盖读数；存量清空时给出「继续生长」的提示（纯展示徽章——它永不
/// 作判定，覆盖满格也不构成任何自动终态，ADR-0010）。
pub fn render_coverage_gauge(reading: &CoverageReading) -> String {
    let pending = reading.total.saturating_sub(reading.met + reading.skipped);
    let footprint = if reading.tier_footprint.is_empty() {
        "概念足迹 0 枚".to_owned()
    } else {
        let items = reading
            .tier_footprint
            .iter()
            .map(|(tier, count)| format!("{} {}", tier.as_str(), count))
            .collect::<Vec<_>>()
            .join(" / ");
        format!(
            "概念足迹 {} 枚（{items}）",
            reading.tier_footprint.iter().map(|(_, count)| count).sum::<usize>()
        )
    };
    if pending == 0 {
        return format!(
            "存量无待办（已学达标 {}、已跳过 {}；跳过≠学会；{footprint}）——考虑继续生长新节点，或以归档收尾课程。",
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
        ProposedNode { title: title.to_owned(), purpose: String::new(), est: Some(20), difficulty: None, teaches, assumes }
    }

    fn batch_of(nodes: Vec<ProposedNode>) -> ProposedBatch {
        ProposedBatch { nodes, mints: Vec::new(), new_aliases: Vec::new(), note: String::new() }
    }

    // ── Auto Refill（m10）────────────────────────────────────────────────────

    /// 裁决三态（m6 定案阈值不动：READY_TRIGGER=3）：就绪 ≥ 触发水位 =
    /// BelowThreshold（水位以上，无需补批）；就绪 < 水位 = Due（面不够，
    /// 补一批）；开关合力 false = Disabled（恒，水位以下也不再补）。
    #[test]
    fn auto_refill_verdict_three_states() {
        assert_eq!(
            auto_refill_verdict(READY_TRIGGER, true),
            AutoRefillVerdict::BelowThreshold
        );
        assert_eq!(
            auto_refill_verdict(READY_TRIGGER + 2, true),
            AutoRefillVerdict::BelowThreshold
        );
        assert_eq!(
            auto_refill_verdict(READY_TRIGGER - 1, true),
            AutoRefillVerdict::Due
        );
        assert_eq!(auto_refill_verdict(0, true), AutoRefillVerdict::Due);
        assert_eq!(auto_refill_verdict(0, false), AutoRefillVerdict::Disabled);
        assert_eq!(
            auto_refill_verdict(READY_TRIGGER - 1, false),
            AutoRefillVerdict::Disabled
        );
        // 读数面机器名（端点/smoke 读数行同源）。
        assert_eq!(AutoRefillVerdict::Due.as_str(), "due");
        assert_eq!(
            AutoRefillVerdict::BelowThreshold.as_str(),
            "below_threshold"
        );
        assert_eq!(AutoRefillVerdict::Disabled.as_str(), "disabled");
    }

    #[test]
    fn tier_ordering_is_know_apply_teach() {
        assert!(ConceptTier::Know < ConceptTier::Apply);
        assert!(ConceptTier::Apply < ConceptTier::Teach);
        assert_eq!(ConceptTier::try_from_str("apply"), Some(ConceptTier::Apply));
        assert_eq!(ConceptTier::try_from_str("nope"), None);
    }

    #[test]
    fn validate_batch_passes_entry_nodes_without_intra_batch_dependencies() {
        // 入口批：节点各自自带引入所教概念（teaches + 铸名），assumes 全空
        // ——同批无学习先后，谁也不依赖谁，全部同时就绪。
        let mut batch = ProposedBatch {
            new_aliases: Vec::new(),
            nodes: vec![
                node("用提公因式法化简多项式", vec![reference("因式分解", ConceptTier::Know)], vec![]),
                node("用公式法解因式分解题", vec![reference("公式法", ConceptTier::Know)], vec![]),
            ],
            mints: vec![
                ConceptMint { canonical: "因式分解".into(), aliases: vec!["因式拆解".into()], definition: String::new(), source: None },
                ConceptMint { canonical: "公式法".into(), aliases: vec![], definition: String::new(), source: None },
            ],
            note: String::new(),
        };
        let (errors, warns) = validate_batch(&mut batch, &ConceptCoverage::new(), &HashSet::new(), &HashMap::new(), &HashSet::new());
        assert!(errors.is_empty(), "{errors:?}");

        // 依赖既往批次的概念合法：已教账覆盖到所假定档位即放行。
        let mut batch = ProposedBatch {
            new_aliases: Vec::new(),
            nodes: vec![node(
                "用公式法解因式分解题",
                vec![reference("公式法", ConceptTier::Know)],
                vec![reference("因式分解", ConceptTier::Apply)],
            )],
            mints: vec![ConceptMint { canonical: "公式法".into(), aliases: vec![], definition: String::new(), source: None }],
            note: String::new(),
        };
        let mut coverage = ConceptCoverage::new();
        coverage.insert("因式分解".to_owned(), ConceptTier::Apply);
        let mut registry = HashSet::new();
        registry.insert("因式分解".to_owned());
        let (errors, warns) = validate_batch(&mut batch, &coverage, &registry, &HashMap::new(), &HashSet::new());
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn validate_batch_resolves_unique_near_form_ref_with_warn() {
        // 字级名漂移（flash 真机实录：「十进位置值」vs 账内「十进制位值」，
        // 距离 2）不烧整批：唯一近形宿主侧归一（learnhub「系统会核对它属于
        // 哪个既有概念」），warn 留痕、引用就地改写。
        let mut batch = batch_of(vec![node(
            "按位权展开把数写成加法式",
            vec![reference("十进位置值", ConceptTier::Apply)],
            vec![],
        )]);
        let mut registry = HashSet::new();
        registry.insert("十进制位值".to_owned());
        let mut name_owner = HashMap::new();
        name_owner.insert("十进制位值".to_owned(), "十进制位值".to_owned());
        let (errors, warns) =
            validate_batch(&mut batch, &ConceptCoverage::new(), &registry, &name_owner, &HashSet::new());
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(warns.len(), 1, "{warns:?}");
        assert_eq!(warns[0].kind, "ng.refs.nearform");
        assert_eq!(batch.nodes[0].teaches[0].name, "十进制位值");

        // 包含式近形：「幂」唯一命中「二的幂」。
        let mut batch = batch_of(vec![node(
            "用 2 的幂拆数",
            vec![reference("幂", ConceptTier::Apply)],
            vec![],
        )]);
        let mut registry = HashSet::new();
        registry.insert("二的幂".to_owned());
        let mut name_owner = HashMap::new();
        name_owner.insert("二的幂".to_owned(), "二的幂".to_owned());
        let (errors, warns) =
            validate_batch(&mut batch, &ConceptCoverage::new(), &registry, &name_owner, &HashSet::new());
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(warns.len(), 1);
        assert_eq!(batch.nodes[0].teaches[0].name, "二的幂");
    }

    #[test]
    fn validate_batch_rejects_ambiguous_near_form_with_candidates() {
        // 多个近形宿主（「按位展开」同时近形「按位权展开」「按位展开法」）
        // 不能替模型猜：红条打回，消息列候选名。
        let mut batch = batch_of(vec![node(
            "按位展开说意义",
            vec![reference("按位展开", ConceptTier::Apply)],
            vec![],
        )]);
        let mut registry = HashSet::new();
        registry.insert("按位权展开".to_owned());
        registry.insert("按位展开法".to_owned());
        let mut name_owner = HashMap::new();
        name_owner.insert("按位权展开".to_owned(), "按位权展开".to_owned());
        name_owner.insert("按位展开法".to_owned(), "按位展开法".to_owned());
        let (errors, warns) =
            validate_batch(&mut batch, &ConceptCoverage::new(), &registry, &name_owner, &HashSet::new());
        assert_eq!(warns.len(), 0);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, "ng.refs.resolvable");
        assert!(errors[0].message.contains("按位权展开"), "{:?}", errors[0]);
        assert!(errors[0].message.contains("按位展开法"), "{:?}", errors[0]);
    }

    #[test]
    fn validate_batch_keeps_short_name_drift_red() {
        // 两字短名一字差常常是两个真概念（「数位」/「数值」）：归一权留给
        // 模型/评审，不自动归一。
        let mut batch = batch_of(vec![node(
            "认读数位",
            vec![reference("数位", ConceptTier::Apply)],
            vec![],
        )]);
        let mut registry = HashSet::new();
        registry.insert("数值".to_owned());
        let mut name_owner = HashMap::new();
        name_owner.insert("数值".to_owned(), "数值".to_owned());
        let (errors, warns) =
            validate_batch(&mut batch, &ConceptCoverage::new(), &registry, &name_owner, &HashSet::new());
        assert_eq!(warns.len(), 0);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, "ng.refs.resolvable");
        assert_eq!(batch.nodes[0].teaches[0].name, "数位");
    }

    #[tokio::test]
    async fn concept_review_keeps_reuse_rulings_on_reject() {
        // 真机实录（run-1792385060145 call 3）：模型把近义铸名裁进 reuse、
        // 其余硬伤放 problems、verdict=reject——旧解析在 reject 分支丢弃
        // reuse，模型的正确裁决被烧进重裁循环。reject 必须保留 reuse，由
        // 服务层先消化裁决（能被裁决覆盖的问题剔除）再谈打回。
        struct Fixed(std::sync::Mutex<std::vec::IntoIter<String>>);
        #[async_trait::async_trait]
        impl LearningCompleter for Fixed {
            async fn complete(
                &self,
                _: Option<(&str, &str)>,
                _: &str,
                _: &str,
                _: u32,
            ) -> Result<String, nomifun_common::AppError> {
                unreachable!("review only uses the json seam")
            }
            async fn complete_json_with_thinking(
                &self,
                _: Option<(&str, &str)>,
                _: &str,
                _: &str,
                _: u32,
                _: Option<f32>,
            ) -> Result<String, nomifun_common::AppError> {
                let mut replies = self.0.lock().unwrap();
                replies.next().ok_or_else(|| nomifun_common::AppError::Internal("script exhausted".into()))
            }
        }
        let reply = serde_json::json!({
            "verdict": "reject",
            "problems": [
                { "node": "从 0 到 15 逐个写出二进制数", "field": "teaches",
                  "issue": "铸名「二进制数」与已教概念「二进制数字」内涵重叠",
                  "advice": "把「二进制数」移入 reuse" },
                { "node": "反复除以 2", "field": "difficulty",
                  "issue": "内容量超过 difficulty 3 的负载", "advice": "拆分或上调" }
            ],
            "reuse": [ { "name": "二进制数", "concept_id": "01a1442d-2338-7a40-a4bd-fe50a0a64a83" } ]
        });
        let completer = Fixed(std::sync::Mutex::new(vec![
            serde_json::to_string(&reply).unwrap(),
        ].into_iter()));
        let batch = batch_of(vec![node(
            "从 0 到 15 逐个写出二进制数",
            vec![reference("二进制数", ConceptTier::Apply)],
            vec![],
        )]);
        let review =
            concept_review(&completer, None, &batch, &CoachContext::default(), "", &[]).await;
        assert_eq!(review.problems.len(), 2, "{review:?}");
        assert_eq!(review.reuse.len(), 1, "{review:?}");
        assert_eq!(review.reuse[0].name, "二进制数");
        assert_eq!(review.reuse[0].concept_id, "01a1442d-2338-7a40-a4bd-fe50a0a64a83");
    }

    /// k07 开关两臂：on 臂判据①为子查结构（登记表外名词/批内提名不算
    /// 出处/内嵌上位词在审/两可词按概念处理）；off 臂逐字回到旧并句措辞。
    /// env 是进程全局态：`#[serial]` 与其它评审判据测试互斥。
    #[test]
    #[serial_test::serial]
    fn concept_review_system_switch_selects_the_criterion_one_wording() {
        struct Restore(Option<String>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => unsafe { std::env::set_var(REVIEW_V2_SWITCH_ENV, value) },
                    None => unsafe { std::env::remove_var(REVIEW_V2_SWITCH_ENV) },
                }
            }
        }
        let previous = std::env::var(REVIEW_V2_SWITCH_ENV).ok();
        let _restore = Restore(previous);
        // 本测试锁 k07 两臂措辞：把 m3 挂号表臂固定在 off（表臂有自己的测试）。
        let _provenance = set_env(REVIEW_PROVENANCE_SWITCH_ENV, "off");

        unsafe { std::env::set_var(REVIEW_V2_SWITCH_ENV, "on") };
        let system = concept_review_system();
        for phrase in [
            "登记表外名词",
            "批内提名本身不是出处",
            "内嵌的上位词同样在审",
            "按概念处理",
            "修法",
        ] {
            assert!(system.contains(phrase), "on 臂缺「{phrase}」");
        }
        assert!(!system.contains("三处皆无的名词"), "on 臂不出现旧并句");

        unsafe { std::env::set_var(REVIEW_V2_SWITCH_ENV, "off") };
        let legacy = concept_review_system();
        assert!(legacy.contains("三处皆无的名词"), "off 臂是新版");
        assert!(!legacy.contains("批内提名本身不是出处"));
        assert!(!legacy.contains("内嵌的上位词同样在审"));
        // 换臂只动判据①：②③④与 findings 通道两臂同文。
        for prompt in [&system, &legacy] {
            assert!(prompt.contains("前提足够"), "判据②不随开关变");
            assert!(prompt.contains("批内兄弟节点教出的概念不算已教"));
            assert!(prompt.contains("命名与内涵一致"), "判据③不随开关变");
            assert!(prompt.contains("难度诚实"), "判据④不随开关变");
            assert!(prompt.contains("findings——数据缺陷留痕"), "findings 通道不动");
        }
        // 开关取值判定只认 off/0/false。
        for value in ["off", "OFF", "0", "false", " False "] {
            assert!(is_review_v2_disabled_value(value));
        }
        for value in ["on", "1", "true", "", "disable"] {
            assert!(!is_review_v2_disabled_value(value));
        }
    }

    /// m3 挂号表臂：评审系统提示词织入表格条款——判据①改「按表核对」（V2
    /// 框定/四入口措辞保留为表的语义说明）+ 输出契约段（来源四值/逐字规则/
    /// 同形词盲区）。条款覆盖短语断言：改词表/删条款 = 本测试红。
    #[test]
    #[serial_test::serial]
    fn provenance_arm_weaves_the_table_contract_into_the_review_prompt() {
        let _provenance = set_env(REVIEW_PROVENANCE_SWITCH_ENV, "on");
        let _v2 = set_env(REVIEW_V2_SWITCH_ENV, "on");
        let system = concept_review_system();
        for phrase in [
            // 表键与来源四值（缺失承重）。
            "term_provenance",
            "已教|自教|常识|缺失",
            // 判据①程序句（V2 框定措辞保留为语义说明）。
            "逐词落行",
            "登记表外名词",
            "批内提名本身不是出处",
            "内嵌的上位词同样在审",
            // 机械核对条款的提示词面：循环自证/本批提名不当已教出处/缺失
            // 自认通道/逐字落地/全覆盖/结论无效。
            "循环自证",
            "本批产出不是已教出处",
            "洗成常识",
            "逐字取自该提名定义原文",
            "至少一行",
            "结论无效",
            // 同形词盲区注记（#176）。
            "同形词盲区",
        ] {
            assert!(system.contains(phrase), "挂号表臂缺「{phrase}」");
        }
        // 表臂不出现 V1 旧并句（V1 把「随批铸名」算出处，与表矛盾）。
        assert!(!system.contains("三处皆无的名词"));

        // off 臂零表条款：不要求表 = k07 后行为（提示词与机械校验都不在）。
        let _off = set_env(REVIEW_PROVENANCE_SWITCH_ENV, "off");
        let plain = concept_review_system();
        for phrase in [
            "term_provenance",
            "已教|自教|常识|缺失",
            "逐词落行",
            "同形词盲区",
        ] {
            assert!(!plain.contains(phrase), "off 臂不得出现「{phrase}」");
        }
        assert!(plain.contains("登记表外名词"), "off 臂照常走 V2 措辞");
    }

    /// k07 挂号 finding 走通 j09 warn 留痕通道的解析面：pass 结论附带的
    /// findings（挂号类 issue）原样透出，不阻断批次。
    #[tokio::test]
    async fn concept_review_passes_registration_findings_through() {
        struct Fixed(std::sync::Mutex<Option<String>>);
        #[async_trait::async_trait]
        impl LearningCompleter for Fixed {
            async fn complete(
                &self,
                _: Option<(&str, &str)>,
                _: &str,
                _: &str,
                _: u32,
            ) -> Result<String, nomifun_common::AppError> {
                unreachable!("review only uses the json seam")
            }
            async fn complete_json_with_thinking(
                &self,
                _: Option<(&str, &str)>,
                _: &str,
                _: &str,
                _: u32,
                _: Option<f32>,
            ) -> Result<String, nomifun_common::AppError> {
                self.0
                    .lock()
                    .unwrap()
                    .take()
                    .ok_or_else(|| nomifun_common::AppError::Internal("script exhausted".into()))
            }
        }
        let reply = serde_json::json!({
            "verdict": "pass",
            "problems": [],
            "findings": [{
                "node": "校准研磨度",
                "field": "mints",
                "issue": "概念名内嵌的上位词「研磨度」未在册、无人教，按概念处理属挂号缺失（疑似数据面缺概念）",
                "advice": "补教「研磨度」或把该节点并入已教概念"
            }]
        });
        let completer = Fixed(std::sync::Mutex::new(Some(
            serde_json::to_string(&reply).unwrap(),
        )));
        let batch = batch_of(vec![node("校准研磨度", vec![], vec![])]);
        let review =
            concept_review(&completer, None, &batch, &CoachContext::default(), "", &[]).await;
        assert!(review.problems.is_empty(), "pass 不阻断：{review:?}");
        assert_eq!(review.findings.len(), 1, "{review:?}");
        assert!(review.findings[0].issue.contains("内嵌的上位词"));
        assert_eq!(review.findings[0].node, "校准研磨度");
    }

    // ── 评审挂号表 term_provenance（m3）：恰一次带死因重试 → Degraded ──────

    /// 脚本化评审 completer：按序吐回包并记录每次 prompt（重试反馈的死因
    /// 断言面）。
    struct ScriptedReview(std::sync::Mutex<(std::vec::IntoIter<String>, Vec<String>)>);
    #[async_trait::async_trait]
    impl LearningCompleter for ScriptedReview {
        async fn complete(
            &self,
            _: Option<(&str, &str)>,
            _: &str,
            _: &str,
            _: u32,
        ) -> Result<String, nomifun_common::AppError> {
            unreachable!("review only uses the json seam")
        }
        async fn complete_json_with_thinking(
            &self,
            _: Option<(&str, &str)>,
            system: &str,
            user: &str,
            _: u32,
            _: Option<f32>,
        ) -> Result<String, nomifun_common::AppError> {
            let mut guard = self.0.lock().unwrap();
            guard.1.push(format!("{system}\n\x1f{user}"));
            guard
                .0
                .next()
                .ok_or_else(|| nomifun_common::AppError::Internal("script exhausted".into()))
        }
    }

    /// 挂号表集成测试批：节点 teaches「所有权」（自教域），铸名「借用」
    /// （定义含 所有权/引用/作用域）。
    fn review_batch_with_mint() -> ProposedBatch {
        ProposedBatch {
            nodes: vec![node("判读借用", vec![reference("所有权", ConceptTier::Know)], vec![])],
            mints: vec![ConceptMint {
                canonical: "借用".into(),
                aliases: Vec::new(),
                definition: "所有权不变式下多读一写：引用随作用域在编译期检查。".into(),
                source: None,
            }],
            new_aliases: Vec::new(),
            note: String::new(),
        }
    }

    fn pass_with_table(rows: serde_json::Value) -> String {
        serde_json::to_string(&serde_json::json!({
            "verdict": "pass",
            "problems": [],
            "term_provenance": rows,
        }))
        .unwrap()
    }

    fn legal_table() -> serde_json::Value {
        serde_json::json!([
            { "concept": "借用", "term": "所有权", "source": "已教", "ref": "所有权" },
            { "concept": "借用", "term": "作用域", "source": "常识" }
        ])
    }

    /// 环境守卫：评审开关是进程全局态（`#[serial]` 互斥 + Drop 恢复原值）。
    fn set_env(env_var: &'static str, value: &str) -> impl Drop {
        struct Restore(&'static str, Option<String>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match self.1.take() {
                    Some(value) => unsafe { std::env::set_var(self.0, value) },
                    None => unsafe { std::env::remove_var(self.0) },
                }
            }
        }
        let previous = std::env::var(env_var).ok();
        unsafe { std::env::set_var(env_var, value) };
        Restore(env_var, previous)
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn missing_table_retries_once_then_degrades_with_finding() {
        let _env = set_env(REVIEW_PROVENANCE_SWITCH_ENV, "on");
        let completer = ScriptedReview(std::sync::Mutex::new((
            vec![
                serde_json::to_string(&serde_json::json!({ "verdict": "pass", "problems": [] })).unwrap(),
                serde_json::to_string(&serde_json::json!({ "verdict": "pass", "problems": [] })).unwrap(),
            ]
            .into_iter(),
            Vec::new(),
        )));
        let review = concept_review(
            &completer,
            None,
            &review_batch_with_mint(),
            &CoachContext::default(),
            "",
            &["所有权".to_owned()],
        )
        .await;
        // Degraded 保守放行：零 problems（评审器故障 ≠ 批缺陷），降级留痕
        // 走 findings（调用方落 j09 通道 → k02 水位消费）。
        assert!(review.problems.is_empty(), "{review:?}");
        assert!(review.reuse.is_empty(), "{review:?}");
        assert_eq!(review.findings.len(), 1, "降级必须留痕：{review:?}");
        assert!(
            review.findings[0].issue.contains("Degraded") && review.findings[0].issue.contains("缺挂号表行"),
            "留痕带死因：{:?}",
            review.findings[0]
        );
        assert_eq!(
            review.provenance,
            provenance::ProvenanceReadout { rows: 0, invalid: 2, retried: 1, degraded: true }
        );
        let prompts = &completer.0.lock().unwrap().1;
        assert_eq!(prompts.len(), 2, "重试恰一次：{prompts:?}");
        assert!(prompts[1].contains("缺挂号表行"), "次轮须携带死因：{}", prompts[1]);
        assert!(prompts[1].contains("挂号表不可用"), "次轮须点名挂号表：{}", prompts[1]);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn first_invalid_second_valid_recovers_with_rows() {
        let _env = set_env(REVIEW_PROVENANCE_SWITCH_ENV, "on");
        let completer = ScriptedReview(std::sync::Mutex::new((
            vec![
                serde_json::to_string(&serde_json::json!({ "verdict": "pass", "problems": [] })).unwrap(),
                pass_with_table(legal_table()),
            ]
            .into_iter(),
            Vec::new(),
        )));
        let review = concept_review(
            &completer,
            None,
            &review_batch_with_mint(),
            &CoachContext::default(),
            "",
            &["所有权".to_owned()],
        )
        .await;
        assert!(review.problems.is_empty(), "{review:?}");
        assert!(review.findings.is_empty(), "恢复则不留痕：{review:?}");
        assert_eq!(
            review.provenance,
            provenance::ProvenanceReadout { rows: 2, invalid: 1, retried: 1, degraded: false }
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn off_arm_requires_no_table_and_makes_one_call() {
        let _env = set_env(REVIEW_PROVENANCE_SWITCH_ENV, "off");
        let completer = ScriptedReview(std::sync::Mutex::new((
            vec![
                serde_json::to_string(&serde_json::json!({ "verdict": "pass", "problems": [] })).unwrap(),
            ]
            .into_iter(),
            Vec::new(),
        )));
        let review = concept_review(
            &completer,
            None,
            &review_batch_with_mint(),
            &CoachContext::default(),
            "",
            &["所有权".to_owned()],
        )
        .await;
        // off 臂 = k07 后行为：不要求表、不做机械校验，缺表照常放行。
        assert!(review.problems.is_empty(), "{review:?}");
        assert!(review.findings.is_empty(), "{review:?}");
        assert_eq!(
            review.provenance,
            provenance::ProvenanceReadout::default(),
            "off 臂零校验零读数"
        );
        assert_eq!(completer.0.lock().unwrap().1.len(), 1, "off 臂单次调用");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn unparseable_source_word_is_a_retryable_table_defect() {
        // 词表外 source = 机械校验看不到的表伪形态（serde 解析失败），与行伪
        // 同路：恰一次带死因重试。
        let _env = set_env(REVIEW_PROVENANCE_SWITCH_ENV, "on");
        let completer = ScriptedReview(std::sync::Mutex::new((
            vec![
                pass_with_table(serde_json::json!([
                    { "concept": "借用", "term": "作用域", "source": "大概会" }
                ])),
                pass_with_table(legal_table()),
            ]
            .into_iter(),
            Vec::new(),
        )));
        let review = concept_review(
            &completer,
            None,
            &review_batch_with_mint(),
            &CoachContext::default(),
            "",
            &["所有权".to_owned()],
        )
        .await;
        assert!(review.problems.is_empty(), "{review:?}");
        assert!(review.findings.is_empty(), "次轮干净结论恢复：{review:?}");
        assert_eq!(
            review.provenance,
            provenance::ProvenanceReadout { rows: 2, invalid: 1, retried: 1, degraded: false }
        );
        let prompts = &completer.0.lock().unwrap().1;
        assert!(
            prompts[1].contains("挂号表不可用"),
            "次轮须携带表形状缺陷死因：{}",
            prompts[1]
        );
    }

    #[test]
    #[serial_test::serial]
    fn provenance_switch_value_predicate_follows_off_convention() {
        struct Restore(Option<String>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => unsafe { std::env::set_var(REVIEW_PROVENANCE_SWITCH_ENV, value) },
                    None => unsafe { std::env::remove_var(REVIEW_PROVENANCE_SWITCH_ENV) },
                }
            }
        }
        let _restore = Restore(std::env::var(REVIEW_PROVENANCE_SWITCH_ENV).ok());
        for value in ["off", "OFF", "0", "false", " False "] {
            assert!(is_review_provenance_disabled_value(value), "{value:?} 关闭");
        }
        for value in ["on", "1", "true", "", "disable"] {
            assert!(!is_review_provenance_disabled_value(value), "{value:?} 不关闭");
        }
        // 未设 = 开启（默认臂）。
        unsafe { std::env::remove_var(REVIEW_PROVENANCE_SWITCH_ENV) };
        assert!(review_provenance_enabled());
        unsafe { std::env::set_var(REVIEW_PROVENANCE_SWITCH_ENV, "off") };
        assert!(!review_provenance_enabled());
    }

    #[test]
    fn validate_batch_rejects_intra_batch_dependency() {
        // 同批无先后、必须同时就绪：本批兄弟的 teaches 不构成 assumes 的供给
        // ——即使概念已随批铸名、也被兄弟教到该档位，依赖它的节点必须留到下一批。
        let mut batch = ProposedBatch {
            new_aliases: Vec::new(),
            nodes: vec![
                node("用提公因式法化简多项式", vec![reference("因式分解", ConceptTier::Know)], vec![]),
                node("用公式法解因式分解题", vec![reference("公式法", ConceptTier::Know)], vec![reference("因式分解", ConceptTier::Know)]),
            ],
            mints: vec![
                ConceptMint { canonical: "因式分解".into(), aliases: vec!["因式拆解".into()], definition: String::new(), source: None },
                ConceptMint { canonical: "公式法".into(), aliases: vec![], definition: String::new(), source: None },
            ],
            note: String::new(),
        };
        let (errors, warns) = validate_batch(&mut batch, &ConceptCoverage::new(), &HashSet::new(), &HashMap::new(), &HashSet::new());
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, "ng.assumes.uncovered");
    }

    #[test]
    fn validate_batch_rejects_uncovered_assumption_instead_of_locking() {
        // 会用-level assumption with only a 知道-level taught coverage: the
        // gate bounces the whole batch (no locked state may ever publish).
        // 该概念已在登记表（生产不变量：coverage 内的概念必然已登记），
        // 唯一打回理由就是假定未被覆盖。
        let mut batch = batch_of(vec![node(
            "应用勾股定理解题",
            vec![],
            vec![reference("勾股定理", ConceptTier::Apply)],
        )]);
        let mut coverage = ConceptCoverage::new();
        coverage.insert("勾股定理".to_owned(), ConceptTier::Know);
        let mut registry = HashSet::new();
        registry.insert("勾股定理".to_owned());
        let (errors, warns) = validate_batch(&mut batch, &coverage, &registry, &HashMap::new(), &HashSet::new());
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, "ng.assumes.uncovered");
    }

    #[test]
    fn validate_batch_enforces_course_internal_title_uniqueness() {
        // 批量上限属形状契约（growth_contract）；这里只测需要 DB 事实的
        // 标题重复的机械摘除。
        let mut existing = HashSet::new();
        existing.insert("已有节点".to_owned());
        let mut batch = batch_of(vec![
            node("已有节点", vec![], vec![]),
            node("新节点", vec![], vec![]),
            node("新节点", vec![], vec![]),
        ]);
        let (errors, warns) = validate_batch(
            &mut batch,
            &ConceptCoverage::new(),
            &HashSet::new(),
            &HashMap::new(),
            &existing,
        );
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(warns.len(), 2, "{warns:?}");
        assert!(warns.iter().all(|warn| warn.kind == "ng.title.pruned"));
        assert_eq!(batch.nodes.len(), 1);
        assert_eq!(batch.nodes[0].title, "新节点");

        // 形状契约（细单测在本模块 growth_contract）：同一批 8 个节点。
        let nodes: Vec<serde_json::Value> = (0..8)
            .map(|index| serde_json::json!({ "title": format!("节点{index}"), "purpose": "p" }))
            .collect();
        let value = serde_json::json!({ "nodes": nodes, "mints": [] });
        assert!(
            growth_contract::validate_proposal(&value)
                .iter()
                .any(|violation| violation.code == "ng.batch.size")
        );
    }

    #[test]
    fn validate_batch_rejects_unresolved_concepts_and_mint_collisions() {
        let mut registry = HashSet::new();
        registry.insert("既有概念".to_owned());
        let mut batch = ProposedBatch {
            new_aliases: Vec::new(),
            nodes: vec![node("新节点", vec![reference("既有概念", ConceptTier::Know)], vec![reference("幽灵概念", ConceptTier::Know)])],
            mints: vec![ConceptMint { canonical: "既有概念".into(), aliases: vec![], definition: String::new(), source: None }],
            note: String::new(),
        };
        let (errors, warns) = validate_batch(&mut batch, &ConceptCoverage::new(), &registry, &HashMap::new(), &HashSet::new());
        assert!(errors.iter().any(|error| error.kind == "ng.mints.collision"));
        assert!(errors.iter().any(|error| error.kind == "ng.refs.resolvable"));
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

    /// t7 反转：中文档位（知道/会用/能教）与 est 字符串数字在契约核验前
    /// 被机械归一（可机械判定的不送模型——真机 round 4 死链的引信），
    /// 回包照常装填；语义违规（est 超范围）仍打回，归一不造值。
    #[test]
    fn parse_coach_reply_normalizes_chinese_tiers_and_string_est() {
        let raw = r#"{
            "nodes": [
                { "title": "用配方法解方程", "purpose": "p", "est": "15",
                  "teaches": [{"name": "配方法", "tier": "会用"}],
                  "assumes": [{"name": "一元二次方程", "tier": "知道"}] }
            ],
            "mints": [],
            "note": "n"
        }"#;
        let batch = parse_coach_reply(raw).unwrap();
        assert_eq!(batch.nodes[0].est, Some(15), "string est digits normalize");
        assert_eq!(batch.nodes[0].teaches[0].tier, ConceptTier::Apply);
        assert_eq!(batch.nodes[0].assumes[0].tier, ConceptTier::Know);
        // 语义违规不受归一庇护：est "0" 解析为 0 后仍被范围门打回。
        let out_of_range = r#"{
            "nodes": [
                { "title": "用配方法解方程", "purpose": "p", "est": "0",
                  "teaches": [{"name": "配方法", "tier": "会用"}], "assumes": [] }
            ],
            "mints": [],
            "note": "n"
        }"#;
        let error = parse_coach_reply(out_of_range).unwrap_err();
        assert!(
            error.to_string().contains("ng.nodes.est"),
            "range gate still fires after normalization: {error}"
        );
        // 未知档位词（非中英词表）仍打回——归一只认可机械判定的映射。
        let unknown_tier = r#"{
            "nodes": [
                { "title": "用配方法解方程", "purpose": "p", "est": 15,
                  "teaches": [{"name": "配方法", "tier": "熟练"}], "assumes": [] }
            ],
            "mints": [],
            "note": "n"
        }"#;
        let error = parse_coach_reply(unknown_tier).unwrap_err();
        assert!(
            error.to_string().contains("ng.refs.tier"),
            "unknown tier words stay red: {error}"
        );
    }

    /// 退役键名：`minutes` 不在节点键白名单，旧习惯回潮按未知键打回。
    #[test]
    fn retired_minutes_key_is_an_unknown_key_violation() {
        let raw = r#"{
            "nodes": [
                { "title": "用配方法解方程", "purpose": "p", "minutes": 15,
                  "teaches": [{"name": "配方法", "tier": "apply"}], "assumes": [] }
            ],
            "mints": [],
            "note": "n"
        }"#;
        let error = parse_coach_reply(raw).unwrap_err();
        assert!(error.to_string().contains("ng.keys"), "{error}");
    }

    /// 严格路径的正向：契约合规的回包按结构装填并做 trim 归一。
    #[test]
    fn parse_coach_reply_accepts_contract_compliant_reply() {
        let raw = r#"{
            "nodes": [
                { "title": " 用配方法解方程 ", "purpose": " 从配方经验切入 ",
                  "est": 15, "difficulty": 3,
                  "teaches": [{"name": " 配方法 ", "tier": "apply"}],
                  "assumes": [{"name": "一元二次方程", "tier": "know"}] }
            ],
            "mints": [ { "canonical": " 配方法 ", "aliases": [" 配方 "], "definition": "" } ],
            "note": " n "
        }"#;
        let batch = parse_coach_reply(raw).unwrap();
        assert_eq!(batch.nodes.len(), 1);
        assert_eq!(batch.nodes[0].title, "用配方法解方程");
        assert_eq!(batch.nodes[0].est, Some(15));
        assert_eq!(batch.nodes[0].teaches[0].tier, ConceptTier::Apply);
        assert_eq!(batch.nodes[0].assumes[0].tier, ConceptTier::Know);
        assert_eq!(batch.mints[0].canonical, "配方法");
        assert_eq!(batch.mints[0].aliases, vec!["配方"]);
        assert_eq!(batch.note, "n");
    }

    #[test]
    fn parse_coach_reply_rejects_garbage() {
        assert!(parse_coach_reply("sure, here is the plan...").is_err());
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

    /// 覆盖读数：存量构成 + 档位足迹 + 清空态提示（纯展示，不作判定）。
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
        // t7：教练面档位词表改英文。
        assert!(text.contains("apply 5"), "{text}");
        assert!(text.contains("know 3"), "{text}");

        let cleared = render_coverage_gauge(&CoverageReading {
            total: 5,
            met: 4,
            skipped: 1,
            pending_titles: vec![],
            tier_footprint: vec![],
        });
        assert!(cleared.contains("存量无待办"), "{cleared}");
        assert!(cleared.contains("以归档收尾"), "{cleared}");
    }

    #[test]
    fn coach_context_renders_the_gap_contract() {
        let context = CoachContext { ready_count: 2, ..Default::default() };
        let rendered = context.render();
        // t7：就绪目标 7→5（缺口 = 5 − 2 = 3）；数量契约细节仍只住系统提示词。
        assert!(rendered.contains("缺口 3 个"), "{rendered}");
        assert!(rendered.contains("把就绪节点补到 5 个") == false, "the number contract lives in the system prompt");
        assert!(coach_system().contains("把就绪节点补到 5 个"));
        assert!(coach_system().contains("至多 5 个"));
    }

    /// k06（learnhub a39d36a 深度本位修订）：档位自省以教学深度本身为准、
    /// 不对照动词字面——操作训练节点全标 know = 低估信号（能按程序做出来
    /// 至少 apply）；首教形式化「写」只是认识步时 know 也如实。
    #[test]
    fn coach_system_tier_self_review_is_depth_based() {
        let system = coach_system();
        assert!(system.contains("教学深度本身"), "{system}");
        assert!(system.contains("不对照动词字面"), "{system}");
        assert!(system.contains("至少该是 apply"), "{system}");
        assert!(system.contains("首教形式化"), "{system}");
    }

    /// 首批九块渲染：结构参考材料 + 地形门口（首批重点框架语）+ 凝练纪律三行；
    /// 非首批不渲染首批纪律、门口换软参照框架语。
    #[test]
    fn coach_context_renders_structure_reference_and_first_batch_discipline() {
        let with_material = CoachContext {
            roster: String::new(),
            structure_reference: "- 块A(a)|主题描述|入门|主干一|无前置".into(),
            terrain_doors: "- [G1]（硬前置）能用配方法解形如 x²=p 的方程".into(),
            ..Default::default()
        };
        let rendered = with_material.render();
        assert!(rendered.contains("结构参考材料"), "{rendered}");
        assert!(rendered.contains("- 块A(a)|主题描述|入门|主干一|无前置"), "{rendered}");
        assert!(rendered.contains("首批入口重点参照"), "{rendered}");
        assert!(rendered.contains("能用配方法解形如 x²=p 的方程"), "{rendered}");
        assert!(rendered.contains("首批纪律"), "{rendered}");
        assert!(rendered.contains("assumes 必须全空"), "empty-ledger mechanical floor: {rendered}");

        // 老学习者开新课：账非空 × 花名册为空是常态——首批纪律换跨课程口径，
        // 机械地板让位，账标注全局来源（评审/教练不再读成材料矛盾）。
        let veteran = CoachContext {
            roster: String::new(),
            taught_summary: "二的幂@会用、按位权展开@会用".into(),
            coverage_gauge: "已学达标 0 / 已跳过 0 / 未达标 0；概念档位足迹：无".into(),
            ..Default::default()
        };
        let rendered = veteran.render();
        assert!(rendered.contains("跨课程全局"), "{rendered}");
        assert!(!rendered.contains("assumes 必须全空"), "ledger non-empty: no mechanical floor: {rendered}");
        assert!(rendered.contains("本批 assumes 可假定账内已有概念"), "{rendered}");
        assert!(rendered.contains("本课程存量口径"), "{rendered}");

        let doorless = CoachContext { roster: String::new(), ..Default::default() };
        let rendered = doorless.render();
        assert!(rendered.contains("首批纪律"), "{rendered}");
        assert!(!rendered.contains("前置地形门口"), "no doors block when snapshot missing: {rendered}");

        let later_batch = CoachContext {
            roster: "在学 | 节点A".into(),
            terrain_doors: "- [G1]（硬前置）能用配方法解方程".into(),
            ..Default::default()
        };
        let rendered = later_batch.render();
        assert!(!rendered.contains("首批纪律"), "later batches: no first-batch discipline");
        assert!(rendered.contains("软参照"), "later batches: doors read as soft reference: {rendered}");
    }
}
