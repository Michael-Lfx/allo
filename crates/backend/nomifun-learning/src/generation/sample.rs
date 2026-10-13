use std::collections::{HashSet, VecDeque};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::terms::{term_hits, term_slices};
use super::*;


/// Sampling budget: at most this many files feed the course-generation
/// prompts. Wider than the knowledge-overview default (more files, larger
/// excerpts, higher total): the multi-stage pipeline has the budget to read
/// them and the lessons need richer grounding. The whole sampled corpus is
/// the INPUT side of the context-window equation — the output budgets live
/// in [`crate::generation::completer`].
pub(crate) const LEARNING_SAMPLE_MAX_FILES: usize = 40;
pub(crate) const LEARNING_SAMPLE_MAX_PER_FILE: usize = 8 * 1024;
pub(crate) const LEARNING_SAMPLE_MAX_TOTAL: usize = 160 * 1024;

/// 目标感知采样（t2）的预算：文件配额与旧路径相同（40），单文件摘录
/// 8KB → 24KB（目标相关段落选摘，相关事实落在大文件中段也能进摘录）；
/// 总量上限 = 配额 × 单文件上限——文件配额才是有效约束，总量只防失控
/// （相关文件优先，绝不因总量把相关文件挤掉）。
pub(crate) const GOAL_SAMPLE_MAX_FILES: usize = LEARNING_SAMPLE_MAX_FILES;
pub(crate) const GOAL_SAMPLE_MAX_PER_FILE: usize = 24 * 1024;
pub(crate) const GOAL_SAMPLE_MAX_TOTAL: usize = GOAL_SAMPLE_MAX_FILES * GOAL_SAMPLE_MAX_PER_FILE;

/// 连贯预算：占采样额度的比例（%）。相关文件优先占额度，连贯补入只用余量，
/// 且不超过该比例——防课程式库断链，又不能把采样变成全量。
const GOAL_SAMPLE_COHERENCE_PERCENT: usize = 30;

/// 多知识库分摊（t4）的每库保底：预算等分后每库至少这么多文件/字节。现行
/// 选择上限（[`crate::models::COURSE_MAX_KNOWLEDGE_BASES`] = 5）下等分
/// （40/5 = 8 文件）已高于保底，保底是上限放宽时的护栏——采样永不把某个
/// 库压到零头。
pub(crate) const MULTI_KB_PER_BASE_MIN_FILES: usize = 4;
pub(crate) const MULTI_KB_PER_BASE_MIN_BYTES: usize = 16 * 1024;

/// 打分与选摘的单文件扫描上限：超长文件只读前 1MB（确定性——不引入机器
/// 相关的遍历预算，同一库同一目标在任何机器上产出同一份采样）。
const SAMPLE_SCAN_MAX_BYTES: usize = 1024 * 1024;

/// 目标感知采样的结果：样本清单（路径字典序）+ 披露快照。无目标路径
/// （字母序回退）与空库不产生披露，`samples` 与旧行为逐字一致。
pub(crate) struct SampleOutcome {
    pub samples: Vec<(String, String)>,
    pub disclosure: Option<SampleDisclosure>,
}

impl SampleOutcome {
    fn legacy(samples: Vec<(String, String)>) -> Self {
        Self { samples, disclosure: None }
    }
}

/// 目标感知采样（t2）：全库候选枚举 → 目标词条打分排序（确定性）→ 相关集合
/// + 连贯预算补相邻 → 输出仍按路径字典序的 `(path, excerpt)`。
///
/// 无目标（词条化为空）或根目录缺失时退回旧行为：字母序 take + 文件头前缀，
/// 不产生披露（旧快照形状不变，t1 的兼容路径）。

/// 目标感知采样（t2）的预算：这是 `learning_courses.samples_json` 的
/// 伴随披露快照，独立字段存放，既有消费方仍按 `Vec<(String, String)>`
/// 解析 `samples_json` 本身——新元数据从不混入旧形状。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SampleDisclosure {
    /// 采样依据的目标文本（无目标路径为 `None`）。
    #[serde(default)]
    pub goal: Option<String>,
    /// 全库候选 markdown 文件数（不含 README.md）。
    #[serde(default)]
    pub total_files: usize,
    /// 已采入样的文件数（等于 `samples.len()`）。
    #[serde(default)]
    pub sampled_files: usize,
    /// 已采文件中因连贯预算补入（低分但结构必需）的数量。
    #[serde(default)]
    pub coherence_files: usize,
    /// 目标与库无交集：没有任何文件命中目标词条，采样退回默认（字母序）
    /// 选择——生成不因目标措辞与库不一致而失败，但披露必须如实说明。
    #[serde(default)]
    pub goal_fallback: bool,
    /// 目标的确定性词条集（t3 复用）：建课时切好随披露持久化，课时检索
    /// 查询直接取用——建课后稳定复用同一套词条，不因实现改动漂移。
    /// 旧披露（t2 早期快照）没有这个字段：读出为空，消费方回退到对目标
    /// 文本重新切片（同一纯函数，结果一致）。
    #[serde(default)]
    pub goal_terms: Vec<String>,
    /// 完整文件树：全库候选文件的相对路径（字典序，目录结构保留）。
    #[serde(default)]
    pub tree: Vec<String>,
    /// 因低于相关度阈值未入样的文件路径（字典序）。
    #[serde(default)]
    pub low_score_files: Vec<String>,
    /// 多知识库（t4）：逐库归属与计数（单库为空——披露形状与 t2 逐字一致，
    /// 序列化时整体省略）。命名空间是采样路径的前缀（`{namespace}/rel`），
    /// 路径→库的归属解析与「来源：库A、库B」展示都由它承载。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bases: Vec<SampleBaseDisclosure>,
}

/// 单库的采样归属（t4 披露）：库 id / 库名 / 路径命名空间 / 逐库计数。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SampleBaseDisclosure {
    /// 知识库 id（归属解析的事实源）。
    #[serde(default)]
    pub kb_id: String,
    #[serde(default)]
    pub name: String,
    /// 采样路径前缀：`{namespace}/rel`。库名清洗（分隔符 → `_`）后按库序
    /// 消歧（重名库第二个起加 `~2`、`~3`…），确定性。
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub total_files: usize,
    #[serde(default)]
    pub sampled_files: usize,
    #[serde(default)]
    pub coherence_files: usize,
    #[serde(default)]
    pub low_score_files: usize,
    /// 该库目标零命中：采样退回默认顺序（逐库如实标注）。
    #[serde(default)]
    pub goal_fallback: bool,
}

impl SampleDisclosure {
    /// 简报里的如实披露行："已按目标采样 x/y 文件"（附加连贯补入与未采
    /// 计数）。确定性纯函数，供大纲提示词与诊断共用。
    pub fn brief_line(&self) -> String {
        // 目标与库无交集时不冒充"按目标采样"：说清退回默认顺序，并给出
        // 可执行的下一步（换目标措辞或补库内容）。
        if self.goal_fallback {
            return format!(
                "库内没有命中目标词条的文件，已按默认顺序采样 {}/{} 个文件（目标措辞与库内容可能不一致）",
                self.sampled_files, self.total_files
            );
        }
        let mut line = format!(
            "已按目标采样 {}/{} 个文件",
            self.sampled_files, self.total_files
        );
        if self.coherence_files > 0 {
            line.push_str(&format!("（其中连贯补入 {} 个）", self.coherence_files));
        }
        if !self.low_score_files.is_empty() {
            line.push_str(&format!(
                "；{} 个低分文件未采样（按需可在课程详情查看）",
                self.low_score_files.len()
            ));
        }
        // 多库（t4）：部分库目标零命中时逐库如实标注——命中的库按目标采样，
        // 零命中的库退回默认顺序，披露不把它们混成一句"按目标采样"。
        let fallback_bases: Vec<&str> = self
            .bases
            .iter()
            .filter(|base| base.goal_fallback)
            .map(|base| base.name.as_str())
            .collect();
        if !fallback_bases.is_empty() {
            line.push_str(&format!(
                "；{} 未命中目标词条，已按默认顺序采样",
                fallback_bases.join("、")
            ));
        }
        line
    }
}

/// Sample the markdown corpus under `root` with explicit budgets. Owned by
/// the learning pipeline (not the knowledge autogen module): course
/// generation is the only caller, and keeping the budget constants next to
/// the pipeline keeps the input-size contract local.
pub(crate) async fn sample_base_files_with_budget(
    root: &Path,
    max_files: usize,
    max_per_file: usize,
    max_total: usize,
) -> Vec<(String, String)> {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        sample_base_files_blocking(&root, max_files, max_per_file, max_total)
    })
    .await
    .unwrap_or_default()
}

