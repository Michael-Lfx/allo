//! 生长/结构循环的重试机制（二轮 t1）：同指纹早停 + 「未点名被改」机械
//! 守卫 + 容量定向打回。两个循环（`growth_body` / `structure_gen`）共用
//! 指纹与判别口径；guard 面只存在于生长批（与「点修不是重写」提示词纪律
//! 配对——allo 一轮已有后者，本模块补前者）。
//!
//! 口径（learnhub #60/#85 同构）：
//! - **同指纹早停**：`attempt > 1 && fingerprint == prev && !guard` → 立即
//!   耗尽，不烧剩余轮次——反馈回灌已无法改变结果（连败两次即停，LLM 调用
//!   数 2 非 3）。
//! - **guard 豁免**：机械守卫失败是固定文案（同指纹≠同输出前提不成立），
//!   永不触发早停，且**原地重试恰一次不占轮预算**；二次越权才计轮。
//! - **容量守卫**：重裁出口超容从「整批违规」改判定向重装指令（专用条款
//!   码 `ng.batch.redraft_capacity`）；`ng.batch.size` 起草轮执法单源不变。

use std::collections::HashMap;

use super::{MAX_BATCH_NODES, ProposedBatch, ProposedNode};

/// 一次 attempt 的失败面：`report` 进重试反馈与耗尽消息；`guard` = 机械
/// 比对守卫失败（豁免早停 + 原地重试）。
#[derive(Debug, Clone)]
pub(crate) struct AttemptFail {
    pub report: String,
    pub guard: bool,
}

impl AttemptFail {
    /// 生成面失败（解析/形状/语义门/评审/LLM 错误——携带输出细节，早停适用）。
    pub fn generation(report: impl Into<String>) -> Self {
        Self { report: report.into(), guard: false }
    }

    /// 机械守卫失败（固定文案，豁免早停）。
    pub fn guard(report: impl Into<String>) -> Self {
        Self { report: report.into(), guard: true }
    }
}

