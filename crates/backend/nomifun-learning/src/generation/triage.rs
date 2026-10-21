//! 全量清点 + 分诊（t1）：传统知识库建课的第一层结构发现。清点侧只经
//! [`KnowledgeService`] 的文件 IO（`list_files`/`read_file`，v1 仅 Markdown
//! 文档面，正文只读一次），分诊侧先做零 LLM 的规则预分类（元数据/空文件/
//! 纯导航/翻译副本/同 hash 重复），剩余文件再按 ≤40/批交给
//! [`LearningCompleter`] 判定角色。不确定的保守保留（`uncertain` 进候选集），
//! 每个排除项都带理由；无 completer 或调用失败时规则降级（`degraded=true`），
//! 绝不静默丢内容。
//!
//! 本票不改 `generate_course` 主流程接线（t2/t3 的接缝）；交付的是可独立调用
//! 的编排入口 [`run_kb_triage`]。

use super::sample::sanitize_namespace;
use super::structured_batch::{structured_batch, BatchAttempt, StructuredBatchError, StructuredBatchFailure};
use super::*;
use futures::stream::{self, StreamExt};
use std::future::Future;
use std::pin::Pin;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

/// 正文预览长度上限（清点读一次正文后只留预览给分诊与 t2 复用）。
const TRIAGE_PREVIEW_MAX_CHARS: usize = 512;
/// H1-H3 标题条数/单条长度上限（提示词输入面有界）。
const TRIAGE_HEADINGS_MAX: usize = 32;
const TRIAGE_HEADING_MAX_CHARS: usize = 120;

/// v1 只扫描 Markdown 文档的披露语（写进报告 warnings，不静默忽略）。
const TRIAGE_MD_ONLY_WARNING: &str =
    "v1 仅扫描 Markdown 文档；PDF/Word/图片暂未纳入清点（后续单独立票）";

/// 降级 core 的置信度（规则兜底不是模型判定，低置信 + 理由披露）。
const TRIAGE_DEGRADED_CONFIDENCE: f32 = 0.25;

/// LLM 分诊批大小上限（票面：剩余文件 ≤40/批）。
pub(crate) const TRIAGE_BATCH_MAX_FILES: usize = 40;
/// 分诊 LLM 调用并发上限（票面建议 ≤4）。
pub(crate) const TRIAGE_BATCH_CONCURRENCY: usize = 4;

/// 清点扫描范围（v1 只有 Markdown 文档面；PDF/Word/图片后续单独立票）。
/// 如实披露、不静默忽略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KbScanScope {
    #[default]
    Markdown,
}

/// 清点条目的文件类型（v1 只有 Markdown；字段形状为 t4 落库保持稳定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KbFileKind {
    #[default]
    Markdown,
}

/// 逐来源库的清点披露（命名空间口径沿用采样侧 `{库名}/rel`，t4）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KbInventoryBase {
    pub kb_id: String,
    pub name: String,
    /// 多库时的引用命名空间；单库为空字符串（路径本就是库内裸路径）。
    pub namespace: String,
    pub total_files: usize,
}

/// 清点出的单个文件（多库时 `rel_path` 是 `{命名空间}/库内路径` 的引用路径；
/// 单库是库内相对路径——与采样/`SourceSpan` 口径一致）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KbInventoryFile {
    pub rel_path: String,
    pub kb_id: String,
    pub size: u64,
    pub modified_at: Option<i64>,
    pub content_hash: String,
    /// H1-H3 标题（跳过代码围栏，条数/长度有上限，供分诊与 t2 复用）。
    #[serde(default)]
    pub headings: Vec<String>,
    /// 正文开头预览（清点只读一次正文，后续结构发现复用它，不重读全库）。
    #[serde(default)]
    pub preview: String,
    pub kind: KbFileKind,
}

/// 知识库全量清点结果。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KbInventory {
    #[serde(default)]
    pub bases: Vec<KbInventoryBase>,
    #[serde(default)]
    pub files: Vec<KbInventoryFile>,
    #[serde(default)]
    pub total_files: usize,
    #[serde(default)]
    pub total_bytes: u64,
    /// `KnowledgeService::list_files` 已在服务端过滤 machinery 目录（点目录/
    /// node_modules/_trash），学习侧无法得知被过滤的数量——字段保留给后续
    /// 直扫实现，v1 恒为 0。
    #[serde(default)]
    pub skipped_hidden: usize,
    #[serde(default)]
    pub scan_scope: KbScanScope,
}

/// 分诊角色（文件级只定角色；章节/主题级覆盖归 t2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KbTriageRole {
    Core,
    Prerequisite,
    Practice,
    Mixed,
    Noise,
    Duplicate,
    Uncertain,
}

impl KbTriageRole {
    /// 该角色是否进入 t2 结构发现的候选集（保守保留口径：噪声与重复之外
    /// 一律保留，`mixed` 不整文件排除，`uncertain` 不静默丢）。
    pub fn is_candidate(self) -> bool {
        !matches!(self, KbTriageRole::Noise | KbTriageRole::Duplicate)
    }
}

