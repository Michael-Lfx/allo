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
//! The coach is a SINGLE LLM call (the whole roster + coverage gauge +
//! behavior digest is injected into the prompt; a growth batch is ≤7 nodes,
//! far too small to justify the old multi-round draft loop). Deterministic
//! gates hold the decision power; one repair retry gets the gate report,
//! then an independent AI concept review runs (advisory-but-blocking once,
//! degraded to pass when unavailable).
//!
//! The knowledge STRUCTURE artifact (spines/blocks/edges snapshot, gd.*
//! contract) lives in [`structure_contract`] and stays reference-only: it
//! feeds no gate, holds no content, and never touches the concept web.

pub mod growth_contract;
pub mod structure_contract;
pub mod structure_gen;
pub mod structure_view;
pub mod structure_prompts;

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
}

/// 一批生长提案：节点 + 铸名 + 批注。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedBatch {
    #[serde(default)]
    pub nodes: Vec<ProposedNode>,
    #[serde(default)]
    pub mints: Vec<ConceptMint>,
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
/// - 铸名不撞登记表（含别名，大小写折叠）；
/// - 概念引用可解析：teaches/assumes 的名字必须是登记表现有概念（含别名）
///   或本批铸名；
/// - **就绪即发布**：每条 assumes 必须被全局已教账（既往批次已落库的
///   teaches）以不低于所假定档位覆盖——同批兄弟的 teaches 与本批铸名不构成
///   供给：同批节点没有学习先后、必须同时就绪，谁也不依赖谁。不满足的节点
///   不存在「入库后锁定」，直接打回；
/// - 课程内节点标题唯一。
pub fn validate_batch(
    batch: &ProposedBatch,
    coverage: &ConceptCoverage,
    registry_names: &HashSet<String>,
    existing_titles: &HashSet<String>,
) -> Vec<GateError> {
    let mut errors = Vec::new();

    // 批内铸名预备账：canonical → 该概念在本批可引用的名字集合（批内唯一性
    // 已由形状契约保证）。
    let mut mint_names: HashSet<String> = HashSet::new();
    for mint in &batch.mints {
        let canonical = mint.canonical.trim();
        if registry_names.contains(&canonical.to_lowercase()) {
            errors.push(GateError {
                kind: "ng.mints.collision",
                message: format!("铸名「{canonical}」与概念登记表现有名字冲突"),
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
                    message: format!("铸名「{alias}」（{canonical} 的别名）与登记表现有名字冲突"),
                });
            }
            mint_names.insert(alias.to_lowercase());
        }
    }

    for node in &batch.nodes {
        let title = node.title.trim();
        let key = title.to_lowercase();
        if existing_titles.contains(&key) {
            errors.push(GateError {
                kind: "ng.title.duplicate",
                message: format!("节点标题与课程已有节点重复：{title}"),
            });
        }

        for concept in node.teaches.iter().chain(node.assumes.iter()) {
            let name = concept.name.trim();
            let key = name.to_lowercase();
            let resolvable = registry_names.contains(&key) || mint_names.contains(&key);
            if !resolvable {
                errors.push(GateError {
                    kind: "ng.refs.resolvable",
                    message: format!(
                        "节点 {title} 引用的概念「{name}」既不在概念登记表，也未随本批铸名——先铸名或改用登记表中的名字"
                    ),
                });
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
                        assumed = tier_zh(assumption.tier),
                        hint = "同批兄弟的 teaches 与本批铸名不算供给（同批无先后、必须同时就绪）：把本节点留到下一批，本批可先长教该概念的铺垫节点",
                    ),
                });
            }
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
      "minutes": 20,
      "difficulty": 2,
      "teaches": [ { "name": "概念名", "tier": "apply" } ],
      "assumes": [ { "name": "概念名（必须已被既往批次教到该档位；本批兄弟不算）", "tier": "know" } ]
    }
  ],
  "mints": [ { "canonical": "新概念名（只登记名字，铸名≠教）", "aliases": ["别名"], "definition": "一句话定义" } ],
  "note": "一句话批注：本批为什么长这些节点"
}
【档位】
- know（知道）：能辨认、复述。apply（会用）：能解题、应用。teach（能教）：能讲解、纠错。
- teaches 标本课把该概念教到什么档位；assumes 标本课假定学习者已把该概念掌握到什么档位。
- assumes 必须被已教账（既往批次已落库的 teaches）以不低于所假定档位覆盖，否则整批被打回——不存在的「先锁后面再补」。同批兄弟的 teaches 与本批铸名不构成供给：同批节点没有学习先后、必须同时就绪，谁也不依赖谁。
【起点纪律】
- 学习者的真实起点 = 【学习目标】里的基础声明（未声明即彻底零基础）。判断一个节点是不是合格入口，看基线，不看它在教材里的位置：「简单」「基础」「入门」「第一章」都不等于零基础可起步。
- 零基础校验：入口节点直接用到的每个概念，要么本节点就教它（写进 teaches、随批铸名），要么属于课程外通用常识（时间、方位、日常活动一类）；既像常识又像本课概念的两可词，按概念对待。
- 「入门教材第一章」不等于零基础入口：第一章默认读者已带着该领域的日常底子，零基础学习者没有——这些概念只是离目标更近的台阶，不是入口。例：目标「业余段位围棋」的真入口是「数出棋盘上一块棋的气」，而不是「用大雪崩定式处理小目挂角」；目标「家庭手冲咖啡」的真入口是「按粉水比称出一次冲煮的粉与水」，而不是「用三段注水矫正萃取率」。
- 入口素材从学习目标「往下问」得来：目标只是路线的上层，它依赖的通用语言——操作工具、认读记号、基本量与单位、分类与命名、简单判断与比较——才是入口层的素材。对目标逐层往下追问「基线学习者现在连什么都不会，第一步从哪算起」，把最底层那一格摆出来；起点够不够低的唯一标准是基线学习者今天拿到节点就能开始，而不是它长得像该方向的入门课。
- 方向感优先级：学习者当下最缺的台阶优先——贴着概念网的最前沿往目标方向长，不直线冲终点。为逼近目标而长出基线够不着的节点，是比停摆更坏的错误。
- 慢就是快：横向补实与向下补基同为推进——发现已教概念脚下悬空（教了它，但它依赖的通用语言没人教过）时，补教那个底座也是朝目标的实质推进；坡道的质量与坡道的长度同等重要，不以前进牺牲认知依赖的完整性。
【节点纪律】
- 节点是单次学习会话（动作句），常规 5-30 分钟，极困难最多 60 分钟。
- 日常经验切口：每个节点的 purpose 必须点出从哪段日常经验引入——但切口只是引入方式，节点内容永远是学科概念：教的是把日常能力升级为学科语言的那一格（例：围棋课把「把什么归为一类」的分类经验升级为棋子的死活与归属判断，手冲咖啡课把「看茶叶沉浮」的观察经验升级为萃取状态的判读）。成人已会的纯生活动作没有教学价值，不合格；「该领域教材的第一章」默认读者已带领域底子，越过学科底层直奔第一章同样不合格。
- 难度标尺（difficulty 1-5，如实申报）：1=生活经验直接上手；2=单概念入门（认读、模仿、单步操作）；3=单概念应用（多步计算、变式练习）；4=多概念综合/形式化引入（记号体系、复合结构）；5=严格证明与抽象推理。形式化/证明类内容不得压档。
- 一次会话能消化的新东西有限：新授概念是会话的稀缺资源，一个节点通常只引入 1 个核心概念、至多 2 个，标题相应只写一个动作；会话时长用练习与变式撑，不用多讲几个概念撑——把一章的新概念塞进一个节点不是自含，是过载，正路是拆成台阶。
- 同一主题可螺旋重现（不同深度、不同动作词），标题不得完全相同。
- 从易到难：新节点优先长在当前就绪前沿的临近难度上，不突兀拔高。
【概念纪律】
- 名字出处：teaches/assumes 引用的每个概念名，要么照抄登记表/已教账中的既有名字，要么随批 mints 铸名；三处皆无 = 整批被打回。与登记表现有概念同义时改用既有名字，不重复铸名。
- 铸名≠供给：mints 只是把名字登记进概念表。本批节点可以 teach 新铸概念（这是铸名的唯一用途），但任何节点都不得 assume 只有本批（兄弟 teaches 或本批铸名）才供给的概念——同批无先后、必须同时就绪，那种依赖整批打回。
- 缺前提的正路是分层长：节点内容用到的概念既往批次没教过，本批就先长教它的铺垫节点（铸名写进该节点的 teaches），把依赖它的节点留到下一批——而不是削掉前提、空泛化内容，或把依赖链塞进同一批。
【第一批（花名册为空时）】
- 已教账为空：第一批全是入口节点——每条 assumes 只能留空或假定课程外常识，逐节点按【起点纪律】校验；需要前提的概念留到下一批，本批先长教它的铺垫节点。一步一层，宁可少长一层。
【行为信号】
- 行为摘要给出近窗正确率趋势、卡点节点与真实保留率：生长方向优先回应卡点——在卡点节点附近补铺垫或对比节点，而不是无视它继续铺新域。
- 但不要为单个卡点堆同质节点：一次至多一个针对性的回应节点。
【数量契约】
- 本次生长：把就绪节点补到 7 个。当前就绪 X 个、缺口 7-X 个，则本批新增节点 ≤ 7-X 个且至多 7 个；缺口为 0 时输出空 nodes。
- 缺口就是本批的铺开面，通常铺得满：入口层天然是宽的——目标之下往往有比预期更多的底层方向，每个方向的最底层台阶各摆一个入口节点，直到铺满 7 个；真穷尽了互不依赖的独立方向才允许少长，并在 note 里逐一点名已铺的方向（举证不是偷懒）。例：目标「独立做出一款小游戏」，入口层可以同时铺编程语言、平面绘图、关卡设计、操作手感、叙事节奏、音效、发布渠道——底层方向常常比预想还多。
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
    /// 块 5：花名册行：「状态 | 标题 | 分钟 | teaches(概念@档) | assumes(概念@档)」；
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
        // 两条纪律 + 空账本机械地板）。
        if self.roster.is_empty() {
            text.push_str(
                "\n【首批纪律（花名册为空）】\n\
                 - 入口从结构参考的入门档起步，逐格往下问「基线学习者今天拿起什么就能开始」；「简单、基础、入门、教材第一章」都不等于零基础可起步。\n\
                 - 动作句 + 单一行为单元；用到的东西要么本节点教它、要么是常识——简单≠常识。\n\
                 - 已教账为空 ⇒ 本批 assumes 必须全空（机械地板：空账不供给任何概念）。\n",
            );
        }
        // 块 4：当前就绪。
        text.push_str(&format!(
            "\n【当前就绪】{ready} 个（目标 7 个，缺口 {gap} 个）\n",
            ready = self.ready_count
        ));
        // 块 5：花名册。
        text.push_str(&format!("\n【花名册】\n{}\n", if self.roster.is_empty() { "（课程还是空的——本批是第一批：全是入口节点，各自自带引入所教概念（teaches + 铸名），assumes 必须全空）" } else { &self.roster }));
        // 块 6：已教概念账。
        text.push_str(&format!(
            "\n【已教概念账】\n{}\n",
            if self.taught_summary.is_empty() { "（空——本批节点不得依赖本批产出的概念：assumes 只能留空或假定课程外常识，需要前提的概念留到下一批）" } else { &self.taught_summary }
        ));
        // 块 7：覆盖读数（纯展示）。
        if !self.coverage_gauge.is_empty() {
            text.push_str(&format!("\n【覆盖读数（纯展示，不构成本批依据）】\n{}\n", self.coverage_gauge));
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
    let value = crate::generation::parse_json_object::<serde_json::Value>(raw).map_err(|error| {
        tracing::warn!(
            error = %error,
            raw_len = raw.len(),
            raw_head = %raw.chars().take(800).collect::<String>(),
            "unparseable coach reply"
        );
        AppError::UnprocessableEntity(format!("- [ng.parse] 教练回包不是单个 JSON 对象：{error}"))
    })?;
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
        COACH_SYSTEM,
        &user,
        crate::generation::COACH_MAX_TOKENS,
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
        "{}\n\n【上一稿（整批被打回、未落库：其 mints 也未进登记表，本稿要引用就得随批重新铸名）】\n{previous_json}\n\n【打回问题，逐条修复】\n{problems}\n\n【修订纪律】\n- 点修不是重写：未被点名的节点、概念引用与 minutes 原样保留，只改被点名的地方。\n- 打回问题让你补充的每个新概念：进本批某节点的 teaches 就随批 mints 铸名；要被 assume 的只有已教账里已有的才合法——既往批次没教过的，把依赖它的节点移出本批（留到下一批），本批改长教它的铺垫节点。\n- 这批修不动就缩小它：减少节点数，先教最缺的台阶——缩小比重写更可能过门。\n- 重新输出完整批次 JSON。",
        context.render()
    );
    let raw = crate::generation::complete_json_coach(
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

const CONCEPT_REVIEW_SYSTEM: &str = r#"你是一名概念评审员：在结构门之后对生长批次做最后一次语义兜底审查。结构门（机器，已通过）保证过：名字都能解析、每条 assumes 都被既往批次的已教账覆盖（同批兄弟的 teaches 与本批铸名不算供给）、批量与标题合法。你只查结构门查不了的概念面问题。

【先读材料，再下结论】
- 本批节点同批落地、没有学习先后，因此批内互不依赖：节点只许依赖既往批次已教的概念，学习者可能从本批任意节点起步。凡以「批内先后顺序」为由的问题一律不成立——同批无顺序，但同批也无互相依赖。
- 事实以材料为准：【已教概念账】【花名册】与待批 JSON 里的 mints 是全部事实源——名单里有的就是有，没有的才是没有；不要凭印象断言「某概念没人教过」。

【判据——逐节点核对四条】
1. 挂号充分：节点标题与 purpose 里出现的每个领域名词，要么已教（已教账或本批 teaches）、要么已假定（本节点 assumes）、要么随批铸名、要么属于课程外的通用常识。「简单」不等于常识——课程内的基础概念也要挂号；三处皆无的名词，打回并指名。purpose 里「为后续 X 打基础/铺路」式的方向性提及不算无出处——它描述的是课程路线，不是本节点的内容依赖；本节点内容真正用到 X 才要求挂号。
2. 前提足够：把「已教账 ∪ 节点声明的 assumes」当作学习者此刻已知的全部（本批 teaches 不算——同批互不依赖），逐节点问：凭这些，学得了这条节点的内容吗？内容真实依赖、却既无出处也未声明的对象/方法/记号，就是跳步——打回，并指名缺的是哪一个。
3. 命名与内涵一致：铸名的定义说的就是名字所指的东西；同一概念用了两个不同名字（应复用既有名），打回。
4. 难度诚实与阶段/档位相符：节点申报的 difficulty 与内容实际相符——形式化/证明类内容（记号体系、严格推理）不得压 1-2 档；节点内容与【结构参考材料】里同名块的档位明显背离时点名。内容过载（一个节点塞进多个核心概念）可附拆分建议。

【不算问题，一律不报】
- 分钟数长短、每批节点数量与措辞偏好、教学法优化建议——这些是教练职权（拆分建议写在难度诚实条的 advice 里，不单独报）。
- 「可以更好」「建议增加」类意见。
- 你从材料确认不了的：宁放勿错。

回复严格 JSON：
- 通过：{ "verdict": "pass" }
- 打回：{ "verdict": "reject", "problems": [ { "node": "节点标题", "field": "teaches|assumes|mints|title|difficulty", "issue": "问题是什么，指名具体概念", "advice": "一条可执行的修复提示" } ] }
问题按严重度降序，至多 8 条；每条都必须是你能从材料确认的硬伤，issue 写清缺的是什么概念；一条硬伤都没有就 pass，绝不为了「显得尽责」凑问题。只输出 JSON，不要任何解释。"#;

/// Run the AI concept review over a gated batch. Any failure (no completer,
/// call error, unparseable reply) degrades to PASS — the review never blocks
/// a structurally valid batch when it cannot run.
pub(crate) async fn concept_review(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    batch: &ProposedBatch,
    context: &CoachContext,
) -> Vec<ReviewProblem> {
    let batch_json = serde_json::to_string_pretty(batch).unwrap_or_default();
    let user = format!(
        "{}\n\n【待审批次】\n{batch_json}",
        context.render()
    );
    // JSON 接缝 + thinking ON（g4：全部生成面统一）。
    let raw = match crate::generation::complete_json_coach(
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
    // 输出契约（ng.review.*）：verdict 词表 + problems 形状。评审自身不可用
    // （调用失败或违约）→ 保守放行（语义不变：评审不阻断结构合法的批次）。
    let Ok(value) = crate::generation::parse_json_object::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    if growth_contract::validate_review(&value)
        .into_iter()
        .any(|violation| !violation.warning)
    {
        tracing::warn!(
            raw_head = %raw.chars().take(400).collect::<String>(),
            "concept review reply violated its contract — degrading to PASS"
        );
        return Vec::new();
    }
    #[derive(Deserialize)]
    struct Verdict {
        #[serde(default)]
        verdict: String,
        #[serde(default)]
        problems: Vec<ReviewProblem>,
    }
    match serde_json::from_value::<Verdict>(value) {
        Ok(verdict) if verdict.verdict == "reject" => verdict.problems.into_iter().take(8).collect(),
        _ => Vec::new(),
    }
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
        ProposedNode { title: title.to_owned(), purpose: String::new(), minutes: Some(20), difficulty: None, teaches, assumes }
    }

    fn batch_of(nodes: Vec<ProposedNode>) -> ProposedBatch {
        ProposedBatch { nodes, mints: Vec::new(), note: String::new() }
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
        let batch = ProposedBatch {
            nodes: vec![
                node("用提公因式法化简多项式", vec![reference("因式分解", ConceptTier::Know)], vec![]),
                node("用公式法解因式分解题", vec![reference("公式法", ConceptTier::Know)], vec![]),
            ],
            mints: vec![
                ConceptMint { canonical: "因式分解".into(), aliases: vec!["因式拆解".into()], definition: String::new() },
                ConceptMint { canonical: "公式法".into(), aliases: vec![], definition: String::new() },
            ],
            note: String::new(),
        };
        let errors = validate_batch(&batch, &ConceptCoverage::new(), &HashSet::new(), &HashSet::new());
        assert!(errors.is_empty(), "{errors:?}");

        // 依赖既往批次的概念合法：已教账覆盖到所假定档位即放行。
        let batch = ProposedBatch {
            nodes: vec![node(
                "用公式法解因式分解题",
                vec![reference("公式法", ConceptTier::Know)],
                vec![reference("因式分解", ConceptTier::Apply)],
            )],
            mints: vec![ConceptMint { canonical: "公式法".into(), aliases: vec![], definition: String::new() }],
            note: String::new(),
        };
        let mut coverage = ConceptCoverage::new();
        coverage.insert("因式分解".to_owned(), ConceptTier::Apply);
        let mut registry = HashSet::new();
        registry.insert("因式分解".to_owned());
        let errors = validate_batch(&batch, &coverage, &registry, &HashSet::new());
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn validate_batch_rejects_intra_batch_dependency() {
        // 同批无先后、必须同时就绪：本批兄弟的 teaches 不构成 assumes 的供给
        // ——即使概念已随批铸名、也被兄弟教到该档位，依赖它的节点必须留到下一批。
        let batch = ProposedBatch {
            nodes: vec![
                node("用提公因式法化简多项式", vec![reference("因式分解", ConceptTier::Know)], vec![]),
                node("用公式法解因式分解题", vec![reference("公式法", ConceptTier::Know)], vec![reference("因式分解", ConceptTier::Know)]),
            ],
            mints: vec![
                ConceptMint { canonical: "因式分解".into(), aliases: vec!["因式拆解".into()], definition: String::new() },
                ConceptMint { canonical: "公式法".into(), aliases: vec![], definition: String::new() },
            ],
            note: String::new(),
        };
        let errors = validate_batch(&batch, &ConceptCoverage::new(), &HashSet::new(), &HashSet::new());
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].kind, "ng.assumes.uncovered");
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
        let errors = validate_batch(&batch, &coverage, &registry, &HashSet::new());
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, "ng.assumes.uncovered");
    }

    #[test]
    fn validate_batch_enforces_course_internal_title_uniqueness() {
        // 批量上限属形状契约（growth_contract）；这里只测需要 DB 事实的
        // 课程内标题唯一。
        let mut existing = HashSet::new();
        existing.insert("已有节点".to_owned());
        let errors = validate_batch(
            &batch_of(vec![node("已有节点", vec![], vec![])]),
            &ConceptCoverage::new(),
            &HashSet::new(),
            &existing,
        );
        assert!(errors.iter().any(|error| error.kind == "ng.title.duplicate"));

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
        let batch = ProposedBatch {
            nodes: vec![node("新节点", vec![reference("既有概念", ConceptTier::Know)], vec![reference("幽灵概念", ConceptTier::Know)])],
            mints: vec![ConceptMint { canonical: "既有概念".into(), aliases: vec![], definition: String::new() }],
            note: String::new(),
        };
        let errors = validate_batch(&batch, &ConceptCoverage::new(), &registry, &HashSet::new());
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

    #[test]
    /// g4：容错 deserializer 已退役——字符串 minutes 与中文档位不再是
    /// 「容忍解析」，而是带条款报告的违约（报告进重试装配）。
    #[test]
    fn parse_coach_reply_rejects_legacy_tolerant_shapes_with_clause_report() {
        let raw = r#"{
            "nodes": [
                { "title": "用配方法解方程", "purpose": "p", "minutes": "15",
                  "teaches": [{"name": "配方法", "tier": "会用"}],
                  "assumes": [{"name": "一元二次方程", "tier": "知道"}] }
            ],
            "mints": [],
            "note": "n"
        }"#;
        let error = parse_coach_reply(raw).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("ng.nodes.minutes"), "{message}");
        assert!(message.contains("ng.refs.tier"), "{message}");
    }

    /// 严格路径的正向：契约合规的回包按结构装填并做 trim 归一。
    #[test]
    fn parse_coach_reply_accepts_contract_compliant_reply() {
        let raw = r#"{
            "nodes": [
                { "title": " 用配方法解方程 ", "purpose": " 从配方经验切入 ",
                  "minutes": 15, "difficulty": 3,
                  "teaches": [{"name": " 配方法 ", "tier": "apply"}],
                  "assumes": [{"name": "一元二次方程", "tier": "know"}] }
            ],
            "mints": [ { "canonical": " 配方法 ", "aliases": [" 配方 "], "definition": "" } ],
            "note": " n "
        }"#;
        let batch = parse_coach_reply(raw).unwrap();
        assert_eq!(batch.nodes.len(), 1);
        assert_eq!(batch.nodes[0].title, "用配方法解方程");
        assert_eq!(batch.nodes[0].minutes, Some(15));
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
        assert!(cleared.contains("以归档收尾"), "{cleared}");
    }

    #[test]
    fn coach_context_renders_the_gap_contract() {
        let context = CoachContext { ready_count: 2, ..Default::default() };
        let rendered = context.render();
        assert!(rendered.contains("缺口 5 个"), "{rendered}");
        assert!(rendered.contains("把就绪节点补到 7 个") == false, "the number contract lives in the system prompt");
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