/// 目标感知采样的异步入口（与 [`sample_base_files_with_budget`] 同形的显式
/// 预算签名，供单测与调用方复用）。
pub(crate) async fn sample_goal_aware_with_budget(
    root: &Path,
    goal: &str,
    max_files: usize,
    max_per_file: usize,
    max_total: usize,
) -> SampleOutcome {
    let root = root.to_path_buf();
    let goal = goal.to_owned();
    tokio::task::spawn_blocking(move || {
        sample_goal_aware_blocking(&root, &goal, max_files, max_per_file, max_total)
    })
    .await
    .unwrap_or_else(|_| SampleOutcome::legacy(Vec::new()))
}

fn sample_goal_aware_blocking(
    root: &Path,
    goal: &str,
    max_files: usize,
    max_per_file: usize,
    max_total: usize,
) -> SampleOutcome {
    let selection = base_selection(root, goal, max_files, max_per_file, max_total);
    SampleOutcome { samples: selection.samples, disclosure: selection.disclosure }
}

/// 单库采样的读数（多库分摊的内部件，也是单库披露的原料）：样本 + 计数。
/// `disclosure` 与 t2 的单库披露同源（无目标路径为 `None`——旧形状）。
struct BaseSelection {
    samples: Vec<(String, String)>,
    disclosure: Option<SampleDisclosure>,
    total_files: usize,
    tree: Vec<String>,
    coherence_files: usize,
    low_score_files: Vec<String>,
    goal_fallback: bool,
}

/// 单库采样的唯一实现（目标感知 / 无目标回退共用）：目标词条非空且库里有
/// 候选时走目标路径（相关度选入 + 连贯补入），否则逐字回退旧行为（字母序
/// take + 文件头前缀）。
fn base_selection(
    root: &Path,
    goal: &str,
    max_files: usize,
    max_per_file: usize,
    max_total: usize,
) -> BaseSelection {
    let terms = term_slices(goal);
    base_selection_with_terms(root, goal, &terms, max_files, max_per_file, max_total)
}

/// [`base_selection`] 的词条显式版（多库分摊逐库复用同一实现与同一套词条）。
fn base_selection_with_terms(
    root: &Path,
    goal: &str,
    terms: &[String],
    max_files: usize,
    max_per_file: usize,
    max_total: usize,
) -> BaseSelection {
    if terms.is_empty() || max_files == 0 {
        // 无目标 / 目标全是标点：回退旧行为（字母序 + 文件头前缀），不披露。
        let candidates = candidate_files(root);
        let samples = legacy_take(&candidates, root, max_files, max_per_file, max_total);
        let sampled: HashSet<&str> = samples.iter().map(|(path, _)| path.as_str()).collect();
        let low_score_files: Vec<String> = candidates
            .iter()
            .filter(|path| !sampled.contains(path.as_str()))
            .cloned()
            .collect();
        return BaseSelection {
            samples,
            disclosure: None,
            total_files: candidates.len(),
            tree: candidates,
            coherence_files: 0,
            low_score_files,
            goal_fallback: false,
        };
    }
    let candidates = candidate_files(root);
    if candidates.is_empty() {
        return BaseSelection {
            samples: Vec::new(),
            disclosure: None,
            total_files: 0,
            tree: Vec::new(),
            coherence_files: 0,
            low_score_files: Vec::new(),
            goal_fallback: false,
        };
    }
    // 打分：与 KnowledgeService::score_md 同尺（路径词条 +4、首标题词条 +3、
    // 正文词频每词条封顶 5），分数为 0 = 低于相关度阈值。读不出的文件按 0 分
    // 参与——它们进不了样本，但必须出现在披露的低分未采清单里（披露不许有洞）。
    let mut scored: Vec<ScoredFile> = Vec::with_capacity(candidates.len());
    for path in &candidates {
        let content = read_prefix_lossy(&root.join(path), SAMPLE_SCAN_MAX_BYTES).unwrap_or_default();
        let content_lc = content.to_lowercase();
        let heading_lc = first_heading_text(&content).to_lowercase();
        let score = goal_relevance_score(path, &heading_lc, &content_lc, &terms);
        scored.push(ScoredFile { path: path.clone(), score, content });
    }
    // 相关集合：分数降序，同分按路径字典序（确定性）。
    let mut relevant: Vec<usize> = (0..scored.len())
        .filter(|index| scored[*index].score > 0)
        .collect();
    relevant.sort_by(|left, right| {
        scored[*right]
            .score
            .cmp(&scored[*left].score)
            .then_with(|| scored[*left].path.cmp(&scored[*right].path))
    });
    let selected_relevant: Vec<usize> = relevant.into_iter().take(max_files).collect();
    let mut sampled_paths: HashSet<String> = selected_relevant
        .iter()
        .map(|index| scored[*index].path.clone())
        .collect();
    // 目标与库无交集（无任何文件命中词条）：退回默认字母序采样——目标措辞
    // 与库不一致时生成照常可跑（不比今天差），但披露如实标注 fallback，绝不
    // 冒充"按目标采样"。
    if sampled_paths.is_empty() {
        let samples = legacy_take(&candidates, root, max_files, max_per_file, max_total);
        let sampled: HashSet<&str> = samples.iter().map(|(path, _)| path.as_str()).collect();
        let low_score_files: Vec<String> = candidates
            .iter()
            .filter(|path| !sampled.contains(path.as_str()))
            .cloned()
            .collect();
        let disclosure = SampleDisclosure {
            goal: Some(goal.trim().to_owned()),
            total_files: candidates.len(),
            sampled_files: samples.len(),
            coherence_files: 0,
            goal_fallback: true,
            goal_terms: terms.to_vec(),
            low_score_files: low_score_files.clone(),
            tree: candidates.clone(),
            bases: Vec::new(),
        };
        return BaseSelection {
            samples,
            disclosure: Some(disclosure),
            total_files: candidates.len(),
            tree: candidates,
            coherence_files: 0,
            low_score_files,
            goal_fallback: true,
        };
    }
    // 连贯预算：相关文件优先占额度，余下额度里最多补 30% 的结构相邻文件
    // （同目录兄弟优先、序号相邻次之——课程式库的中间必经章不断链）。
    let coherence_cap = max_files * GOAL_SAMPLE_COHERENCE_PERCENT / 100;
    let coherence_slots = coherence_cap.min(max_files.saturating_sub(sampled_paths.len()));
    let coherence_paths = coherence_fill(&candidates, &sampled_paths, coherence_slots);
    sampled_paths.extend(coherence_paths.iter().cloned());
    let mut selected: Vec<&str> = sampled_paths.iter().map(String::as_str).collect();
    selected.sort_unstable();
    // 摘录：路径字典序交付（结构信号交给大纲 agent），总量封顶。
    let content_of = |path: &str| -> Option<&str> {
        scored.iter().find(|file| file.path == path).map(|file| file.content.as_str())
    };
    let mut samples: Vec<(String, String)> = Vec::with_capacity(selected.len());
    let mut total = 0usize;
    for path in selected {
        if total >= max_total {
            break;
        }
        let budget = max_per_file.min(max_total - total);
        let Some(content) = content_of(path) else { continue };
        let excerpt = goal_excerpt(content, &terms, budget);
        if excerpt.trim().is_empty() {
            continue;
        }
        total += excerpt.len();
        samples.push((path.to_owned(), excerpt));
    }
    let sampled_paths: HashSet<&str> = samples.iter().map(|(path, _)| path.as_str()).collect();
    let coherence_files = coherence_paths
        .iter()
        .filter(|path| sampled_paths.contains(path.as_str()))
        .count();
    let low_score_files = scored
        .iter()
        .filter(|file| file.score == 0 && !sampled_paths.contains(file.path.as_str()))
        .map(|file| file.path.clone())
        .collect::<Vec<String>>();
    let disclosure = SampleDisclosure {
        goal: Some(goal.trim().to_owned()),
        total_files: candidates.len(),
        sampled_files: samples.len(),
        coherence_files,
        goal_fallback: false,
        goal_terms: terms.to_vec(),
        tree: candidates.clone(),
        low_score_files: low_score_files.clone(),
        bases: Vec::new(),
    };
    BaseSelection {
        samples,
        disclosure: Some(disclosure),
        total_files: candidates.len(),
        tree: candidates,
        coherence_files,
        low_score_files,
        goal_fallback: false,
    }
}

/// 一个知识库的采样输入（t4）：库 id（归属事实源）、库名（命名空间源）与
/// 内容根（采样读的文件树）。
pub(crate) struct SampleBaseInput<'a> {
    pub kb_id: &'a str,
    pub name: &'a str,
    pub root: &'a Path,
}