/// 角色计数（各角色文件数之和恒等于报告文件总数——总数守恒）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KbTriageCounts {
    #[serde(default)]
    pub core: usize,
    #[serde(default)]
    pub prerequisite: usize,
    #[serde(default)]
    pub practice: usize,
    #[serde(default)]
    pub mixed: usize,
    #[serde(default)]
    pub noise: usize,
    #[serde(default)]
    pub duplicate: usize,
    #[serde(default)]
    pub uncertain: usize,
}

/// 单文件分诊判定（逐项带理由；`duplicate_of` 指向 canonical 的引用路径）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KbTriageFile {
    pub rel_path: String,
    pub kb_id: String,
    pub role: KbTriageRole,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<String>,
}

/// 一个被排除出候选集的文件（`noise`/`duplicate`），必须能回答
/// 「为什么没进课程」。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KbTriageExclusion {
    pub path: String,
    pub role: KbTriageRole,
    pub reason: String,
}

/// 一条重复/翻译副本记录：指向 canonical + canonical 的入选理由。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KbTriageDuplicate {
    pub path: String,
    pub duplicate_of: String,
    pub canonical_reason: String,
}

/// 分诊报告。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KbTriageReport {
    #[serde(default)]
    pub files: Vec<KbTriageFile>,
    #[serde(default)]
    pub counts: KbTriageCounts,
    #[serde(default)]
    pub excluded: Vec<KbTriageExclusion>,
    #[serde(default)]
    pub duplicates: Vec<KbTriageDuplicate>,
    #[serde(default)]
    pub degraded: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl KbTriageReport {
    /// 进入 t2 结构发现候选集的文件（core/prerequisite/practice/mixed/
    /// uncertain——噪声与重复之外一律保留）。
    pub fn candidates(&self) -> Vec<&KbTriageFile> {
        self.files.iter().filter(|file| file.role.is_candidate()).collect()
    }
}

/// 全量清点 + 分诊编排入口（t1 交付；t2/t3 在建课管线接线）。
///
/// - 逐库 `list_files` 清点 + 每文件正文只读一次（hash/H1-H3/preview）；
/// - 规则预分类零 LLM：元数据/空文件/纯导航 → noise，翻译副本与同 hash
///   重复按 canonical 规则（Q12）标 duplicate 并给理由，判不了 → uncertain；
/// - 剩余文件按 ≤40/批、并发 ≤4 做 LLM 分诊；单文件解析失败 → uncertain；
/// - 无 completer 或调用失败 → 规则降级（非噪声全部 core），`degraded=true`。
/// 单个清点文件在分诊管线里的内部形态：`KbInventoryFile` 之外携带只在
/// 规则预分类阶段需要的正文信号（正文本身不保留——清点只读一次）。
struct ScannedFile {
    file: KbInventoryFile,
    /// 库内相对路径（`read_file` 的 IO 键；多库时与 `rel_path` 不同）。
    base_rel: String,
    /// 库内去语言键（翻译布局分组的组键：后缀布局 = 目录+去后缀词干，
    /// 目录布局 = 去语言段后的路径）。
    langless_key: String,
    lang_tag: Option<String>,
    cjk_ratio: f32,
    is_empty: bool,
    pure_navigation: bool,
}

/// 规则预分类的判定（零 LLM、高置信）。
#[derive(Clone)]
struct RuleVerdict {
    role: KbTriageRole,
    confidence: f32,
    reason: String,
    duplicate_of: Option<String>,
    canonical_reason: Option<String>,
}

