//! 覆盖对账的纯函数层(t2):把 t1 的候选文件拆成 heading/paragraph/path 级
//! topic,再按章节 `source_anchors` 与显式跳过清单对账出状态台账。不编造
//! 标题——headingless 文件用段落序号定位,仍无法定位才退 path 级
//! (`anchor_uncertain` + warning)。文件只是锚载体,不是覆盖单位。

use super::*;

/// 一个 topic 的拆分种子(定位锚 + 展示标签;状态由对账决定)。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TopicSeed {
    pub topic_id: String,
    pub label: String,
    pub paragraph: Option<usize>,
    pub heading: bool,
    pub anchor_uncertain: bool,
}

/// 待对账的候选文件(role 来自 t1;seeds 由 [`split_file_topics`] 产出)。
#[derive(Debug, Clone)]
pub(crate) struct TopicFile {
    pub rel_path: String,
    pub kb_id: String,
    pub role: KbTriageRole,
    pub seeds: Vec<TopicSeed>,
}

/// 模型显式跳过的 topic(与目标无关的段落;必须带理由)。
#[derive(Debug, Clone)]
pub(crate) struct SkippedTopic {
    pub rel_path: String,
    pub heading: String,
    pub reason: String,
}

/// 按定位顺序拆文件:heading(inventory 的 H1-H3)→ 段落序号(headingless,
/// 需要正文)→ path 级单 topic(`anchor_uncertain` + warning)。
pub(crate) fn split_file_topics(
    rel_path: &str,
    headings: &[String],
    content: Option<&str>,
    warnings: &mut Vec<String>,
) -> Vec<TopicSeed> {
    if !headings.is_empty() {
        return headings
            .iter()
            .enumerate()
            .map(|(index, heading)| TopicSeed {
                topic_id: format!("{rel_path}#h{index}"),
                label: heading.trim().to_owned(),
                paragraph: None,
                heading: true,
                anchor_uncertain: false,
            })
            .collect();
    }
    if let Some(content) = content {
        let paragraphs: Vec<&str> = content
            .split("\n\n")
            .map(str::trim)
            .filter(|paragraph| !paragraph.is_empty())
            .collect();
        if !paragraphs.is_empty() {
            return paragraphs
                .iter()
                .enumerate()
                .map(|(index, _)| TopicSeed {
                    topic_id: format!("{rel_path}#p{index}"),
                    // 位置标签不是编造的标题:只用段落序号定位。
                    label: format!("第 {} 段", index + 1),
                    paragraph: Some(index + 1),
                    heading: false,
                    anchor_uncertain: false,
                })
                .collect();
        }
    }
    warnings.push(format!(
        "文件 {rel_path} 无标题且无法按段落定位,覆盖按 path 级兜底(anchor_uncertain)"
    ));
    vec![TopicSeed {
        topic_id: format!("{rel_path}#path"),
        label: rel_path.rsplit('/').next().unwrap_or(rel_path).to_owned(),
        paragraph: None,
        heading: false,
        anchor_uncertain: true,
    }]
}

/// 锚点标题与 topic 标签的匹配(模型可能略去上下文词):精确相等或互相
/// 包含即算命中。
pub(crate) fn heading_matches(anchor_heading: &str, label: &str) -> bool {
    let anchor_heading = anchor_heading.trim();
    let label = label.trim();
    if anchor_heading.is_empty() || label.is_empty() {
        return false;
    }
    anchor_heading == label || anchor_heading.contains(label) || label.contains(anchor_heading)
}

/// 锚点是否覆盖该 topic(同文件;heading 精确/包含匹配、段落序号相等、
/// path 级锚点覆盖全文件)。
fn anchor_covers_topic(anchor: &SourceAnchor, seed: &TopicSeed) -> bool {
    if anchor.heading_or_anchor.as_deref().is_some_and(|heading| seed.heading && heading_matches(heading, &seed.label)) {
        return true;
    }
    if let (Some(paragraph), None) = (anchor.paragraph, &anchor.heading_or_anchor) {
        if !seed.heading && seed.paragraph == Some(paragraph) {
            return true;
        }
    }
    // path 级锚点(无 heading 无 paragraph):整文件兜底。
    anchor.heading_or_anchor.is_none() && anchor.paragraph.is_none()
}

/// 覆盖对账:每个 topic 必须被某章锚点覆盖、或显式 skipped + 理由、或
/// uncertain 可见;文件状态由 topic 聚合(never 整文件静默排除)。
pub(crate) fn reconcile_coverage(
    files: &[TopicFile],
    chapters: &[PlannedChapter],
    skipped: &[SkippedTopic],
) -> CoverageLedger {
    let mut topics: Vec<CoverageTopic> = Vec::new();
    let mut file_rows: Vec<CoverageFile> = Vec::with_capacity(files.len());

    for file in files {
        let mut covered_count = 0usize;
        let mut skipped_count = 0usize;
        let mut file_chapters: Vec<String> = Vec::new();
        for seed in &file.seeds {
            let mut assigned: Vec<String> = Vec::new();
            let mut topic_anchor = None;
            for chapter in chapters {
                let hit = chapter
                    .source_anchors
                    .iter()
                    .find(|anchor| anchor.rel_path == file.rel_path && anchor_covers_topic(anchor, seed));
                if let Some(anchor) = hit {
                    if !assigned.contains(&chapter.title) {
                        assigned.push(chapter.title.clone());
                    }
                    if topic_anchor.is_none() {
                        topic_anchor = Some(SourceAnchor {
                            kb_id: file.kb_id.clone(),
                            rel_path: file.rel_path.clone(),
                            heading_or_anchor: if seed.heading {
                                Some(seed.label.clone())
                            } else {
                                None
                            },
                            paragraph: seed.paragraph,
                            snippet: anchor.snippet.clone(),
                            anchor_uncertain: seed.anchor_uncertain || anchor.anchor_uncertain,
                        });
                    }
                }
            }
            let (status, reason) = if !assigned.is_empty() {
                covered_count += 1;
                (TopicStatus::Covered, None)
            } else {
                // 显式跳过:模型点名同文件同标题的段落并给理由。
                let explicit = skipped.iter().find(|entry| {
                    entry.rel_path == file.rel_path && heading_matches(&entry.heading, &seed.label)
                });
                match explicit {
                    Some(entry) => {
                        skipped_count += 1;
                        (TopicStatus::Skipped, Some(entry.reason.clone()))
                    }
                    None => (TopicStatus::Uncertain, None),
                }
            };
            for title in &assigned {
                if !file_chapters.contains(title) {
                    file_chapters.push(title.clone());
                }
            }
            topics.push(CoverageTopic {
                topic_id: seed.topic_id.clone(),
                label: seed.label.clone(),
                source_anchors: topic_anchor.into_iter().collect(),
                assigned_chapters: assigned,
                status,
                reason,
            });
        }
        let status = if covered_count > 0 {
            if covered_count == file.seeds.len() {
                FileCoverageStatus::Covered
            } else {
                FileCoverageStatus::Partial
            }
        } else if skipped_count == file.seeds.len() && !file.seeds.is_empty() {
            FileCoverageStatus::Skipped
        } else {
            FileCoverageStatus::Uncertain
        };
        file_rows.push(CoverageFile {
            rel_path: file.rel_path.clone(),
            role: file.role,
            assigned_chapters: file_chapters,
            status,
        });
    }
    CoverageLedger {
        topics,
        files: file_rows,
        excluded: Vec::new(),
        unresolved: Vec::new(),
    }
}