/// 多库采样的异步入口（t4）：单库逐字走 t2 旧路径（不命名空间化、披露形状
/// 不变）；多库按库分摊预算、路径带 `{库名}/rel` 命名空间、逐库归属进披露。
pub(crate) async fn sample_bases_with_budget(
    bases: &[SampleBaseInput<'_>],
    goal: Option<&str>,
    max_files: usize,
    max_per_file: usize,
    max_total: usize,
) -> SampleOutcome {
    if bases.is_empty() {
        return SampleOutcome::legacy(Vec::new());
    }
    if bases.len() == 1 {
        // 单库零回归：行为与披露与 t2 逐字一致（无命名空间、无逐库条目）。
        let root = bases[0].root.to_path_buf();
        let goal = goal.map(ToOwned::to_owned);
        return tokio::task::spawn_blocking(move || match goal {
            Some(goal) => sample_goal_aware_blocking(&root, &goal, max_files, max_per_file, max_total),
            None => SampleOutcome::legacy(sample_base_files_blocking(
                &root,
                max_files,
                max_per_file,
                max_total,
            )),
        })
        .await
        .unwrap_or_else(|_| SampleOutcome::legacy(Vec::new()));
    }
    let inputs: Vec<(String, String, std::path::PathBuf)> = bases
        .iter()
        .map(|base| (base.kb_id.to_owned(), base.name.to_owned(), base.root.to_path_buf()))
        .collect();
    let goal = goal.map(ToOwned::to_owned);
    tokio::task::spawn_blocking(move || {
        sample_multi_base_blocking(&inputs, goal.as_deref(), max_files, max_per_file, max_total)
    })
    .await
    .unwrap_or_else(|_| SampleOutcome::legacy(Vec::new()))
}

/// 多库采样（阻塞体）：库序 = 库名 + 库 id 字典序（确定性，与提交顺序无关）
/// → 逐库等分配额（余量借给后面的库，每库保底
/// [`MULTI_KB_PER_BASE_MIN_FILES`]/[`MULTI_KB_PER_BASE_MIN_BYTES`]）→ 路径加
/// 命名空间 → 合并清单回到全局字典序（不倒退纪律：结构按路径序读）。
/// 无目标时逐库走字母序回退，但**仍产出披露**——命名空间需要披露承载库归属。
fn sample_multi_base_blocking(
    bases: &[(String, String, std::path::PathBuf)],
    goal: Option<&str>,
    max_files: usize,
    max_per_file: usize,
    max_total: usize,
) -> SampleOutcome {
    // 确定性库序：库名（trim）字典序，同名前缀按库 id——用户提交顺序不影响
    // 分摊结果（乱序提交同一组库得到同一份采样）。
    let mut order: Vec<usize> = (0..bases.len()).collect();
    order.sort_by(|left, right| {
        bases[*left]
            .1
            .trim()
            .cmp(bases[*right].1.trim())
            .then_with(|| bases[*left].0.cmp(&bases[*right].0))
    });
    // 命名空间：库名清洗 + 同库序内消歧（重名库第二个起加 `~2`、`~3`…）。
    let mut namespaces: Vec<String> = vec![String::new(); bases.len()];
    let mut used = HashSet::new();
    for index in &order {
        let (kb_id, name, _) = &bases[*index];
        let base_ns = sanitize_namespace(name, kb_id);
        let mut candidate = base_ns.clone();
        let mut suffix = 2usize;
        while !used.insert(candidate.clone()) {
            candidate = format!("{base_ns}~{suffix}");
            suffix += 1;
        }
        namespaces[*index] = candidate;
    }
    let goal = goal.map(str::trim).filter(|goal| !goal.is_empty());
    let terms = goal.map(term_slices).unwrap_or_default();
    let select = |root: &Path, quota_files: usize, quota_bytes: usize| -> BaseSelection {
        match goal {
            Some(goal) => goal_aware_selection(root, goal, &terms, quota_files, max_per_file, quota_bytes),
            None => legacy_selection(root, quota_files, max_per_file, quota_bytes),
        }
    };
    // 名额分摊（确定性）：逐库按「剩余 / 剩余库数」等分（余量前向借用——用不
    // 满的库把它后面的库的份额留大），每库保底
    // [`MULTI_KB_PER_BASE_MIN_FILES`]/[`MULTI_KB_PER_BASE_MIN_BYTES`]，总量
    // 永不超预算。第二遍把末尾库没用掉的余量按「打满配额的库」再分摊——
    // 小库不再让整锅预算缩水（确定性：按库序均分，余数给靠前的库）。
    let mut quotas: Vec<(usize, usize)> = Vec::with_capacity(order.len());
    let mut selections: Vec<BaseSelection> = Vec::with_capacity(order.len());
    let mut remaining_files = max_files;
    let mut remaining_bytes = max_total;
    for (position, index) in order.iter().enumerate() {
        let remaining_bases = order.len() - position;
        let quota_files = (remaining_files / remaining_bases)
            .max(MULTI_KB_PER_BASE_MIN_FILES)
            .min(remaining_files);
        let quota_bytes = (remaining_bytes / remaining_bases)
            .max(MULTI_KB_PER_BASE_MIN_BYTES)
            .min(remaining_bytes);
        let selection = select(&bases[*index].2, quota_files, quota_bytes);
        remaining_files -= selection.samples.len();
        remaining_bytes -= selection.samples.iter().map(|(_, text)| text.len()).sum::<usize>();
        quotas.push((quota_files, quota_bytes));
        selections.push(selection);
    }
    if remaining_files > 0 {
        let saturated: Vec<usize> = (0..selections.len())
            .filter(|index| quotas[*index].0 > 0 && selections[*index].samples.len() >= quotas[*index].0)
            .collect();
        if !saturated.is_empty() {
            let share_files = remaining_files / saturated.len();
            let share_bytes = remaining_bytes / saturated.len();
            let mut rest_files = remaining_files % saturated.len();
            for index in saturated {
                let extra_files = share_files + usize::from(rest_files > 0);
                rest_files = rest_files.saturating_sub(1);
                if extra_files == 0 {
                    continue;
                }
                let (quota_files, quota_bytes) = quotas[index];
                let extra_bytes = (extra_files * max_per_file).min(share_bytes);
                selections[index] = select(
                    &bases[order[index]].2,
                    quota_files + extra_files,
                    quota_bytes + extra_bytes,
                );
            }
        }
    }
    let mut samples: Vec<(String, String)> = Vec::new();
    let mut base_disclosures: Vec<SampleBaseDisclosure> = Vec::new();
    let mut tree: Vec<String> = Vec::new();
    let mut low_score_files: Vec<String> = Vec::new();
    let mut coherence_files = 0usize;
    for (position, index) in order.iter().enumerate() {
        let (kb_id, name, _) = &bases[*index];
        let selection = &selections[position];
        let namespace = &namespaces[*index];
        for (path, excerpt) in &selection.samples {
            samples.push((format!("{namespace}/{path}"), excerpt.clone()));
        }
        coherence_files += selection.coherence_files;
        tree.extend(selection.tree.iter().map(|path| format!("{namespace}/{path}")));
        low_score_files.extend(
            selection
                .low_score_files
                .iter()
                .map(|path| format!("{namespace}/{path}")),
        );
        base_disclosures.push(SampleBaseDisclosure {
            kb_id: kb_id.clone(),
            name: name.trim().to_owned(),
            namespace: namespace.clone(),
            total_files: selection.total_files,
            sampled_files: selection.samples.len(),
            coherence_files: selection.coherence_files,
            low_score_files: selection.low_score_files.len(),
            goal_fallback: selection.goal_fallback,
        });
    }
    // 合并清单回到全局字典序（每库内部序不变，跨库按命名空间分组）。
    samples.sort_by(|left, right| left.0.cmp(&right.0));
    tree.sort();
    low_score_files.sort();
    // 顶层 fallback：仅当目标对**所有**有候选的库都零命中（逐库如实值在
    // bases 里，不把部分命中冒充成全局失败，也不把部分回退冒充成按目标采样）。
    let goal_fallback = goal.is_some()
        && !base_disclosures.is_empty()
        && base_disclosures
            .iter()
            .all(|base| base.goal_fallback || base.total_files == 0);
    let disclosure = SampleDisclosure {
        goal: goal.map(str::to_owned),
        total_files: base_disclosures.iter().map(|base| base.total_files).sum(),
        sampled_files: samples.len(),
        coherence_files,
        goal_fallback,
        goal_terms: terms,
        tree,
        low_score_files,
        bases: base_disclosures,
    };
    SampleOutcome { samples, disclosure: Some(disclosure) }
}

/// 目标路径的单库采样（多库分摊复用 [`base_selection`]）。
fn goal_aware_selection(
    root: &Path,
    goal: &str,
    terms: &[String],
    max_files: usize,
    max_per_file: usize,
    max_total: usize,
) -> BaseSelection {
    let selection = base_selection_with_terms(root, goal, terms, max_files, max_per_file, max_total);
    selection
}

/// 无目标路径的单库采样：字母序 take + 文件头前缀（旧行为逐字一致）。
fn legacy_selection(
    root: &Path,
    max_files: usize,
    max_per_file: usize,
    max_total: usize,
) -> BaseSelection {
    let candidates = candidate_files(root);
    let samples = legacy_take(&candidates, root, max_files, max_per_file, max_total);
    let sampled: HashSet<&str> = samples.iter().map(|(path, _)| path.as_str()).collect();
    let low_score_files: Vec<String> = candidates
        .iter()
        .filter(|path| !sampled.contains(path.as_str()))
        .cloned()
        .collect();
    BaseSelection {
        samples,
        disclosure: None,
        total_files: candidates.len(),
        tree: candidates,
        coherence_files: 0,
        low_score_files,
        goal_fallback: false,
    }
}

/// 命名空间清洗（t4）：库名 trim 后把路径分隔符与控制字符替换为 `_`；清洗
/// 后为空（或只剩 `.`/`..`）退回 `kb-{id 前 8 位}`——命名空间必须非空且不
/// 携带路径语义。确定性纯函数。
fn sanitize_namespace(name: &str, kb_id: &str) -> String {
    let mut cleaned = String::new();
    for ch in name.trim().chars() {
        if ch == '/' || ch == '\\' || ch.is_control() {
            cleaned.push('_');
        } else {
            cleaned.push(ch);
        }
    }
    let cleaned = cleaned.trim().to_owned();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return format!("kb-{}", kb_id.chars().take(8).collect::<String>());
    }
    cleaned
}