pub async fn run_kb_triage(
    knowledge: &KnowledgeService,
    kb_ids: &[KnowledgeBaseId],
    goal: &str,
    completer: Option<&dyn LearningCompleter>,
) -> Result<(KbInventory, KbTriageReport), AppError> {
    // 去重保序（对齐 `knowledge_base_ids_normalized` 的口径）。
    let mut ids: Vec<KnowledgeBaseId> = Vec::with_capacity(kb_ids.len());
    for id in kb_ids {
        if !ids.iter().any(|seen| seen == id) {
            ids.push(id.clone());
        }
    }
    if ids.is_empty() {
        return Err(AppError::BadRequest(
            "no knowledge bases selected for triage".into(),
        ));
    }

    let mut warnings: Vec<String> = vec![TRIAGE_MD_ONLY_WARNING.to_owned()];

    // 逐库核验存在（已删库 id 不读）：缺库跳过 + 警告，不让一个坏库阻断
    // 其余库的清点。
    let mut live_bases: Vec<(String, String)> = Vec::with_capacity(ids.len());
    for id in &ids {
        match knowledge.get_base_info(id.as_str()).await {
            Ok(info) => live_bases.push((id.as_str().to_owned(), info.name)),
            Err(error) => warnings.push(format!(
                "知识库 {} 不存在或不可读，已跳过：{error}",
                id.as_str()
            )),
        }
    }
    let namespaces = assign_namespaces(&live_bases);
    let namespaced = live_bases.len() >= 2;

    // 清点：逐库 list_files + 每文件正文只读一次（hash/H1-H3/preview 一次算齐）。
    let mut scanned: Vec<ScannedFile> = Vec::new();
    let mut inventory_bases: Vec<KbInventoryBase> = Vec::with_capacity(live_bases.len());
    for (position, (kb_id, _name)) in live_bases.iter().enumerate() {
        let namespace = &namespaces[position];
        let entries = knowledge.list_files(kb_id).await?;
        let mut base_files = 0usize;
        for entry in knowledge.list_files(kb_id).await? {
            let citation_path = if namespaced {
                format!("{namespace}/{}", entry.rel_path)
            } else {
                entry.rel_path.clone()
            };
            let content = match knowledge.read_file(kb_id, &entry.rel_path).await {
                Ok(content) => content.content,
                Err(error) => {
                    warnings.push(format!("文件 {citation_path} 读取失败，未入清点：{error}"));
                    continue;
                }
            };
            base_files += 1;
            let (langless_key, lang_tag) = langless_key_of(&entry.rel_path);
            let analysis = analyze_content(&content);
            scanned.push(ScannedFile {
                file: KbInventoryFile {
                    rel_path: citation_path,
                    kb_id: kb_id.clone(),
                    size: entry.size,
                    modified_at: entry.modified_at,
                    content_hash: content_hash_hex(&content),
                    headings: analysis.headings,
                    preview: analysis.preview,
                    kind: KbFileKind::Markdown,
                },
                base_rel: entry.rel_path,
                langless_key,
                lang_tag,
                cjk_ratio: analysis.cjk_ratio,
                is_empty: analysis.is_empty,
                pure_navigation: analysis.pure_navigation,
            });
        }
        inventory_bases.push(KbInventoryBase {
            kb_id: kb_id.clone(),
            name: _name.clone(),
            namespace: namespace.clone(),
            total_files: base_files,
        });
    }
    // 确定性输出：按引用路径字典序（多库按命名空间分组，与采样合并序一致）。
    scanned.sort_by(|left, right| left.file.rel_path.cmp(&right.file.rel_path));

    let total_files = scanned.len();
    if total_files == 0 {
        let skipped: Vec<&str> = warnings
            .iter()
            .filter(|warning| warning.starts_with("知识库"))
            .map(String::as_str)
            .collect();
        return Err(AppError::BadRequest(format!(
            "selected knowledge bases have no readable markdown documents to triage{}",
            if skipped.is_empty() {
                String::new()
            } else {
                format!("（{}）", skipped.join("；"))
            }
        )));
    }
    let total_bytes = scanned.iter().map(|file| file.file.size).sum();
    let inventory = KbInventory {
        bases: inventory_bases,
        files: scanned.iter().map(|file| file.file.clone()).collect(),
        total_files,
        total_bytes,
        skipped_hidden: 0,
        scan_scope: KbScanScope::Markdown,
    };

    // 规则预分类（零 LLM，t1 逐步填充：元数据/空/导航/翻译/同 hash）。
    let verdicts = rule_verdicts(&scanned, goal);
    let remaining: Vec<&ScannedFile> = scanned
        .iter()
        .filter(|file| !verdicts.contains_key(&file.file.rel_path))
        .collect();

    // LLM 分诊；无 completer 或调用失败 → 规则降级（非噪声全部 core）。
    let mut degraded_cause: Option<String> = None;
    let mut llm_verdicts: HashMap<String, RuleVerdict> = HashMap::new();
    match completer {
        None => {
            degraded_cause = Some("未配置模型（completer）".to_owned());
        }
        Some(completer) => {
            if !remaining.is_empty() {
                let lookup: HashMap<&str, &ScannedFile> = scanned
                    .iter()
                    .map(|file| (file.file.rel_path.as_str(), file))
                    .collect();
                let attempt = TriageBatchAttempt {
                    completer,
                    goal,
                    inventory: &inventory,
                    lookup: &lookup,
                };
                let batch_ids: Vec<String> = remaining
                    .iter()
                    .map(|file| file.file.rel_path.clone())
                    .collect();
                // 批次划分（≤40/批）+ 批间并发（≤4）；批内拆半由
                // structured_batch 递归处理（顺序，失败罕见且对数深度）。
                let batches: Vec<Vec<String>> = batch_ids
                    .chunks(TRIAGE_BATCH_MAX_FILES)
                    .map(<[String]>::to_vec)
                    .collect();
                let attempt_ref = &attempt;
                let outputs = stream::iter(batches)
                    .map(|batch| async move { structured_batch(&batch, attempt_ref).await })
                    .buffer_unordered(TRIAGE_BATCH_CONCURRENCY)
                    .collect::<Vec<_>>()
                    .await;
                let mut call_failed: Vec<String> = Vec::new();
                for batch_output in outputs {
                    for (path, outcome) in batch_output {
                        match outcome {
                            Ok(verdict) => {
                                llm_verdicts.insert(path, verdict);
                            }
                            Err(StructuredBatchFailure::Parse(message)) => {
                                // 单文件兜底仍失败 → uncertain + 理由，不丢。
                                llm_verdicts.insert(
                                    path,
                                    RuleVerdict {
                                        role: KbTriageRole::Uncertain,
                                        confidence: 0.2,
                                        reason: format!(
                                            "单文件分诊仍解析失败，保守按 uncertain 保留：{message}"
                                        ),
                                        duplicate_of: None,
                                        canonical_reason: None,
                                    },
                                );
                            }
                            Err(StructuredBatchFailure::Call(_)) => call_failed.push(path),
                        }
                    }
                }
                if !call_failed.is_empty() {
                    degraded_cause =
                        Some(format!("{} 个文件的分诊调用失败", call_failed.len()));
                }
            }
        }
    }
    let degraded = degraded_cause.is_some();
    if let Some(cause) = &degraded_cause {
        warnings.push(format!("{cause}，分诊按规则降级：非噪声文件保守进入 core"));
    }

    // 汇总报告：文件序 = 清点序（守恒：counts 之和 == files.len() ==
    // inventory.total_files）。
    let mut files: Vec<KbTriageFile> = Vec::with_capacity(total_files);
    let mut counts = KbTriageCounts::default();
    let mut excluded: Vec<KbTriageExclusion> = Vec::new();
    let mut duplicates: Vec<KbTriageDuplicate> = Vec::new();
    for file in &scanned {
        let verdict = llm_verdicts
            .get(&file.file.rel_path)
            .or_else(|| verdicts.get(&file.file.rel_path))
            .cloned()
            .unwrap_or_else(|| match &degraded_cause {
                Some(cause) => degraded_core_verdict(cause),
                None => RuleVerdict {
                    role: KbTriageRole::Uncertain,
                    confidence: TRIAGE_DEGRADED_CONFIDENCE,
                    reason: "分诊未覆盖该文件，保守标记为 uncertain".into(),
                    duplicate_of: None,
                    canonical_reason: None,
                },
            });
        if let Some(duplicate_of) = &verdict.duplicate_of {
            duplicates.push(KbTriageDuplicate {
                path: file.file.rel_path.clone(),
                duplicate_of: duplicate_of.clone(),
                canonical_reason: verdict
                    .canonical_reason
                    .clone()
                    .unwrap_or_else(|| "canonical 判定为规则默认".into()),
            });
        }
        if !verdict.role.is_candidate() {
            excluded.push(KbTriageExclusion {
                path: file.file.rel_path.clone(),
                role: verdict.role,
                reason: verdict.reason.clone(),
            });
        }
        counts.add(verdict.role);
        files.push(KbTriageFile {
            rel_path: file.file.rel_path.clone(),
            kb_id: file.file.kb_id.clone(),
            role: verdict.role,
            confidence: verdict.confidence,
            reason: verdict.reason,
            duplicate_of: verdict.duplicate_of,
        });
    }
    let report = KbTriageReport {
        files,
        counts,
        excluded,
        duplicates,
        degraded,
        warnings,
    };
    Ok((inventory, report))
}

