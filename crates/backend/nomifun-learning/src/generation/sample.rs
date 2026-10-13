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
    /// 完整文件树：全库候选文件的相对路径（字典序，目录结构保留）。
    #[serde(default)]
    pub tree: Vec<String>,
    /// 因低于相关度阈值未入样的文件路径（字典序）。
    #[serde(default)]
    pub low_score_files: Vec<String>,
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
    let terms = term_slices(goal);
    if terms.is_empty() || max_files == 0 {
        // 无目标 / 目标全是标点：回退旧行为（字母序 + 文件头前缀），不披露。
        return SampleOutcome::legacy(sample_base_files_blocking(
            root,
            max_files,
            max_per_file,
            max_total,
        ));
    }
    let candidates = candidate_files(root);
    if candidates.is_empty() {
        return SampleOutcome::legacy(Vec::new());
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
        let samples = sample_base_files_blocking(root, max_files, max_per_file, max_total);
        let sampled: HashSet<&str> = samples.iter().map(|(path, _)| path.as_str()).collect();
        let disclosure = SampleDisclosure {
            goal: Some(goal.trim().to_owned()),
            total_files: candidates.len(),
            sampled_files: samples.len(),
            coherence_files: 0,
            goal_fallback: true,
            low_score_files: candidates
                .iter()
                .filter(|path| !sampled.contains(path.as_str()))
                .cloned()
                .collect(),
            tree: candidates,
        };
        return SampleOutcome { samples, disclosure: Some(disclosure) };
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
        .collect();
    let disclosure = SampleDisclosure {
        goal: Some(goal.trim().to_owned()),
        total_files: candidates.len(),
        sampled_files: samples.len(),
        coherence_files,
        goal_fallback: false,
        tree: candidates,
        low_score_files,
    };
    SampleOutcome { samples, disclosure: Some(disclosure) }
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
fn goal_excerpt(content: &str, terms: &[String], budget: usize) -> String {
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
    let rels = candidate_files(root);
    let mut samples = Vec::new();
    let mut total = 0usize;
    for rel in rels.into_iter().take(max_files) {
        if total >= max_total {
            break;
        }
        let budget = max_per_file.min(max_total - total);
        let Some(excerpt) = read_prefix_lossy(&root.join(&rel), budget) else {
            continue;
        };
        if excerpt.trim().is_empty() {
            continue;
        }
        total += excerpt.len();
        samples.push((rel, excerpt));
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


/// Sample the knowledge base's markdown documents for course generation.
/// Shared by the synchronous pipeline and the background job runner so the
/// snapshot stored in `learning_course_jobs` has the same shape everywhere.
///
/// 目标感知（t2）：带目标时按目标相关度选入 + 连贯预算补相邻，并返回披露
/// 快照；无目标（t1 的旧客户端路径）时行为与旧采样逐字一致、无披露。
pub(crate) async fn sample_base_files(
    knowledge: &KnowledgeService,
    knowledge_base_id: &str,
    goal: Option<&str>,
) -> Result<SampleOutcome, AppError> {
    // A local folder's public `root_path` is its read-only source location.
    // Course generation samples the app-managed Markdown projection so it
    // sees converted documents while the source remains available.
    let content_root = knowledge
        .content_root_for_base(knowledge_base_id)
        .await?;
    if !content_root.is_dir() {
        return Err(AppError::BadRequest(
            "selected knowledge base content directory does not exist".into(),
        ));
    }
    let goal = goal.map(str::trim).filter(|goal| !goal.is_empty());
    let outcome = match goal {
        // 目标词条化为空（纯标点等）时按无目标处理：预算与行为都回到旧路径。
        Some(goal) if !term_slices(goal).is_empty() => {
            sample_goal_aware_with_budget(
                &content_root,
                goal,
                GOAL_SAMPLE_MAX_FILES,
                GOAL_SAMPLE_MAX_PER_FILE,
                GOAL_SAMPLE_MAX_TOTAL,
            )
            .await
        }
        _ => SampleOutcome::legacy(
            sample_base_files_with_budget(
                &content_root,
                LEARNING_SAMPLE_MAX_FILES,
                LEARNING_SAMPLE_MAX_PER_FILE,
                LEARNING_SAMPLE_MAX_TOTAL,
            )
            .await,
        ),
    };
    if outcome.samples.is_empty() {
        return Err(AppError::BadRequest(
            "knowledge base has no markdown documents to generate a course from".into(),
        ));
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
}