/// 采样路径 → 来源库（t4）：`{namespace}/rel` 的命名空间命中披露里的库；
/// 裸路径（单库/旧课程）回退唯一来源库。解析不出返回 `None`（读侧再退课程
/// 主来源；披露不许凭空造归属）。
pub(crate) fn base_for_sample_path(
    path: &str,
    bases: &[SampleBaseDisclosure],
    fallback_ids: &[KnowledgeBaseId],
) -> Option<KnowledgeBaseId> {
    if let Some((namespace, _)) = path.split_once('/') {
        if let Some(base) = bases
            .iter()
            .find(|base| !base.namespace.is_empty() && base.namespace == namespace)
        {
            if let Ok(id) = KnowledgeBaseId::parse(&base.kb_id) {
                return Some(id);
            }
        }
    }
    // 裸路径（单库/旧课程）：唯一来源库是确定性归属。有逐库披露（多库）时
    // 不做回退——命名空间是唯一归属依据，绝不把未知前缀的路径错认给别的库
    // （已删库的路径必须解析成"无归属"，而不是落到幸存库）。
    match fallback_ids {
        [only] if bases.is_empty() => Some(only.clone()),
        _ => None,
    }
}

/// 某库的采样命名空间（多库课程读侧去前缀用）：披露逐库条目里按库 id 命中；
/// 单库/旧课程无条目 → `None`（路径本就没有命名空间）。
pub(crate) fn namespace_for_base<'a>(
    bases: &'a [SampleBaseDisclosure],
    kb_id: &KnowledgeBaseId,
) -> Option<&'a str> {
    bases
        .iter()
        .find(|base| base.kb_id == kb_id.as_str() && !base.namespace.is_empty())
        .map(|base| base.namespace.as_str())
}

/// 库内相对路径（去命名空间前缀，t4）：`{namespace}/rel` → `rel`；没有该
/// 前缀（单库裸路径/旧课程）原样返回——读侧永远拿库内路径去 `read_file`。
pub(crate) fn rel_path_for_base<'a>(path: &'a str, namespace: Option<&str>) -> &'a str {
    match namespace {
        Some(namespace) if !namespace.is_empty() => path
            .strip_prefix(namespace)
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or(path),
        _ => path,
    }
}

/// 蓝图的来源标注（t4）：逐课时把 source 路径解析回来源库，写入
/// [`SourceSpan::kb_id`]——快照/蓝图/课时三处的库归属由此统一。多库路径按
/// 命名空间解析；单库裸路径回退唯一来源库；解析不出的保持 `None`（读侧再
/// 退课程主来源，绝不猜）。
pub(crate) fn annotate_blueprint_sources(
    blueprint: &mut super::Blueprint,
    disclosure: Option<&SampleDisclosure>,
    fallback_ids: &[KnowledgeBaseId],
) {
    let empty: Vec<SampleBaseDisclosure> = Vec::new();
    let bases = disclosure.map(|d| d.bases.as_slice()).unwrap_or(empty.as_slice());
    for module in &mut blueprint.modules {
        for lesson in &mut module.lessons {
            let Some(source) = lesson.source.as_mut() else { continue };
            source.kb_id = base_for_sample_path(&source.path, bases, fallback_ids);
        }
    }
}

/// 一个候选文件的打分原料：路径、相关度、扫描上限内的正文。
struct ScoredFile {
    path: String,
    score: u32,
    content: String,
}

/// 目标相关度打分（与检索侧 `score_md` 同尺，可直接对照两处行为）：
/// 词条命中路径 +4、命中首个标题 +3、正文词频每词条封顶 5。分数为 0 即
/// 低于相关度阈值（不进相关集合）。确定性、可重放，不含 LLM。
fn goal_relevance_score(path: &str, heading_lower: &str, content_lower: &str, terms: &[String]) -> u32 {
    let path_lower = path.to_lowercase();
    let mut score: u32 = 0;
    for term in terms {
        if path_lower.contains(term.as_str()) {
            score += 4;
        }
        if heading_lower.contains(term.as_str()) {
            score += 3;
        }
        score += term_hits(content_lower, term).min(5) as u32;
    }
    score
}

/// 首个 markdown 标题（`# ...`）的文本；无标题返回空串。
fn first_heading_text(content: &str) -> String {
    for line in content.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("# ") {
            return rest.trim().to_owned();
        }
        if let Some(rest) = trimmed.strip_prefix('#') {
            // `## 标题` 等同首标题；`#` 后无空格的 `#标题` 也接受。
            return rest.trim_start_matches('#').trim().to_owned();
        }
    }
    String::new()
}

/// 连贯补入（t2 的不倒退纪律）：从已选文件出发，反复补入同目录下字典序紧邻
/// 的前后文件（库按 01-…09- 编排时，序号相邻 = 字典序相邻），直到预算用尽。
/// 补入文件即使相关度为 0 也保留（结构优先于相关度，读什么看相关度、按什么
/// 顺序看看结构）。返回补入的路径，按发现顺序，确定性。
fn coherence_fill(all: &[String], selected: &HashSet<String>, slots: usize) -> Vec<String> {
    let mut added: Vec<String> = Vec::new();
    if slots == 0 {
        return added;
    }
    let mut queued: HashSet<String> = selected.clone();
    let mut queue: VecDeque<String> = selected.iter().cloned().collect();
    queue.make_contiguous().sort_unstable();
    while let Some(path) = queue.pop_front() {
        if added.len() >= slots {
            break;
        }
        let directory = parent_dir(&path);
        let siblings: Vec<&String> = all
            .iter()
            .filter(|candidate| parent_dir(candidate) == directory)
            .collect();
        let Some(position) = siblings.iter().position(|candidate| **candidate == path) else {
            continue;
        };
        let neighbours = [
            position.checked_sub(1).map(|index| siblings[index]),
            siblings.get(position + 1).copied(),
        ];
        for neighbour in neighbours.into_iter().flatten() {
            if added.len() >= slots {
                break;
            }
            if queued.insert(neighbour.clone()) {
                added.push(neighbour.clone());
                queue.push_back(neighbour.clone());
            }
        }
    }
    added
}

/// 路径的目录部分（`a/b/c.md` → `a/b`；顶层文件 → 空串）。同目录判据。
fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