impl KbTriageCounts {
    fn add(&mut self, role: KbTriageRole) {
        match role {
            KbTriageRole::Core => self.core += 1,
            KbTriageRole::Prerequisite => self.prerequisite += 1,
            KbTriageRole::Practice => self.practice += 1,
            KbTriageRole::Mixed => self.mixed += 1,
            KbTriageRole::Noise => self.noise += 1,
            KbTriageRole::Duplicate => self.duplicate += 1,
            KbTriageRole::Uncertain => self.uncertain += 1,
        }
    }
}

/// 多库命名空间分配（沿用采样侧口径，t4）：按（库名 trim, kb_id）字典序定
/// 库序，库名清洗（`sanitize_namespace`）后同名库从第二个起加 `~2`、`~3`。
fn assign_namespaces(bases: &[(String, String)]) -> Vec<String> {
    let mut order: Vec<usize> = (0..bases.len()).collect();
    order.sort_by(|left, right| {
        bases[*left]
            .1
            .trim()
            .cmp(bases[*right].1.trim())
            .then_with(|| bases[*left].0.cmp(&bases[*right].0))
    });
    let mut namespaces: Vec<String> = vec![String::new(); bases.len()];
    let mut used: HashSet<String> = HashSet::new();
    for index in order {
        let (kb_id, name) = &bases[index];
        let base_ns = sanitize_namespace(name, kb_id);
        let mut candidate = base_ns.clone();
        let mut suffix = 2usize;
        while !used.insert(candidate.clone()) {
            candidate = format!("{base_ns}~{suffix}");
            suffix += 1;
        }
        namespaces[index] = candidate;
    }
    namespaces
}

/// 正文一次读出的全部分析信号（语言/布局键在扫描处单独计算）。
struct ContentAnalysis {
    headings: Vec<String>,
    preview: String,
    cjk_ratio: f32,
    is_empty: bool,
    pure_navigation: bool,
}

fn analyze_content(content: &str) -> ContentAnalysis {
    let trimmed = content.trim();
    ContentAnalysis {
        headings: extract_headings(content),
        preview: preview_of(trimmed),
        cjk_ratio: cjk_ratio(trimmed),
        is_empty: trimmed.is_empty(),
        pure_navigation: is_pure_navigation(content),
    }
}

fn content_hash_hex(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// H1-H3 标题（跳过代码围栏内的 `#` 行；条数与单条长度有上限）。
fn extract_headings(content: &str) -> Vec<String> {
    let mut headings: Vec<String> = Vec::new();
    let mut in_fence = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let title = if let Some(rest) = trimmed.strip_prefix("### ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("# ") {
            rest
        } else {
            continue;
        };
        let title: String = title.trim().chars().take(TRIAGE_HEADING_MAX_CHARS).collect();
        headings.push(title);
        if headings.len() >= TRIAGE_HEADINGS_MAX {
            break;
        }
    }
    headings
}