/// 失败报告的规范化指纹：逐行 trim、去空行后重排——结构化报告序列化本身
/// 稳定，规范化只防转发性空白漂移击穿比对。
pub(crate) fn normalize_report(report: &str) -> String {
    report
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 早停判据（两循环同款）：本轮失败与上一轮同指纹、且都不是守卫失败。
pub(crate) fn should_stop_early(
    attempt: usize,
    fingerprint: &str,
    prev_fingerprint: Option<&str>,
    guard: bool,
) -> bool {
    attempt > 1 && !guard && prev_fingerprint == Some(fingerprint)
}

/// 「未点名被改」机械守卫：重裁输出与上一稿逐节点 diff——未出现在打回
/// 报告点名清单中的既有节点必须逐字段相等（title/purpose/est/difficulty/
/// teaches/assumes），被移除同理算改动；**新增节点合法**（点修允许补铺垫
/// 节点），被点名的节点不核对。违者返回固定文案（guard 失败）。
pub(crate) fn redraft_guard_violation(
    previous: &ProposedBatch,
    candidate: &ProposedBatch,
    problems: &str,
) -> Option<String> {
    for node in &previous.nodes {
        let named = !node.title.trim().is_empty() && problems.contains(node.title.trim());
        if named {
            continue;
        }
        let Some(redrafted) = candidate
            .nodes
            .iter()
            .find(|other| other.title == node.title)
        else {
            return Some(unnamed_edit_message(&node.title, "被移除"));
        };
        if redrafted != node {
            return Some(unnamed_edit_message(&node.title, "被改动"));
        }
    }
    None
}

fn unnamed_edit_message(title: &str, action: &str) -> String {
    format!(
        "- [ng.redraft.scope] 点修越权：未被打回报告点名的节点「{title}」{action}。\
         修订纪律是点修不是重写：只改被点名的地方，其余节点逐字保留（含 est/难度/teaches/assumes）；\
         修法确实需要动未点名节点才能成立时，放弃该修法。请重新输出完整批次。"
    )
}

/// 容量定向打回的专用条款码（`ng.batch.size` 起草轮执法单源不变——本码
/// 只在重裁出口把「整批违规」改判为定向重装指令）。
pub(crate) const REDRAFT_CAPACITY_CODE: &str = "ng.batch.redraft_capacity";

/// 重裁出口超容的定向指令（learnhub `assert_round_capacity` 同构）：携带
/// 拆分后形态（上一稿节点清单），明示超出留后续批、不要放弃已正确的拆分。
pub(crate) fn redraft_capacity_report(previous: &ProposedBatch) -> String {
    let titles: Vec<String> = previous
        .nodes
        .iter()
        .map(|node: &ProposedNode| node.title.clone())
        .collect();
    format!(
        "- [{REDRAFT_CAPACITY_CODE}] 重裁稿超出批容量 {MAX_BATCH_NODES}。\
         按拆分后的形态重新装箱：本批只保留装得下的部分（至多 {MAX_BATCH_NODES} 个），\
         超出的条目留待后续批次，不要放弃已正确的拆分。\
         建议优先保留的既有节点：{}",
        titles.join("、")
    )
}

/// 重裁失败报告是否为起草轮的批容量形状违约（据此在重裁出口改判定向指
/// 令——形状条款的措辞由 growth_contract 单源，这里只认条款码）。
pub(crate) fn is_batch_size_violation(report: &str) -> bool {
    report.contains("ng.batch.size")
}

/// 调试/测试辅助：上一稿节点标题集合（守卫判据的可见面）。
#[allow(dead_code)]
pub(crate) fn previous_titles(previous: &ProposedBatch) -> HashMap<String, usize> {
    previous
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.title.clone(), index))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning_graph::ConceptRefInput;

    fn node(title: &str, purpose: &str) -> ProposedNode {
        ProposedNode {
            title: title.to_owned(),
            purpose: purpose.to_owned(),
            est: Some(15),
            difficulty: Some(2),
            teaches: vec![ConceptRefInput {
                name: "概念".into(),
                tier: crate::learning_graph::ConceptTier::Know,
            }],
            assumes: Vec::new(),
        }
    }

    #[test]
    fn same_fingerprint_after_first_failure_stops() {
        assert!(!should_stop_early(1, "a", None, false));
        assert!(should_stop_early(2, "a", Some("a"), false));
        assert!(!should_stop_early(2, "a", Some("b"), false));
        // guard 永不早停（同指纹≠同输出前提不成立）。
        assert!(!should_stop_early(2, "a", Some("a"), true));
    }

    #[test]
    fn normalize_report_ignores_whitespace_drift() {
        assert_eq!(normalize_report("a\n\n b \nc"), normalize_report("a\nb\nc"));
    }

    #[test]
    fn guard_fires_only_for_unnamed_edits() {
        let previous = ProposedBatch {
            nodes: vec![node("甲", "p1"), node("乙", "p2")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        // 点名甲、改甲：合法。
        let candidate = ProposedBatch {
            nodes: vec![node("甲", "改了的 p1"), node("乙", "p2")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        assert!(redraft_guard_violation(&previous, &candidate, "问题涉及 甲").is_none());
        // 未点名而改乙：guard。
        let candidate = ProposedBatch {
            nodes: vec![node("甲", "p1"), node("乙", "偷偷改了")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let violation = redraft_guard_violation(&previous, &candidate, "问题涉及 甲");
        assert!(
            violation.as_ref().is_some_and(|text| text.contains("乙")),
            "{violation:?}"
        );
        // 未点名而移除乙：guard。
        let candidate = ProposedBatch {
            nodes: vec![node("甲", "p1")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        assert!(redraft_guard_violation(&previous, &candidate, "问题涉及 甲").is_some());
        // 新增节点合法。
        let candidate = ProposedBatch {
            nodes: vec![node("甲", "p1"), node("乙", "p2"), node("丙", "新铺垫")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        assert!(redraft_guard_violation(&previous, &candidate, "问题涉及 甲").is_none());
    }

    #[test]
    fn capacity_report_carries_the_directive() {
        let report = redraft_capacity_report(&previous_titles_batch());
        assert!(report.contains(REDRAFT_CAPACITY_CODE));
        assert!(report.contains("留待后续批次"));
        assert!(report.contains("不要放弃已正确的拆分"));
    }

    fn previous_titles_batch() -> ProposedBatch {
        ProposedBatch {
            nodes: vec![node("甲", "p"), node("乙", "p")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        }
    }
}
