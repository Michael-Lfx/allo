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
//!
//! m5 收敛梯子对账加固（learnhub #175 三断层 + #175A 模型面）：
//! - **守卫全量枚举**（#175B）：`redraft_guard_violation` 一次收集全部
//!   违约单报告（首错早退会让带守卫原因的原地重试只避开被报处，其余
//!   盲试）；单违约文案与历史逐字同形。
//! - **点名集两面同源**（#175A）：守卫的点名判定对评审问题清单只认各条
//!   `node` 字段（批级意见/advice 文字提到的名字不构成点名）；门/契约等
//!   散文报告退回子串口径（历史行为）。
//! - **评审意见回灌**（#175C）：[`AttemptFail`] 增独立 `review_history`
//!   面——守卫违规/输出违约上抛时随失败携带前轮评审问题清单，经
//!   [`AttemptFail::into_feedback`] 作独立段注入重排；**不并入
//!   `report`**（同指纹早停按 report 逐字比对，评审意见逐轮漂移不得
//!   影响指纹）。

use std::collections::{HashMap, HashSet};

use super::{MAX_BATCH_NODES, ProposedBatch, ProposedNode, ReviewProblem};

/// 一次 attempt 的失败面：`report` 进重试反馈与耗尽消息；`guard` = 机械
/// 比对守卫失败（豁免早停 + 原地重试）；`review_history` = 前轮语义评审
/// 意见（#175C，独立字段不进 `report`——理由见模块头注）。
#[derive(Debug, Clone)]
pub(crate) struct AttemptFail {
    pub report: String,
    pub guard: bool,
    pub review_history: Option<String>,
}

impl AttemptFail {
    /// 生成面失败（解析/形状/语义门/评审/LLM 错误——携带输出细节，早停适用）。
    pub fn generation(report: impl Into<String>) -> Self {
        Self { report: report.into(), guard: false, review_history: None }
    }

    /// 机械守卫失败（固定文案，豁免早停）。
    pub fn guard(report: impl Into<String>) -> Self {
        Self { report: report.into(), guard: true, review_history: None }
    }

    /// 附前轮评审意见（#175C；None/空 = 无意见，反馈零附加段）。
    pub fn with_review_history(mut self, history: Option<String>) -> Self {
        self.review_history = history.filter(|text| !text.trim().is_empty());
        self
    }

    /// 重试反馈（learnhub `into_feedback` 同构）：`report` 为主文；前轮
    /// 评审意见（若有）作**独立段**追加——段不并入 `report`，同指纹早停
    /// 的指纹只取 `report`。段头与重排 prior 参数的注入段同标记
    /// （【前轮评审意见】，smoke 存档单标记可 grep）。
    pub fn into_feedback(self) -> String {
        let mut text = self.report;
        if let Some(history) = &self.review_history {
            text.push_str(
                "\n\n【前轮评审意见（随失败回灌；前轮批已打回未落库，本轮重排后评审会复审）】\n\
                 【反乒乓纪律】已修不重提：前轮已点名、上一稿已按建议修正的地方保持修正，不要回退重写；\
                 已过不翻案：前轮评审已放行的内容不要推翻重改。\n",
            );
            text.push_str(history);
        }
        text
    }
}

/// 评审往返记录的一轮条目（#175C 有状态复审：点名集合 + 结论随 attempt
/// 传递；`round` 从 1 起）。
pub(crate) fn review_history_round(round: usize, report: &str) -> String {
    format!("第 {round} 轮评审打回：\n{report}")
}