fn preview_of(trimmed: &str) -> String {
    trimmed.chars().take(TRIAGE_PREVIEW_MAX_CHARS).collect()
}

/// CJK 字符占比（粗粒度语言信号：≥ 阈值视为中文文档）。
fn cjk_ratio(text: &str) -> f32 {
    let mut total = 0usize;
    let mut cjk = 0usize;
    for ch in text.chars() {
        if ch.is_whitespace() {
            continue;
        }
        total += 1;
        if matches!(ch as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF) {
            cjk += 1;
        }
    }
    if total == 0 {
        0.0
    } else {
        cjk as f32 / total as f32
    }
}

/// 纯导航页：所有非空行都是标题或链接列表项（无正文散文），且至少有一个
/// 链接——目录页/索引页成噪声，不进课程。
fn is_pure_navigation(content: &str) -> bool {
    let mut has_link = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let hashes = trimmed.len() - trimmed.trim_start_matches('#').len();
        if hashes >= 1 {
            let rest = &trimmed[hashes..];
            if rest.is_empty() || rest.starts_with(' ') {
                continue;
            }
        }
        let payload = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
            .map(str::trim)
            .unwrap_or(trimmed);
        if is_link_payload(payload) {
            has_link = true;
            continue;
        }
        return false;
    }
    has_link
}

fn is_link_payload(payload: &str) -> bool {
    if payload.is_empty() {
        return true;
    }
    if payload.starts_with("http://") || payload.starts_with("https://") {
        return true;
    }
    // [label](target) 整行链接（标签可含嵌套空白）。
    if payload.starts_with('[') {
        if let Some((_, rest)) = split_md_link(payload) {
            return rest.trim().is_empty();
        }
    }
    false
}

/// 拆开行首 `[label](target)`，返回 (label, target 之后的余量)。
fn split_md_link(payload: &str) -> Option<(String, &str)> {
    let mut depth = 0usize;
    let mut label_end = None;
    for (index, ch) in payload.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    label_end = Some(index);
                    break;
                }
            }
            _ => {}
        }
    }
    let label_end = label_end?;
    let rest = payload[label_end + ']'.len_utf8()..].strip_prefix('(')?;
    let close = rest.find(')')?;
    Some((payload[1..label_end].to_owned(), &rest[close + ')'.len_utf8()..]))
}

/// 翻译布局的语言标签集合（保守：常见语言码；命中才参与布局检测）。
const LANG_TAGS: &[&str] = &[
    "en", "zh", "zh-cn", "zh-tw", "zh-hans", "zh-hant", "ja", "ko", "fr", "de", "es", "ru",
    "pt", "pt-br", "it", "ar", "hi", "th", "vi", "id", "tr", "nl", "pl", "uk",
];

fn is_lang_tag(token: &str) -> bool {
    let lowered = token.to_ascii_lowercase();
    LANG_TAGS.contains(&lowered.as_str())
}

/// 去语言键 + 语言标签（String 形态）：后缀布局（`guide.zh-CN.md` →
/// (目录/guide.md, zh)）与目录布局（`en/guide.md` → 去 `en` 段后的路径）；
/// 无标签的文件键即自身路径、标签为 None。
fn langless_key_of(base_rel: &str) -> (String, Option<String>) {
    // 目录布局：路径中的语言段（最后命中的一段）。
    let mut segments: Vec<&str> = base_rel.split('/').collect();
    if let Some(position) = segments[..segments.len() - 1]
        .iter()
        .rposition(|segment| is_lang_tag(segment))
    {
        let lang = segments.remove(position);
        return (segments.join("/"), Some(lang.to_ascii_lowercase()));
    }
    // 后缀布局：词干尾部的语言后缀（`.`/`_`/`-` 分隔，取最右侧的合法段）。
    let file_name = segments.last().copied().unwrap_or_default();
    let dir = &base_rel[..base_rel.len() - file_name.len()];
    let stem = match file_name.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("md") => stem,
        _ => file_name,
    };
    for (index, _) in stem.char_indices().rev() {
        if index == 0 {
            break;
        }
        let separator = stem.as_bytes()[index - 1];
        if separator != b'.' && separator != b'_' && separator != b'-' {
            continue;
        }
        let (base, tag) = stem.split_at(index - 1);
        let token = &tag[1..];
        if !base.is_empty() && is_lang_tag(token) {
            return (
                format!("{dir}{base}.md"),
                Some(token.to_ascii_lowercase()),
            );
        }
    }
    (base_rel.to_owned(), None)
}

/// 元数据文件词干名单（票面）：许可/贡献指南/行为准则/模板等库机制文件，
/// 与课程内容无关，高置信 noise。
const METADATA_FILE_STEMS: &[&str] = &[
    "license",
    "licence",
    "contributing",
    "code_of_conduct",
    "security",
    "changelog",
    "authors",
    "maintainers",
    "pull_request_template",
    "issue_template",
    "support",
    "faq",
    "citation",
    "codeowners",
];

/// CJK 占比达到该阈值视为中文文档（粗粒度语言信号，Q12 canonical 规则用）。
const CJK_LANG_THRESHOLD: f32 = 0.2;

fn detected_lang(cjk_ratio: f32) -> &'static str {
    if cjk_ratio >= CJK_LANG_THRESHOLD {
        "zh"
    } else {
        "en"
    }
}