/// 单文件摘录：正文不超过预算时整篇交付；超预算时按目标词条命中定位相关段落
/// 选摘（确定性：命中段落 ± 上下文窗口，权重降序预取、原文顺序交付），无命中
/// 段落则回退文件头。大文件里的相关事实因此不会因为落在中段而丢失，邻接命中
/// 段的结论句也不会被切掉。
///
/// t2 目标感知采样与 t3 课时 grounding 的超预算截断共用一把尺：同一条目在
/// 采样与写作两处得到同一份选摘。
pub(crate) fn goal_excerpt(content: &str, terms: &[String], budget: usize) -> String {
    if content.len() <= budget {
        return content.to_owned();
    }
    let blocks: Vec<&str> = split_paragraphs(content);
    let hits: Vec<usize> = blocks
        .iter()
        .map(|block| {
            let lower = block.to_lowercase();
            terms.iter().map(|term| term_hits(&lower, term)).sum()
        })
        .collect();
    // 命中段落权重 = 自身命中数；± 一个上下文窗口（前后相邻段各 +1）——
    // 相关事实常与命中的标题/定义段邻接（如「## 聚类分析要点」后紧跟结论句）。
    let weight: Vec<usize> = (0..blocks.len())
        .map(|index| {
            hits[index]
                + usize::from(index > 0 && hits[index - 1] > 0)
                + usize::from(hits.get(index + 1).is_some_and(|next| *next > 0))
        })
        .collect();
    if weight.iter().all(|count| *count == 0) {
        return head_excerpt(content, budget);
    }
    // 只取有相关性的段落：不与无关填充瓜分预算（上下文是稀缺资源）。
    let mut order: Vec<usize> = (0..blocks.len()).filter(|index| weight[*index] > 0).collect();
    order.sort_by(|left, right| {
        weight[*right].cmp(&weight[*left]).then_with(|| left.cmp(right))
    });
    let mut picked: Vec<usize> = Vec::new();
    let mut used = 0usize;
    for index in order {
        if used >= budget {
            break;
        }
        let cost = blocks[index].len() + if picked.is_empty() { 0 } else { 2 };
        if used + cost > budget {
            continue;
        }
        picked.push(index);
        used += cost;
    }
    // 命中段落全都装不进预算（单段落超预算一类）：回退文件头——摘录绝不
    // 产出空串（空片段在课时 grounding 里等于没有 grounding）。
    if picked.is_empty() {
        return head_excerpt(content, budget);
    }
    picked.sort_unstable();
    let mut excerpt = picked
        .into_iter()
        .map(|index| blocks[index].trim())
        .collect::<Vec<_>>()
        .join("\n\n");
    if excerpt.len() > budget {
        excerpt = head_excerpt(&excerpt, budget);
    }
    excerpt
}

/// 段落切分：空行分段（markdown 的段落边界），保留段落内的换行；纯空白段丢弃。
fn split_paragraphs(content: &str) -> Vec<&str> {
    let mut blocks: Vec<&str> = Vec::new();
    let mut start: Option<usize> = None;
    let mut offset = 0usize;
    for line in content.split_inclusive('\n') {
        let blank = line.trim().is_empty();
        match (blank, start) {
            (false, None) => start = Some(offset),
            (true, Some(from)) => {
                blocks.push(&content[from..offset]);
                start = None;
            }
            _ => {}
        }
        offset += line.len();
    }
    if let Some(from) = start {
        blocks.push(&content[from..]);
    }
    blocks.retain(|block| !block.trim().is_empty());
    blocks
}

/// 文件头前缀（预算内在字符边界截断）。
fn head_excerpt(content: &str, budget: usize) -> String {
    if content.len() <= budget {
        return content.to_owned();
    }
    let mut end = budget;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    content[..end].to_owned()
}

/// 库内候选 markdown 文件（相对路径、字典序、排除 README.md）。
fn candidate_files(root: &Path) -> Vec<String> {
    if !root.is_dir() {
        return Vec::new();
    }
    let mut rels: Vec<String> = walkdir::WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|entry| entry.file_type().is_file() && is_md(entry.path()))
        .filter_map(|entry| {
            let rel = entry.path().strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/");
            (rel != "README.md").then_some(rel)
        })
        .collect();
    rels.sort();
    rels
}

fn sample_base_files_blocking(
    root: &Path,
    max_files: usize,
    max_per_file: usize,
    max_total: usize,
) -> Vec<(String, String)> {
    legacy_take(&candidate_files(root), root, max_files, max_per_file, max_total)
}

/// 旧采样（无目标）：候选清单（字典序）里 take 预算内的文件、按文件头前缀
/// 取摘录。候选清单由调用方给出——多库分摊复用同一次目录遍历的读数。
fn legacy_take(
    rels: &[String],
    root: &Path,
    max_files: usize,
    max_per_file: usize,
    max_total: usize,
) -> Vec<(String, String)> {
    let mut samples = Vec::new();
    let mut total = 0usize;
    for rel in rels.iter().take(max_files) {
        if total >= max_total {
            break;
        }
        let budget = max_per_file.min(max_total - total);
        let Some(excerpt) = read_prefix_lossy(&root.join(rel), budget) else {
            continue;
        };
        if excerpt.trim().is_empty() {
            continue;
        }
        total += excerpt.len();
        samples.push((rel.clone(), excerpt));
    }
    samples
}

/// Read at most `limit` bytes from the start of `path`, lossily decoded
/// (a multi-byte char cut at the boundary degrades to U+FFFD, never a panic).
fn read_prefix_lossy(path: &Path, limit: usize) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; limit];
    let mut read = 0usize;
    loop {
        match file.read(&mut buf[read..]) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(_) => return None,
        }
        if read == buf.len() {
            break;
        }
    }
    Some(String::from_utf8_lossy(&buf[..read]).into_owned())
}

fn is_md(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
}