/// 评审问题清单的点名集（#175A 机械面）：各条 `node` 字段的非空值——批级
/// 意见（node 空）不点名任何节点；issue/advice 文字里提到的节点名不构成
/// 点名。门/契约等非结构化报告不经此面（守卫对它们退回子串口径）。
pub(crate) fn review_point_names(problems: &[ReviewProblem]) -> HashSet<String> {
    problems
        .iter()
        .map(|problem| problem.node.trim())
        .filter(|node| !node.is_empty())
        .map(str::to_owned)
        .collect()
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
///
/// 点名判定（#175A 两面同源）：`point_names` 在场（评审问题清单的 node
/// 字段集，[`review_point_names`] 装配）= 只认字段——批级意见与 issue/
/// advice 文字提到的节点名不构成点名；缺省（门/契约等散文报告）= 历史子
/// 码口径，报告全文提及算点名。
///
/// 违约**全量收集**（#175B：首错早退会让带守卫原因的原地重试只避开被报
/// 的那处，其余违约盲试）；按上一稿节点出现序枚举（顺序确定性 = 重试反
/// 馈的读者面），单违约文案与历史逐字同形（下游 substring 断言依赖）。
pub(crate) fn redraft_guard_violation(
    previous: &ProposedBatch,
    candidate: &ProposedBatch,
    problems: &str,
    point_names: Option<&HashSet<String>>,
) -> Option<String> {
    let is_named = |title: &str| -> bool {
        let title = title.trim();
        !title.is_empty()
            && match point_names {
                Some(set) => set.contains(title),
                None => problems.contains(title),
            }
    };
    let mut violations: Vec<String> = Vec::new();
    for node in &previous.nodes {
        if is_named(&node.title) {
            continue;
        }
        match candidate.nodes.iter().find(|other| other.title == node.title) {
            None => violations.push(unnamed_edit_message(&node.title, "被移除")),
            Some(redrafted) if redrafted != node => {
                violations.push(unnamed_edit_message(&node.title, "被改动"))
            }
            Some(_) => {}
        }
    }
    match violations.as_slice() {
        [] => None,
        [one] => Some(one.clone()),
        many => Some(many.join("\n")),
    }
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
        assert!(redraft_guard_violation(&previous, &candidate, "问题涉及 甲", None).is_none());
        // 未点名而改乙：guard。
        let candidate = ProposedBatch {
            nodes: vec![node("甲", "p1"), node("乙", "偷偷改了")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let violation = redraft_guard_violation(&previous, &candidate, "问题涉及 甲", None);
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
        assert!(redraft_guard_violation(&previous, &candidate, "问题涉及 甲", None).is_some());
        // 新增节点合法。
        let candidate = ProposedBatch {
            nodes: vec![node("甲", "p1"), node("乙", "p2"), node("丙", "新铺垫")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        assert!(redraft_guard_violation(&previous, &candidate, "问题涉及 甲", None).is_none());
    }

    /// 未点名守卫全量收集（m5 #175B，learnhub 同名测试移植）：多违约逐条
    /// 枚举（单报会让带守卫原因的原地重试只避开被报处，其余盲试）；单违
    /// 约文案与历史逐字同形（下游 substring 断言依赖）；点名集内改动放行。
    #[test]
    fn unnamed_guard_collects_all_violations() {
        let previous = ProposedBatch {
            nodes: vec![node("甲", "p1"), node("乙", "p2"), node("丙", "p3")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let point_names = ["甲".to_owned()].into_iter().collect::<HashSet<_>>();

        // 甲被改（点名，放行）+ 乙被改 + 丙被移除：两处违约一次报全。
        let candidate = ProposedBatch {
            nodes: vec![node("甲", "改了的 p1"), node("乙", "被偷改的 p2")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let err = redraft_guard_violation(&previous, &candidate, "问题涉及 甲", Some(&point_names))
            .expect("两处违约须报全");
        assert!(err.contains("「乙」被改动"), "{err}");
        assert!(err.contains("「丙」") && err.contains("被移除"), "{err}");
        assert!(err.contains("只改被点名的地方"), "{err}");

        // 单违约：文案逐字同历史形（下游 substring 断言依赖）。
        let candidate = ProposedBatch {
            nodes: vec![node("甲", "改了的 p1"), node("乙", "被偷改的 p2"), node("丙", "p3")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let err = redraft_guard_violation(&previous, &candidate, "", Some(&point_names))
            .expect("单违约须照报");
        assert_eq!(
            err,
            "- [ng.redraft.scope] 点修越权：未被打回报告点名的节点「乙」被改动。\
             修订纪律是点修不是重写：只改被点名的地方，其余节点逐字保留（含 est/难度/teaches/assumes）；\
             修法确实需要动未点名节点才能成立时，放弃该修法。请重新输出完整批次。"
        );

        // 全部点名 = 放行（点名节点允许改动，含被移除）。
        let all = ["甲", "乙", "丙"].into_iter().map(str::to_owned).collect::<HashSet<_>>();
        assert!(redraft_guard_violation(&previous, &candidate, "", Some(&all)).is_none());
    }

    /// advice 范围（m5 #175A）：批级意见（node 空）的 issue/advice 文字提到
    /// 「乙」不构成点名——点名集只认各条问题的 node 字段；改乙照报违约，
    /// 改点名字段里的甲放行（守卫不误伤守纪律的点修）。
    #[test]
    fn batch_level_opinion_text_does_not_constitute_point_naming() {
        let problems = vec![
            // 批级意见：node 空，文字里提到乙。
            ReviewProblem {
                node: String::new(),
                field: "mints".into(),
                issue: "定义捆扎，涉及乙节点的铸名".into(),
                advice: "拆条；乙节点保持不动".into(),
            },
            // node 字段点名甲。
            ReviewProblem {
                node: "甲".into(),
                field: "teaches".into(),
                issue: "缺前提".into(),
                advice: "补挂 assumes".into(),
            },
        ];
        let point_names = review_point_names(&problems);
        assert!(!point_names.contains("乙"), "批级意见不点名：{point_names:?}");
        let previous = ProposedBatch {
            nodes: vec![node("甲", "p1"), node("乙", "p2")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        // 只改点名字段里的甲：放行。
        let candidate = ProposedBatch {
            nodes: vec![node("甲", "改了"), node("乙", "p2")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        assert!(
            redraft_guard_violation(&previous, &candidate, "", Some(&point_names)).is_none(),
            "守卫不误伤守纪律的点修"
        );
        // 顺批级意见的文字去改乙：违约照报（advice 提到的名字不构成点名）。
        let candidate = ProposedBatch {
            nodes: vec![node("甲", "p1"), node("乙", "被 advice 勾出来的改动")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let violation =
            redraft_guard_violation(&previous, &candidate, "", Some(&point_names));
        assert!(violation.as_ref().is_some_and(|text| text.contains("乙")), "{violation:?}");
    }

    /// 评审意见走独立字段（m5 #175C）：`report`（早停指纹的取材面）不被评
    /// 审意见污染；`into_feedback` 作独立段注入（带反乒乓纪律句）；无意见
    /// = 反馈零附加段。
    #[test]
    fn review_history_rides_beside_not_inside_the_report() {
        let history = review_history_round(1, "- [teaches] 甲: 缺前提（建议：补挂 assumes）");
        let fail = AttemptFail::guard("守卫文案").with_review_history(Some(history.clone()));
        assert_eq!(fail.report, "守卫文案");
        assert_eq!(fail.review_history.as_deref(), Some(history.as_str()));
        let feedback = fail.into_feedback();
        assert!(feedback.starts_with("守卫文案"), "report 是主文：{feedback}");
        assert!(feedback.contains("【前轮评审意见"), "{feedback}");
        assert!(feedback.contains("已修不重提") && feedback.contains("已过不翻案"), "{feedback}");
        assert!(feedback.contains("第 1 轮评审打回") && feedback.contains("补挂 assumes"), "{feedback}");

        // 空白意见 = 无意见（不产生空段）。
        let plain = AttemptFail::generation("契约报告").with_review_history(Some("  ".into()));
        assert!(plain.review_history.is_none());
        assert!(!plain.into_feedback().contains("前轮评审意见"));
    }

    #[test]
    fn review_history_round_labels_the_round() {
        let entry = review_history_round(2, "- [teaches] 甲: 问题（建议：修法）");
        assert!(entry.contains("第 2 轮评审打回"), "{entry}");
        assert!(entry.contains("- [teaches] 甲: 问题"), "{entry}");
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