/// 规则预分类（零 LLM、高置信）：元数据/空文件/纯导航 → noise；翻译布局
/// （后缀 + 目录）成对记 duplicate_of，canonical 按 Q12（非翻译后缀原文
/// 优先 → README → 目标语言 → 最新 → 判不了双 uncertain 不排除）；同
/// content_hash → duplicate（canonical 同规则口径）。
fn rule_verdicts(scanned: &[ScannedFile], goal: &str) -> HashMap<String, RuleVerdict> {
    let mut verdicts: HashMap<String, RuleVerdict> = HashMap::new();

    // 1) 元数据 / 空文件 / 纯导航 → noise。
    for file in scanned {
        let verdict = if let Some(stem) = metadata_stem_of(&file.langless_key) {
            RuleVerdict {
                role: KbTriageRole::Noise,
                confidence: 0.95,
                reason: format!("元数据文件（{stem}）：库机制文件，与课程内容无关"),
                duplicate_of: None,
                canonical_reason: None,
            }
        } else if file.is_empty {
            RuleVerdict {
                role: KbTriageRole::Noise,
                confidence: 0.9,
                reason: "空文件（无正文内容）".to_owned(),
                duplicate_of: None,
                canonical_reason: None,
            }
        } else if file.pure_navigation {
            RuleVerdict {
                role: KbTriageRole::Noise,
                confidence: 0.9,
                reason: "纯导航页（只有链接目录，无正文）".to_owned(),
                duplicate_of: None,
                canonical_reason: None,
            }
        } else {
            continue;
        };
        verdicts.insert(file.file.rel_path.clone(), verdict);
    }

    // 2) 翻译布局（后缀 microsoft/parallel/suffix 统一口径：库内去语言键
    //    分组；成员 ≥2 才是家族，孤儿翻译保守当原文）。
    let mut families: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (index, file) in scanned.iter().enumerate() {
        if verdicts.contains_key(&file.file.rel_path) {
            continue;
        }
        families
            .entry((file.file.kb_id.as_str(), file.langless_key.as_str()))
            .or_default()
            .push(index);
    }
    let goal_lang = detected_lang(cjk_ratio(goal.trim()));
    for members in families.values() {
        if members.len() < 2 {
            continue;
        }
        match translation_canonical(scanned, members, goal_lang) {
            Some((canonical_index, canonical_reason)) => {
                let canonical_path = scanned[canonical_index].file.rel_path.clone();
                for &index in members {
                    if index == canonical_index {
                        continue;
                    }
                    let file = &scanned[index];
                    let tag = file.lang_tag.clone().unwrap_or_default();
                    verdicts.insert(
                        file.file.rel_path.clone(),
                        RuleVerdict {
                            role: KbTriageRole::Duplicate,
                            confidence: 0.9,
                            reason: format!(
                                "翻译副本（语言标签 {tag}），canonical 为 {canonical_path}"
                            ),
                            duplicate_of: Some(canonical_path.clone()),
                            canonical_reason: Some(canonical_reason.clone()),
                        },
                    );
                }
            }
            None => {
                for &index in members {
                    let file = &scanned[index];
                    verdicts.insert(
                        file.file.rel_path.clone(),
                        RuleVerdict {
                            role: KbTriageRole::Uncertain,
                            confidence: 0.3,
                            reason: "翻译布局检测到多语言副本，但无法判定翻译方向（规则不定）"
                                .to_owned(),
                            duplicate_of: None,
                            canonical_reason: None,
                        },
                    );
                }
            }
        }
    }

    // 3) 同 content_hash → duplicate（跨库全局口径；canonical：非翻译命名
    //    优先 → 最新 mtime → 路径字典序兜底，全程确定性）。
    let mut hash_groups: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, file) in scanned.iter().enumerate() {
        if verdicts.contains_key(&file.file.rel_path) {
            continue;
        }
        hash_groups
            .entry(file.file.content_hash.as_str())
            .or_default()
            .push(index);
    }
    for members in hash_groups.values() {
        if members.len() < 2 {
            continue;
        }
        let (canonical_index, canonical_reason) = hash_canonical(scanned, members);
        let canonical_path = scanned[canonical_index].file.rel_path.clone();
        for &index in members {
            if index == canonical_index {
                continue;
            }
            let file = &scanned[index];
            verdicts.insert(
                file.file.rel_path.clone(),
                RuleVerdict {
                    role: KbTriageRole::Duplicate,
                    confidence: 0.9,
                    reason: format!(
                        "与 {canonical_path} 内容完全相同（sha256 一致），canonical 按「{canonical_reason}」选定"
                    ),
                    duplicate_of: Some(canonical_path.clone()),
                    canonical_reason: Some(canonical_reason.clone()),
                },
            );
        }
    }
    verdicts
}

/// 元数据词干（先去语言后缀再匹配，`contributing.zh-CN.md` 也是元数据）。
fn metadata_stem_of(langless_key: &str) -> Option<&'static str> {
    let file_name = langless_key.rsplit('/').next().unwrap_or_default();
    let stem = file_name.strip_suffix(".md").unwrap_or(file_name);
    let lowered = stem.to_ascii_lowercase();
    METADATA_FILE_STEMS
        .iter()
        .find(|candidate| **candidate == lowered)
        .copied()
}