/// 多知识库采样入口（t4）：逐库解析元数据与内容根，预算按库分摊、路径带
/// `{库名}/rel` 命名空间、逐库归属进披露。单库走逐字旧行为（不命名空间化、
/// 披露形状不变）。
///
/// 缺库口径（写进票面 Resolution）：单库内容目录缺失/无 markdown → 沿用
/// 400（零回归）；多库下按库跳过（缺失的库计数 0、不阻断其他库），仅当
/// **全部**库都没有可采文件时才 400。
pub(crate) async fn sample_bases(
    knowledge: &KnowledgeService,
    knowledge_base_ids: &[String],
    goal: Option<&str>,
) -> Result<SampleOutcome, AppError> {
    let goal = goal.map(str::trim).filter(|goal| !goal.is_empty());
    let goal_aware = goal.is_some_and(|goal| !term_slices(goal).is_empty());
    let (max_files, max_per_file, max_total) = if goal_aware {
        (GOAL_SAMPLE_MAX_FILES, GOAL_SAMPLE_MAX_PER_FILE, GOAL_SAMPLE_MAX_TOTAL)
    } else {
        (
            LEARNING_SAMPLE_MAX_FILES,
            LEARNING_SAMPLE_MAX_PER_FILE,
            LEARNING_SAMPLE_MAX_TOTAL,
        )
    };
    // A local folder's public `root_path` is its read-only source location.
    // Course generation samples the app-managed Markdown projection so it
    // sees converted documents while the source remains available.
    let mut resolved: Vec<(String, String, std::path::PathBuf)> = Vec::with_capacity(knowledge_base_ids.len());
    for kb_id in knowledge_base_ids {
        if knowledge_base_ids.len() == 1 {
            let content_root = knowledge.content_root_for_base(kb_id).await?;
            if !content_root.is_dir() {
                return Err(AppError::BadRequest(
                    "selected knowledge base content directory does not exist".into(),
                ));
            }
            resolved.push((kb_id.clone(), String::new(), content_root));
            continue;
        }
        // 逐库核验存在（写票面要求）：库不存在/内容根缺失按库跳过，披露里
        // 计数为 0——不让一个坏库阻断其余库的建课。
        let name = match knowledge.get_base_info(kb_id).await {
            Ok(base) => base.name,
            Err(_) => {
                resolved.push((kb_id.clone(), kb_id.clone(), std::path::PathBuf::from("")));
                continue;
            }
        };
        let content_root = knowledge
            .content_root_for_base(kb_id)
            .await
            .unwrap_or_else(|_| std::path::PathBuf::from(""));
        resolved.push((kb_id.clone(), name, content_root));
    }
    let outcome = if knowledge_base_ids.len() == 1 {
        let root = &resolved[0].2;
        match goal {
            // 目标词条化为空（纯标点等）时按无目标处理：预算与行为都回到旧路径。
            Some(goal) if goal_aware => {
                sample_goal_aware_with_budget(root, goal, max_files, max_per_file, max_total).await
            }
            _ => SampleOutcome::legacy(
                sample_base_files_with_budget(root, max_files, max_per_file, max_total).await,
            ),
        }
    } else {
        let inputs: Vec<SampleBaseInput<'_>> = resolved
            .iter()
            .map(|(kb_id, name, root)| SampleBaseInput {
                kb_id: kb_id.as_str(),
                name: name.as_str(),
                root: root.as_path(),
            })
            .collect();
        sample_bases_with_budget(&inputs, goal, max_files, max_per_file, max_total).await
    };
    if outcome.samples.is_empty() {
        return Err(AppError::BadRequest(if knowledge_base_ids.len() > 1 {
            "selected knowledge bases have no markdown documents to generate a course from".into()
        } else {
            "knowledge base has no markdown documents to generate a course from".into()
        }));
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sampling_skips_readme_and_caps_budgets() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::write(root.join("README.md"), "# old readme").unwrap();
        for i in 0..25 {
            std::fs::write(root.join(format!("f{i:02}.md")), format!("# 文件 {i}\n正文")).unwrap();
        }
        std::fs::write(root.join("big.md"), "x".repeat(LEARNING_SAMPLE_MAX_PER_FILE * 2)).unwrap();

        // Explicit 20-file budget: the corpus has 26 non-README files, so the
        // cap (not the corpus) decides the sample count.
        let samples = sample_base_files_with_budget(root, 20, LEARNING_SAMPLE_MAX_PER_FILE, LEARNING_SAMPLE_MAX_TOTAL)
            .await;
        assert_eq!(samples.len(), 20, "{:?}", samples.iter().map(|s| &s.0).collect::<Vec<_>>());
        assert!(samples.iter().all(|(rel, _)| rel != "README.md"));
        let big = samples.iter().find(|(rel, _)| rel == "big.md").expect("big.md sampled (sorted first)");
        assert!(big.1.len() <= LEARNING_SAMPLE_MAX_PER_FILE);
        let total: usize = samples.iter().map(|(_, s)| s.len()).sum();
        assert!(total <= LEARNING_SAMPLE_MAX_TOTAL);
    }

    #[tokio::test]
    async fn sampling_with_learning_budget_keeps_larger_excerpts_under_caps() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::write(root.join("big.md"), "x".repeat(LEARNING_SAMPLE_MAX_PER_FILE * 2)).unwrap();
        std::fs::write(root.join("other.md"), "# 正文\n内容").unwrap();

        let samples = sample_base_files_with_budget(
            root,
            LEARNING_SAMPLE_MAX_FILES,
            LEARNING_SAMPLE_MAX_PER_FILE,
            LEARNING_SAMPLE_MAX_TOTAL,
        )
        .await;
        let total: usize = samples.iter().map(|(_, s)| s.len()).sum();
        assert!(total <= LEARNING_SAMPLE_MAX_TOTAL);
        let big = samples.iter().find(|(rel, _)| rel == "big.md").expect("big.md sampled");
        assert!(big.1.len() <= LEARNING_SAMPLE_MAX_PER_FILE);
    }

    #[tokio::test]
    async fn sampling_empty_or_missing_root() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(sample_base_files_with_budget(dir.path(), 1, 1, 1).await.is_empty());
        assert!(sample_base_files_with_budget(&dir.path().join("nope"), 1, 1, 1).await.is_empty());
    }

    /// 目标感知采样的最小预算（测试内联，避免依赖生产常量）：
    /// 40 文件、单文件 24KB、总 48KB。
    const GOAL_MAX_FILES: usize = 40;
    const GOAL_PER_FILE: usize = 24 * 1024;
    const GOAL_TOTAL: usize = 48 * 1024;

    async fn write_file(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    /// 混杂库：目标相关文档入样、高噪声类别不入样，且输出保持路径字典序
    /// （结构信号原样交给大纲 agent）。
    #[tokio::test]
    async fn goal_sampling_selects_relevant_files_in_lexicographic_order() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        write_file(root, "clippings/收据-2024-03.md", "# 收据\n餐饮 88 元，交通 32 元。").await;
        write_file(root, "clippings/会议记录-0301.md", "# 会议记录\n讨论了排期与人力。").await;
        write_file(root, "notes/向量代数.md", "# 向量代数\n向量的加法与数乘。").await;
        write_file(
            root,
            "notes/data/python-数据分析-基础.md",
            "# Python 数据分析基础\n用 pandas 读取数据并做分组聚合。",
        )
        .await;
        write_file(
            root,
            "notes/data/数据分析案例.md",
            "# 数据分析案例\n销售数据的清洗、聚合与可视化。",
        )
        .await;

        let outcome = sample_goal_aware_with_budget(
            root,
            "零基础学 Python 数据分析",
            GOAL_MAX_FILES,
            GOAL_PER_FILE,
            GOAL_TOTAL,
        )
        .await;
        let paths: Vec<&str> = outcome.samples.iter().map(|(path, _)| path.as_str()).collect();
        assert!(paths.contains(&"notes/data/python-数据分析-基础.md"), "{paths:?}");
        assert!(paths.contains(&"notes/data/数据分析案例.md"), "{paths:?}");
        assert!(
            !paths.iter().any(|path| path.starts_with("clippings/")),
            "高噪声类别不入样: {paths:?}"
        );
        // 结构信号靠路径序：输出必须严格字典序。
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted);

        let disclosure = outcome.disclosure.expect("goal run discloses sampling");
        assert_eq!(disclosure.goal.as_deref(), Some("零基础学 Python 数据分析"));
        assert_eq!(disclosure.total_files, 5);
        assert_eq!(disclosure.sampled_files, outcome.samples.len());
        assert_eq!(
            disclosure.low_score_files,
            vec![
                "clippings/会议记录-0301.md".to_owned(),
                "clippings/收据-2024-03.md".to_owned(),
                "notes/向量代数.md".to_owned(),
            ]
        );
        assert_eq!(disclosure.tree.len(), 5, "完整文件树（含目录结构）");
        assert!(disclosure.brief_line().contains("已按目标采样 2/5 个文件"));
    }

    /// 课程式库专项（不倒退）：库按 01-…09- 编排、目标只命中其中一章时，
    /// 连贯预算把章节主干连成不中断的一条链（中间必经章不缺席），清单元
    /// 顺序仍是字典序。
    #[tokio::test]
    async fn goal_sampling_keeps_the_course_trunk_intact() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        let chapters = [
            "01-开篇与学习方法",
            "02-数据结构基础",
            "03-流程控制",
            "04-函数与模块",
            "05-聚类分析入门",
            "06-回归与预测",
            "07-模型评估",
            "08-工程落地",
            "09-综合实战",
        ];
        for chapter in chapters {
            let body = if chapter.starts_with("05") {
                "# 聚类分析入门\nK-means 的直觉：把样本按距离聚成簇。"
            } else {
                // 其余章节不含任何目标词条——只有结构相邻把它们连起来。
                "# 章节正文\n本节覆盖本主题的基础内容与例题。"
            };
            write_file(root, &format!("课程/{chapter}.md"), body).await;
        }

        let outcome = sample_goal_aware_with_budget(
            root,
            "掌握聚类分析",
            GOAL_MAX_FILES,
            GOAL_PER_FILE,
            GOAL_TOTAL,
        )
        .await;
        let paths: Vec<&str> = outcome.samples.iter().map(|(path, _)| path.as_str()).collect();
        for chapter in chapters {
            let expected = format!("课程/{chapter}.md");
            assert!(
                paths.contains(&expected.as_str()),
                "章节主干不断链，缺少 {expected}: {paths:?}"
            );
        }
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted, "清单元顺序仍是字典序");
        let disclosure = outcome.disclosure.unwrap();
        assert_eq!(disclosure.coherence_files, 8, "仅 1 章直接相关，其余靠连贯预算");
        assert!(disclosure.low_score_files.is_empty(), "补入的低分章节不算未采样");
    }

    /// 无目标路径（t1 的兼容路径）：与旧行为逐字一致——字母序 take + 文件头
    /// 前缀，且不产生披露（旧课程快照形状不变）。
    #[tokio::test]
    async fn goal_sampling_without_a_goal_falls_back_to_the_legacy_result() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        for i in 0..6 {
            write_file(root, &format!("f{i:02}.md"), &format!("# 文件 {i}\n正文 {i}")).await;
        }
        write_file(root, "notes/deep.md", "# 深\n内容").await;

        let legacy = sample_base_files_with_budget(root, 4, 64, 200).await;
        for goal in ["", "   ", "？！"] {
            let outcome = sample_goal_aware_with_budget(root, goal, 4, 64, 200).await;
            assert_eq!(outcome.samples, legacy, "goal={goal:?} must not change the legacy result");
            assert!(outcome.disclosure.is_none(), "no goal → no disclosure (old snapshot shape)");
        }
    }

    /// 大文件里的相关事实不因落在中段而丢失（t2 摘录策略）：超单文件上限
    /// 的文件按词条命中定位相关段落选摘，不再固定取文件头；未超上限的文件
    /// （>8KB 但 <24KB）整篇交付。
    #[tokio::test]
    async fn goal_sampling_keeps_relevant_facts_from_the_middle_of_a_large_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        let filler = "这是一段与目标无关的填充内容，用于把关键段落推到大文件中段。\n\n";
        let head = "# 报表导出说明\n\n开头噪声标记：本节只讲导出按钮的位置。\n\n";
        let middle = "## 聚类分析要点\n\n关键事实标记：K-means 的簇数 k 需要预先指定。\n\n";
        let mut big = String::from(head);
        for _ in 0..600 {
            big.push_str(filler);
        }
        big.push_str(middle);
        for _ in 0..600 {
            big.push_str(filler);
        }
        assert!(big.len() > GOAL_PER_FILE, "fixture must exceed the per-file budget");
        write_file(root, "notes/大文件.md", &big).await;
        // >8KB 但 <24KB：整篇交付（旧 8KB 上限会截掉的事实现在也在）。
        let mut medium = String::from("# 聚类分析速览\n\n");
        for _ in 0..120 {
            medium.push_str(filler);
        }
        medium.push_str("中段标记：层次聚类的合并顺序由距离度量决定。\n");
        asset_medium_size(&medium);
        write_file(root, "notes/中等文件.md", &medium).await;

        let outcome = sample_goal_aware_with_budget(
            root,
            "聚类分析",
            GOAL_MAX_FILES,
            GOAL_PER_FILE,
            GOAL_TOTAL,
        )
        .await;
        let big_excerpt = outcome
            .samples
            .iter()
            .find(|(path, _)| path == "notes/大文件.md")
            .map(|(_, excerpt)| excerpt.clone())
            .expect("large relevant file sampled");
        assert!(
            big_excerpt.contains("K-means 的簇数 k 需要预先指定"),
            "mid-file fact must survive: {}",
            big_excerpt.chars().take(200).collect::<String>()
        );
        assert!(big_excerpt.contains("聚类分析要点"), "the hit paragraph's heading rides along");
        assert!(
            !big_excerpt.contains("开头噪声标记"),
            "hit-less head block must be dropped, not prefixed"
        );
        assert!(big_excerpt.len() <= GOAL_PER_FILE, "excerpt stays inside the budget");

        let medium_excerpt = outcome
            .samples
            .iter()
            .find(|(path, _)| path == "notes/中等文件.md")
            .map(|(_, excerpt)| excerpt.clone())
            .expect("medium file sampled");
        assert_eq!(medium_excerpt, medium, "under the cap the whole document is delivered");
        assert!(medium_excerpt.contains("中段标记"));
    }

    /// `medium` fixture 必须落在 8KB（旧上限）与 24KB（新上限）之间，
    /// 否则这条断言就退化成"小文件整篇交付"。
    fn asset_medium_size(medium: &str) {
        assert!(
            medium.len() > 8 * 1024 && medium.len() < GOAL_PER_FILE,
            "fixture size {} outside the 8KB..24KB window",
            medium.len()
        );
    }

    /// 目标与库无交集（库里有文件、但一个词条都不命中）：生成不能失败——
    /// 退回默认字母序采样，披露如实标注 fallback 而非冒充"按目标采样"。
    #[tokio::test]
    async fn goal_sampling_falls_back_when_the_goal_matches_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        write_file(root, "a.md", "# 期权入门\n认购与认沽的基本概念。").await;
        write_file(root, "b.md", "# 保证金\n维持保证金与追保流程。").await;

        let outcome =
            sample_goal_aware_with_budget(root, "零基础学 Python 数据分析", GOAL_MAX_FILES, GOAL_PER_FILE, GOAL_TOTAL)
                .await;
        assert_eq!(
            outcome.samples.iter().map(|(path, _)| path.as_str()).collect::<Vec<_>>(),
            vec!["a.md", "b.md"],
            "fallback keeps the legacy alphabetical sample"
        );
        let disclosure = outcome.disclosure.expect("fallback still discloses");
        assert!(disclosure.goal_fallback);
        assert_eq!(disclosure.total_files, 2);
        assert_eq!(disclosure.sampled_files, 2);
        assert!(disclosure.low_score_files.is_empty(), "the fallback samples the whole corpus");
        let line = disclosure.brief_line();
        assert!(line.contains("已按默认顺序采样 2/2 个文件"), "{line}");
        assert!(!line.contains("已按目标采样"), "fallback must not claim goal-aware selection");
    }

    /// 票面验收：混杂库（2000 文件：收据/会议记录/剪藏 + 目标相关文档）建课
    /// ——目标相关文件 100% 入样、高噪声类别不入样、披露可读。
    #[tokio::test]
    async fn goal_sampling_on_a_mixed_two_thousand_file_library() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        for (category, count) in [("收据", 640), ("会议记录", 660), ("剪藏", 660)] {
            for index in 0..count {
                write_file(
                    root,
                    &format!("{category}/{index:04}.md"),
                    &format!("# {category} {index:04}\n本文件是该类别的日常记录，与课程目标无关。"),
                )
                .await;
            }
        }
        let mut relevant = Vec::new();
        for index in 0..GOAL_MAX_FILES {
            let path = format!("笔记/python-数据分析-{index:02}.md");
            write_file(
                root,
                &path,
                &format!("# Python 数据分析 第 {index} 讲\n用 pandas 读取数据、清洗并做分组聚合。"),
            )
            .await;
            relevant.push(path);
        }

        let outcome = sample_goal_aware_with_budget(
            root,
            "零基础学 Python 数据分析",
            GOAL_MAX_FILES,
            GOAL_PER_FILE,
            GOAL_TOTAL,
        )
        .await;
        let paths: Vec<&str> = outcome.samples.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(paths.len(), GOAL_MAX_FILES, "文件配额生效");
        for path in &relevant {
            assert!(paths.contains(&path.as_str()), "目标相关文件 100% 入样，缺 {path}");
        }
        assert!(
            paths.iter().all(|path| path.starts_with("笔记/")),
            "高噪声类别不入样: {:?}",
            paths.iter().filter(|path| !path.starts_with("笔记/")).collect::<Vec<_>>()
        );
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted, "清单顺序仍是字典序");
        let disclosure = outcome.disclosure.expect("disclosed");
        assert!(!disclosure.goal_fallback);
        assert_eq!(disclosure.total_files, 2000);
        assert_eq!(disclosure.sampled_files, GOAL_MAX_FILES);
        assert_eq!(disclosure.coherence_files, 0, "相关文件占满额度，连贯补入无余量");
        assert_eq!(disclosure.low_score_files.len(), 2000 - GOAL_MAX_FILES);
        assert_eq!(disclosure.tree.len(), 2000, "完整文件树（含目录结构）");
        assert!(
            disclosure.brief_line().contains("已按目标采样 40/2000 个文件"),
            "{}",
            disclosure.brief_line()
        );
    }

    // ── 多知识库（t4）：预算分摊 + 路径命名空间 + 逐库披露 ─────────────────

    /// 测试库：库名 + 内容根（TempDir 活到用例结束）。
    struct TestBase {
        kb_id: String,
        name: String,
        dir: tempfile::TempDir,
    }

    impl TestBase {
        fn new(kb_id: &str, name: &str) -> Self {
            Self {
                kb_id: kb_id.to_owned(),
                name: name.to_owned(),
                dir: tempfile::TempDir::new().unwrap(),
            }
        }

        fn root(&self) -> &Path {
            self.dir.path()
        }

        fn write(&self, rel: &str, content: &str) {
            let path = self.dir.path().join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, content).unwrap();
        }
    }

    fn inputs<'a>(bases: &'a [&'a TestBase]) -> Vec<SampleBaseInput<'a>> {
        bases
            .iter()
            .map(|base| SampleBaseInput {
                kb_id: base.kb_id.as_str(),
                name: base.name.as_str(),
                root: base.root(),
            })
            .collect()
    }

    const KB_A: &str = "0190f5fe-7c00-7a00-8abc-00000000000a";
    const KB_B: &str = "0190f5fe-7c00-7a00-8abc-00000000000b";

    /// 预算按库等分（40 文件 / 2 库 = 每库 20），路径带 `{库名}/rel` 命名
    /// 空间，合并清单仍是全局字典序，披露逐库给出归属与计数。
    #[tokio::test]
    async fn multi_base_sampling_splits_the_quota_and_namespaces_paths() {
        let first = TestBase::new(KB_A, "笔记库");
        let second = TestBase::new(KB_B, "课程库");
        for index in 0..30 {
            first.write(&format!("a/{index:02}.md"), &format!("# 甲 {index}\n聚类分析要点 {index}"));
            second.write(&format!("b/{index:02}.md"), &format!("# 乙 {index}\n回归分析要点 {index}"));
        }

        let bases = [&first, &second];
        let outcome = sample_bases_with_budget(
            &inputs(&bases),
            Some("聚类分析"),
            GOAL_MAX_FILES,
            GOAL_PER_FILE,
            GOAL_TOTAL,
        )
        .await;

        let paths: Vec<&str> = outcome.samples.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(paths.len(), GOAL_MAX_FILES, "总量预算不变：{paths:?}");
        assert!(
            paths.iter().all(|path| path.starts_with("笔记库/") || path.starts_with("课程库/")),
            "跨库路径带来源命名空间: {paths:?}"
        );
        let first_count = paths.iter().filter(|path| path.starts_with("笔记库/")).count();
        let second_count = paths.iter().filter(|path| path.starts_with("课程库/")).count();
        assert_eq!((first_count, second_count), (20, 20), "等分预算");
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted, "合并清单仍是字典序（不倒退纪律）");

        let disclosure = outcome.disclosure.expect("多库路径恒有披露");
        assert_eq!(disclosure.total_files, 60);
        assert_eq!(disclosure.sampled_files, 40);
        assert_eq!(disclosure.bases.len(), 2, "逐库归属");
        let names: Vec<(&str, &str, usize)> = disclosure
            .bases
            .iter()
            .map(|base| (base.name.as_str(), base.namespace.as_str(), base.sampled_files))
            .collect();
        assert_eq!(
            names,
            vec![("笔记库", "笔记库", 20), ("课程库", "课程库", 20)],
            "库序按库名字典序（确定性，与提交顺序无关）"
        );
        assert!(disclosure.tree.iter().all(|path| path.contains('/')), "{:?}", disclosure.tree);
    }

    /// 小库用不满配额时，余量被后面的库（前向借用）或打满配额的库（末尾回
    /// 收）用掉——总量预算不因一个小库缩水；每库保底常量在现行上限内不约束
    /// （等分已高于保底）。
    #[tokio::test]
    async fn multi_base_sampling_borrows_unused_quota() {
        for (small_name, big_name) in [("A 小库", "B 大库"), ("B 小库", "A 大库")] {
            let small = TestBase::new(KB_A, small_name);
            let big = TestBase::new(KB_B, big_name);
            for index in 0..3 {
                small.write(&format!("s{index}.md"), &format!("# 小 {index}\n内容"));
            }
            for index in 0..30 {
                big.write(&format!("b{index:02}.md"), &format!("# 大 {index}\n内容"));
            }

            let bases = [&small, &big];
            let outcome = sample_bases_with_budget(&inputs(&bases), None, 20, GOAL_PER_FILE, 20 * GOAL_PER_FILE)
                .await;
            let paths: Vec<&str> = outcome.samples.iter().map(|(path, _)| path.as_str()).collect();
            assert_eq!(
                paths.len(),
                20,
                "小库 3 + 大库 17：余量被借用/回收（{small_name} / {big_name}）"
            );
            let small_ns = format!("{small_name}/");
            assert_eq!(
                paths.iter().filter(|path| path.starts_with(&small_ns)).count(),
                3
            );

            let disclosure = outcome.disclosure.expect("无目标的多库路径也要披露（承载库归属）");
            assert!(disclosure.goal.is_none(), "无目标：不冒充按目标采样");
            let mut counts: Vec<usize> = disclosure.bases.iter().map(|base| base.sampled_files).collect();
            counts.sort_unstable();
            assert_eq!(counts, vec![3, 17]);
        }
        assert_eq!(MULTI_KB_PER_BASE_MIN_FILES, 4);
        assert_eq!(MULTI_KB_PER_BASE_MIN_BYTES, 16 * 1024);
    }

    /// 库名重名 / 含路径分隔符：命名空间确定性消歧（同名第二个加 `~2`，
    /// 分隔符替换为 `_`），路径仍能唯一归属到库。
    #[tokio::test]
    async fn multi_base_namespaces_disambiguate_duplicate_names() {
        let first = TestBase::new(KB_A, "金融/笔记");
        let second = TestBase::new(KB_B, "金融/笔记");
        first.write("README-not.md", "# 甲\n聚类要点");
        second.write("README-not.md", "# 乙\n聚类要点");

        let bases = [&first, &second];
        let outcome = sample_bases_with_budget(&inputs(&bases), None, GOAL_MAX_FILES, GOAL_PER_FILE, GOAL_TOTAL)
            .await;
        let paths: Vec<&str> = outcome.samples.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(paths, vec!["金融_笔记/README-not.md", "金融_笔记~2/README-not.md"]);
        let disclosure = outcome.disclosure.unwrap();
        assert_eq!(
            disclosure.bases.iter().map(|base| base.namespace.as_str()).collect::<Vec<_>>(),
            vec!["金融_笔记", "金融_笔记~2"]
        );
        assert_eq!(disclosure.bases[0].kb_id, KB_A, "消歧后归属仍确定");
    }

    /// 单库零回归：路径不命名空间化，披露形状与 t2 逐字同源（无逐库条目）。
    #[tokio::test]
    async fn single_base_sampling_stays_unprefixed() {
        let only = TestBase::new(KB_A, "笔记库");
        only.write("docs/聚类.md", "# 聚类\nK-means 的簇数 k 需要预先指定。");

        let bases = [&only];
        let multi = sample_bases_with_budget(
            &inputs(&bases),
            Some("聚类"),
            GOAL_MAX_FILES,
            GOAL_PER_FILE,
            GOAL_TOTAL,
        )
        .await;
        let single = sample_goal_aware_with_budget(
            only.root(),
            "聚类",
            GOAL_MAX_FILES,
            GOAL_PER_FILE,
            GOAL_TOTAL,
        )
        .await;
        assert_eq!(multi.samples, single.samples, "单库行为与 t2 逐字一致");
        assert_eq!(multi.disclosure, single.disclosure);
        assert!(
            multi.disclosure.unwrap().bases.is_empty(),
            "单库不产逐库条目（旧披露形状）"
        );
    }

    /// 一库为空（无 markdown）：按库跳过、不阻断其他库，披露如实标注 0；
    /// 全部库为空时样本为空（由调用方报错）。
    #[tokio::test]
    async fn multi_base_skips_an_empty_base() {
        let empty = TestBase::new(KB_A, "空库");
        empty.write("notes.txt", "not markdown");
        let filled = TestBase::new(KB_B, "有库");
        filled.write("a.md", "# 甲\n内容");

        let bases = [&empty, &filled];
        let outcome = sample_bases_with_budget(&inputs(&bases), None, 10, 1024, 10 * 1024).await;
        assert_eq!(
            outcome.samples.iter().map(|(path, _)| path.as_str()).collect::<Vec<_>>(),
            vec!["有库/a.md"]
        );
        let disclosure = outcome.disclosure.unwrap();
        let empty_entry = disclosure.bases.iter().find(|base| base.name == "空库").unwrap();
        assert_eq!((empty_entry.total_files, empty_entry.sampled_files), (0, 0));

        let all_empty = [&empty];
        let outcome = sample_bases_with_budget(&inputs(&all_empty), None, 10, 1024, 10 * 1024).await;
        assert!(outcome.samples.is_empty(), "全部为空 → 空样本（调用方报错）");
    }

    /// 目标只命中一个库：另一个库如实标注逐库 fallback，顶层 `goal_fallback`
    /// 仅当所有库都零命中时才为真（不冒充"按目标采样"）。
    #[tokio::test]
    async fn multi_base_goal_fallback_is_per_base_and_honest() {
        let matching = TestBase::new(KB_A, "甲库");
        let unrelated = TestBase::new(KB_B, "乙库");
        matching.write("a.md", "# 聚类分析\nK-means 的簇数 k 需要预先指定。");
        unrelated.write("b.md", "# 期权入门\n认购与认沽的基本概念。");

        let bases = [&matching, &unrelated];
        let outcome = sample_bases_with_budget(
            &inputs(&bases),
            Some("聚类分析"),
            GOAL_MAX_FILES,
            GOAL_PER_FILE,
            GOAL_TOTAL,
        )
        .await;
        let disclosure = outcome.disclosure.unwrap();
        assert!(!disclosure.goal_fallback, "有一个库命中即非全局回退");
        let by_name = |name: &str| disclosure.bases.iter().find(|base| base.name == name).unwrap();
        assert!(!by_name("甲库").goal_fallback);
        assert!(by_name("乙库").goal_fallback, "零命中的库如实标注");
        let line = disclosure.brief_line();
        assert!(
            line.contains("已按目标采样") && line.contains("乙库") && line.contains("未命中目标词条"),
            "披露点名回退的库: {line}"
        );

        // 目标与所有库都无交集：全局回退为真。
        let outcome = sample_bases_with_budget(
            &inputs(&bases),
            Some("零基础学油画"),
            GOAL_MAX_FILES,
            GOAL_PER_FILE,
            GOAL_TOTAL,
        )
        .await;
        let disclosure = outcome.disclosure.unwrap();
        assert!(disclosure.goal_fallback, "全部退回默认顺序");
        assert!(disclosure.brief_line().contains("已按默认顺序采样"), "{}", disclosure.brief_line());
    }
}