/// 翻译家族的 canonical（Q12）：非翻译后缀的原文 → README → 目标语言
/// （字符占比）→ 最新修改；全平手返回 None（双双 uncertain，不自动排除）。
fn translation_canonical(
    scanned: &[ScannedFile],
    members: &[usize],
    goal_lang: &str,
) -> Option<(usize, String)> {
    if let Some(&index) = members
        .iter()
        .find(|&&index| scanned[index].lang_tag.is_none())
    {
        return Some((index, "非翻译后缀的原文优先".to_owned()));
    }
    let readme_members: Vec<usize> = members
        .iter()
        .copied()
        .filter(|&index| readme_stem_of(&scanned[index].base_rel))
        .collect();
    if readme_members.len() == 1 {
        return Some((readme_members[0], "README 文件优先作为 canonical".to_owned()));
    }
    let lang_matches: Vec<usize> = members
        .iter()
        .copied()
        .filter(|&index| detected_lang(scanned[index].cjk_ratio) == goal_lang)
        .collect();
    if lang_matches.len() == 1 {
        return Some((
            lang_matches[0],
            "语言与学习目标一致（字符占比判定）".to_owned(),
        ));
    }
    newest_member(scanned, members).map(|index| (index, "同语言下最新修改时间".to_owned()))
}

/// 同 hash 组的 canonical：唯一非翻译命名成员优先，否则最新 mtime，否则
/// 路径字典序兜底（确定性——永不返回 None）。
fn hash_canonical(scanned: &[ScannedFile], members: &[usize]) -> (usize, String) {
    let untagged: Vec<usize> = members
        .iter()
        .copied()
        .filter(|&index| scanned[index].lang_tag.is_none())
        .collect();
    let pool: &[usize] = if untagged.len() == 1 {
        return (untagged[0], "非翻译命名的原文优先".to_owned());
    } else if untagged.len() > 1 {
        &untagged
    } else {
        members
    };
    if let Some(index) = newest_member(scanned, pool) {
        return (index, "同内容文件中最新修改时间".to_owned());
    }
    let first = pool
        .iter()
        .copied()
        .min_by(|left, right| scanned[*left].file.rel_path.cmp(&scanned[*right].file.rel_path))
        .unwrap_or(pool[0]);
    (first, "路径字典序（确定性兜底）".to_owned())
}

/// 唯一的最大 mtime 成员（None 视为最旧；并列返回 None——无法分新旧）。
fn newest_member(scanned: &[ScannedFile], members: &[usize]) -> Option<usize> {
    let max = members
        .iter()
        .map(|&index| scanned[index].file.modified_at)
        .max()
        .flatten()?;
    let winners: Vec<usize> = members
        .iter()
        .copied()
        .filter(|&index| scanned[index].file.modified_at == Some(max))
        .collect();
    if winners.len() == 1 {
        Some(winners[0])
    } else {
        None
    }
}

/// LLM 分诊的批尝试器：一片条目 → 提示词 → complete_json → 容错解析 →
/// 逐文件判定。调用层失败报 [`StructuredBatchError::Call`]（驱动层不拆半，
/// 交给降级兜底），解析失败报 [`StructuredBatchError::Parse`]（驱动层拆半）。
struct TriageBatchAttempt<'a> {
    completer: &'a dyn LearningCompleter,
    goal: &'a str,
    inventory: &'a KbInventory,
    lookup: &'a HashMap<&'a str, &'a ScannedFile>,
}

impl BatchAttempt<RuleVerdict> for TriageBatchAttempt<'_> {
    fn attempt<'s>(
        &'s self,
        slice: &'s [String],
    ) -> Pin<Box<dyn Future<Output = Result<HashMap<String, RuleVerdict>, StructuredBatchError>> + Send + 's>>
    {
        Box::pin(async move {
            let files: Vec<&ScannedFile> = slice
                .iter()
                .filter_map(|id| self.lookup.get(id.as_str()).copied())
                .collect();
            if files.is_empty() {
                return Err(StructuredBatchError::Parse(
                    "批内没有可分诊的文件（内部错误）".to_owned(),
                ));
            }
            let prompt = build_triage_user_prompt(self.goal, self.inventory, &files);
            let raw = self
                .completer
                .complete_json(
                    None,
                    TRIAGE_SYSTEM,
                    &prompt,
                    triage_output_tokens(slice.len()),
                    Some(0.1),
                )
                .await
                .map_err(|error| StructuredBatchError::Call(error.to_string()))?;
            let reply: TriageLlmReply =
                parse_json_object(&raw).map_err(StructuredBatchError::Parse)?;
            Ok(map_llm_reply(reply, &files))
        })
    }
}

/// 把模型回包映射回请求集（未知路径忽略；未知角色保守 uncertain）。
fn map_llm_reply(
    reply: TriageLlmReply,
    remaining: &[&ScannedFile],
) -> HashMap<String, RuleVerdict> {
    let requested: HashSet<&str> = remaining.iter().map(|file| file.file.rel_path.as_str()).collect();
    let mut verdicts = HashMap::new();
    for entry in reply.files {
        if !requested.contains(entry.path.as_str()) {
            continue;
        }
        let role = match entry.role.as_str() {
            "core" => KbTriageRole::Core,
            "prerequisite" => KbTriageRole::Prerequisite,
            "practice" => KbTriageRole::Practice,
            "mixed" => KbTriageRole::Mixed,
            "noise" => KbTriageRole::Noise,
            "duplicate" => KbTriageRole::Duplicate,
            "uncertain" => KbTriageRole::Uncertain,
            unknown => {
                verdicts.insert(
                    entry.path.clone(),
                    RuleVerdict {
                        role: KbTriageRole::Uncertain,
                        confidence: entry.confidence.clamp(0.0, 1.0),
                        reason: format!("模型返回未知角色「{unknown}」，保守按 uncertain 保留"),
                        duplicate_of: None,
                        canonical_reason: None,
                    },
                );
                continue;
            }
        };
        let reason = if entry.reason.trim().is_empty() {
            "模型未给出理由".to_owned()
        } else {
            entry.reason.trim().to_owned()
        };
        verdicts.insert(
            entry.path.clone(),
            RuleVerdict {
                role,
                confidence: entry.confidence.clamp(0.0, 1.0),
                reason,
                duplicate_of: None,
                canonical_reason: None,
            },
        );
    }
    verdicts
}

/// 降级兜底：非噪声文件全部 core（翻译/重复仍按规则标记——它们根本不进
/// `remaining`）。
fn degraded_core_verdict(cause: &str) -> RuleVerdict {
    RuleVerdict {
        role: KbTriageRole::Core,
        confidence: TRIAGE_DEGRADED_CONFIDENCE,
        reason: format!("{cause}，规则降级：保守按 core 进入候选"),
        duplicate_of: None,
        canonical_reason: None,
    }
}

/// 每批输出 token 上限（每文件判定约 100 token，留余量防截断）。
fn triage_output_tokens(file_count: usize) -> u32 {
    (file_count as u32 * 150 + 512).min(16_384)
}

/// 分诊系统提示词（内联常量风格，中文，输出 JSON schema 明确）。
const TRIAGE_SYSTEM: &str = r#"你是知识库课程化的分诊员。给定学习目标、库的目录树和一批文件清单（标题/预览/类型），为每个文件判定它在课程中的角色。

只输出一个 JSON 对象，形状如下：
{
  "files": [
    {"path": "清单中的原样路径", "role": "core", "confidence": 0.9, "reason": "一句话理由"}
  ]
}
role 只能取以下七值之一：
- core：本课程要教的内容；
- prerequisite：学习目标需要、但不必单独成课的前置背景；
- practice：练习、实验、示例代码；
- mixed：文件既有与目标相关的正文，也有明显无关的段落（会议记录、推广、旧章节等）——不要因为无关段排除整个文件；
- noise：与目标无关的元数据、推广页、空导航、闲谈记录；
- duplicate：重复、翻译副本或旧版本；
- uncertain：判不准。

规则：
- 必须为清单中的每一个 path 给出判定，不要发明清单外的路径。
- confidence 是 0 到 1 的小数；判不准时给低置信并选 uncertain。
- uncertain 是保守选项：宁可 uncertain 也不要把可能是教学内容的内容静默丢掉。
- 每个文件都必须给出一句话 reason。
- 只输出 JSON，不要 Markdown 围栏或解释。"#;

/// 批提示词：目标 + README 摘录 + 目录树 + 每文件（路径/标题/预览/类型）。
fn build_triage_user_prompt(
    goal: &str,
    inventory: &KbInventory,
    files: &[&ScannedFile],
) -> String {
    let mut prompt = String::new();
    let goal = goal.trim();
    if goal.is_empty() {
        prompt.push_str("学习目标：（未提供，按文件内容整体判断）\n\n");
    } else {
        prompt.push_str(&format!("学习目标：{goal}\n\n"));
    }
    if let Some(readme) = inventory
        .files
        .iter()
        .find(|file| readme_stem_of(&file.rel_path))
    {
        prompt.push_str(&format!("README 摘录（{}/…）：{}\n\n", readme.rel_path, readme.preview));
    }
    prompt.push_str("目录树（全部清点文件）：\n");
    for path in inventory.files.iter().map(|file| file.rel_path.as_str()).take(400) {
        prompt.push_str(&format!("- {path}\n"));
    }
    prompt.push_str("\n待分诊文件清单：\n");
    for file in files {
        prompt.push_str(&format!(
            "- 文件 {} :: 类型 {} :: 标题 [{}] :: 预览 {}\n",
            file.file.rel_path,
            match file.file.kind {
                KbFileKind::Markdown => "markdown",
            },
            file.file.headings.join(" / "),
            file.file.preview.replace('\n', " "),
        ));
    }
    prompt
}

fn readme_stem_of(citation_path: &str) -> bool {
    citation_path
        .rsplit('/')
        .next()
        .and_then(|name| name.split('.').next())
        .is_some_and(|stem| stem.eq_ignore_ascii_case("readme"))
}

/// 模型回包的宽容反序列化形状。
#[derive(Deserialize)]
struct TriageLlmReply {
    #[serde(default)]
    files: Vec<TriageLlmFile>,
}

#[derive(Deserialize)]
struct TriageLlmFile {
    #[serde(default)]
    path: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    confidence: f32,
    #[serde(default)]
    reason: String,
}

#[cfg(test)]
mod tests;
