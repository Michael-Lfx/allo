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

use nomifun_common::AppError;

use super::{MAX_BATCH_NODES, ProposedBatch, ProposedNode, ReviewProblem};

/// 一次 attempt 的失败面：`report` 进重试反馈与耗尽消息；`guard` = 机械
/// 比对守卫失败（豁免早停 + 原地重试）；`review_history` = 前轮语义评审
/// 意见（#175C，独立字段不进 `report`——理由见模块头注）；`transport` =
/// 模型没收到（y02：原错误立即上抛，不写反馈、不进早停、不占轮次）。
#[derive(Debug)]
pub(crate) struct AttemptFail {
    pub report: String,
    pub guard: bool,
    pub review_history: Option<String>,
    pub transport: Option<AppError>,
}

impl AttemptFail {
    /// 生成面失败（解析/形状/语义门/评审/LLM 错误——携带输出细节，早停适用）。
    pub fn generation(report: impl Into<String>) -> Self {
        Self {
            report: report.into(),
            guard: false,
            review_history: None,
            transport: None,
        }
    }

    /// 机械守卫失败（固定文案，豁免早停）。
    pub fn guard(report: impl Into<String>) -> Self {
        Self {
            report: report.into(),
            guard: true,
            review_history: None,
            transport: None,
        }
    }

    /// 传输失败（y02）：原 `AppError` 立即上抛——不写反馈、不进早停、不占
    /// 契约轮次；模型对网络错误无能为力。
    pub fn transport(error: AppError) -> Self {
        Self {
            report: error.to_string(),
            guard: false,
            review_history: None,
            transport: Some(error),
        }
    }

    /// 是否为传输失败（[`Self::transport`] 构造）。
    pub fn is_transport(&self) -> bool {
        self.transport.is_some()
    }

    /// 附前轮评审意见（#175C；None/空 = 无意见，反馈零附加段）。
    pub fn with_review_history(mut self, history: Option<String>) -> Self {
        self.review_history = history.filter(|text| !text.trim().is_empty());
        self
    }

    /// 重试反馈（learnhub `into_feedback` 同构）：`report` 为主文；前轮
    /// 评审意见（若有）作**独立段**追加——段不并入 `report`，同指纹早停
    /// 的指纹只取 `report`。段头与重排 prior 参数的注入段同标记
    /// （【前轮评审意见】，smoke 存档单标记可 grep）；纪律句单源
    /// （[`super::ANTI_PING_PONG_DISCIPLINE`]）。
    pub fn into_feedback(self) -> String {
        let mut text = self.report;
        if let Some(history) = &self.review_history {
            text.push_str("\n\n【前轮评审意见（随失败回灌；前轮批已打回未落库，本轮重排后评审会复审）】\n");
            text.push_str(super::ANTI_PING_PONG_DISCIPLINE);
            text.push('\n');
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
/// teaches/assumes/intent/scope），被移除同理算改动；**新增节点合法**（点修允许补铺垫
/// 节点），被点名的节点不核对。违者返回固定文案（guard 失败）。
///
/// 点名判定（#175A 两面同源）：`point_names` 在场（评审问题清单的 node
/// 字段集，[`review_point_names`] 装配）= 只认字段——批级意见与 issue/
/// advice 文字提到的节点名不构成点名；缺省（门/契约等散文报告）= 历史子
/// 码口径，报告全文提及算点名。调用方契约（w01）：`problems` 必须是触发
/// 本轮重裁的**打回报告**（或钉住它的点名探针），不得传守卫自身的违约文
/// 案——守卫文本提及的标题（被点为越权的节点）不是打回点名，喂进去会把
/// 越权节点错标成「已点名」而免检。
///
/// 违约**全量收集**（#175B：首错早退会让带守卫原因的原地重试只避开被报
/// 的那处，其余违约盲试）；按上一稿节点出现序枚举（顺序确定性 = 重试反
/// 馈的读者面）。文案两臂随 [`super::redraft_splice_enabled`] 开关（u01，
/// 与相邻 [`splice_unnamed_nodes_if_enabled`] 同一开关单源——开关读数在
/// 机制内，调用点不再传布尔）：off = 历史基线形逐字（off 臂回归钉，下游
/// substring 断言依赖）；on = 指令式「从上一稿复制该节点 JSON」出口（改
/// 动臂）并随文案携带上一稿节点 JSON（移除臂——原地重试的「上一稿」已
/// 是缺该节点的候选稿，JSON 必须随文案携带才复制得了）。条款码
/// `ng.redraft.scope` 两臂同在。
pub(crate) fn redraft_guard_violation(
    previous: &ProposedBatch,
    candidate: &ProposedBatch,
    problems: &str,
    point_names: Option<&HashSet<String>>,
) -> Option<String> {
    let direct_copy_wording = super::redraft_splice_enabled();
    let is_named = |title: &str| -> bool {
        point_named(title, problems, point_names)
    };
    let mut violations: Vec<String> = Vec::new();
    for node in &previous.nodes {
        if is_named(&node.title) {
            continue;
        }
        match candidate.nodes.iter().find(|other| other.title == node.title) {
            None => violations.push(unnamed_edit_message(
                &node.title,
                "被移除",
                direct_copy_wording,
                Some(node),
            )),
            Some(redrafted) if redrafted != node => {
                violations.push(unnamed_edit_message(
                    &node.title,
                    "被改动",
                    direct_copy_wording,
                    None,
                ))
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

/// 点名判定单源（#175A；守卫与回填两面同口径——回填豁免面与守卫核对面
/// 必须一致，否则回填会把守卫该报的改动悄悄抹掉）：`point_names` 在场 =
/// 只认字段；缺省 = 散文子串口径。
fn point_named(
    title: &str,
    problems: &str,
    point_names: Option<&HashSet<String>>,
) -> bool {
    let title = title.trim();
    !title.is_empty()
        && match point_names {
            Some(set) => set.contains(title),
            None => problems.contains(title),
        }
}

fn unnamed_edit_message(
    title: &str,
    action: &str,
    direct_copy_wording: bool,
    restore_node: Option<&ProposedNode>,
) -> String {
    if direct_copy_wording {
        // u01 on 臂：指令式——不把节点名写成可被误读的主语（真机病历：
        // 模型把「未被打回报告点名的节点「X」被改动」读成「把 X 恢复」，
        // 反把点名节点回退），给「从上一稿复制 JSON」的可执行出口。
        return match restore_node {
            Some(node) => {
                let node_json = serde_json::to_string(node).unwrap_or_default();
                format!(
                    "- [ng.redraft.scope] 点修越权：未被打回报告点名的节点「{title}」{action}。\
                     该节点不在本轮打回范围内，必须原样保留——把下面的节点 JSON 原样复制回本批 nodes\
                     （任何字段（含标点）都不要改）：{node_json}。请重新输出完整批次。"
                )
            }
            None => format!(
                "- [ng.redraft.scope] 点修越权：以下节点不在本轮打回范围内，必须与上一稿逐字一致\
                 ——直接从上一稿复制该节点 JSON，任何字段（含标点）都不要改：「{title}」\
                 （候选稿把它{action}）。请重新输出完整批次。"
            ),
        };
    }
    format!(
        "- [ng.redraft.scope] 点修越权：未被打回报告点名的节点「{title}」{action}。\
         修订纪律是点修不是重写：只改被点名的地方，其余节点逐字保留（含 est/难度/teaches/assumes）；\
         修法确实需要动未点名节点才能成立时，放弃该修法。请重新输出完整批次。"
    )
}

/// u01 点修回填（纯函数）：上一稿中**不在点名集**、且候选稿里同标题仍在
/// 的节点，用上一稿该节点**整对象回填**——抹掉标点/措辞漂移（真机
/// run-1793951137445：「粗/中/细」→「粗、中、细」的漂移让守卫拦下整批，
/// 原地重试又越权改了别的节点）。候选稿缺失（改名/删除）的未点名节点
/// **不回填**，照旧走守卫（避免把「改名」变成「新旧并存」）；新增节点、
/// 点名节点一律以候选稿为准。返回（回填后的批，实际回填节点数——逐字节
/// 相同的照抄不计入）。
pub(crate) fn splice_unnamed_nodes(
    previous: &ProposedBatch,
    mut candidate: ProposedBatch,
    problems: &str,
    point_names: Option<&HashSet<String>>,
) -> (ProposedBatch, usize) {
    let mut spliced = 0usize;
    for node in &previous.nodes {
        if point_named(&node.title, problems, point_names) {
            continue;
        }
        if let Some(slot) = candidate
            .nodes
            .iter_mut()
            .find(|other| other.title == node.title)
        {
            if *slot != *node {
                *slot = node.clone();
                spliced += 1;
            }
        }
    }
    (candidate, spliced)
}

/// u01 开关面（growth_body 调用点单源）：on = [`splice_unnamed_nodes`]；
/// off = 候选稿原样返回、计数恒零（off 臂 = 基线逐字节——守卫照旧全量
/// 比对）。开关读数在机制内（[`super::redraft_splice_enabled`]，缺省 on）。
pub(crate) fn splice_unnamed_nodes_if_enabled(
    previous: &ProposedBatch,
    candidate: ProposedBatch,
    problems: &str,
    point_names: Option<&HashSet<String>>,
) -> (ProposedBatch, usize) {
    if super::redraft_splice_enabled() {
        splice_unnamed_nodes(previous, candidate, problems, point_names)
    } else {
        (candidate, 0)
    }
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
            intent: String::new(),
            scope: String::new(),
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
    #[serial_test::serial]
    fn guard_fires_only_for_unnamed_edits() {
        // 守卫文案随 REDRAFT_SPLICE 开关（机制内读数）：本测裁点名判定，
        // 基线文案臂——显式 off（进程 env 全局态，serial 互斥）。
        let _baseline = set_switch_env(super::super::REDRAFT_SPLICE_SWITCH_ENV, Some("off"));
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

    /// 声明面两字段自动进守卫（absorb-8 z01）：derived PartialEq 逐字段比对
    /// 把 intent/scope 一并纳保——未点名节点只改 scope 也触发（learnhub
    /// 「未点名条目整对象机械比对自动含两字段」同效）。
    #[test]
    #[serial_test::serial]
    fn guard_covers_declaration_fields_via_derived_equality() {
        let _baseline = set_switch_env(super::super::REDRAFT_SPLICE_SWITCH_ENV, Some("off"));
        let previous = ProposedBatch {
            nodes: vec![node("甲", "p1")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let mut redrafted = node("甲", "p1");
        redrafted.scope = "偷改的教学范围".into();
        let candidate = ProposedBatch {
            nodes: vec![redrafted],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let violation = redraft_guard_violation(&previous, &candidate, "", None);
        assert!(
            violation.as_ref().is_some_and(|text| text.contains("甲")),
            "未点名只改 scope 也须触发守卫：{violation:?}"
        );
    }

    /// 未点名守卫全量收集（m5 #175B，learnhub 同名测试移植）：多违约逐条
    /// 枚举（单报会让带守卫原因的原地重试只避开被报处，其余盲试）；单违
    /// 约文案与历史逐字同形（下游 substring 断言依赖）；点名集内改动放行。
    #[test]
    #[serial_test::serial]
    fn unnamed_guard_collects_all_violations() {
        let _baseline = set_switch_env(super::super::REDRAFT_SPLICE_SWITCH_ENV, Some("off"));
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
    #[serial_test::serial]
    fn batch_level_opinion_text_does_not_constitute_point_naming() {
        let _baseline = set_switch_env(super::super::REDRAFT_SPLICE_SWITCH_ENV, Some("off"));
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
        // 纪律句单源：反馈携带的必须是共享常量的逐字全文（改常量即两处同改）。
        assert!(
            feedback.contains(crate::learning_graph::ANTI_PING_PONG_DISCIPLINE),
            "{feedback}"
        );
        assert!(feedback.contains("第 1 轮评审打回") && feedback.contains("补挂 assumes"), "{feedback}");

        // 空白意见 = 无意见（不产生空段）。
        let plain = AttemptFail::generation("契约报告").with_review_history(Some("  ".into()));
        assert!(plain.review_history.is_none());
        assert!(!plain.into_feedback().contains("前轮评审意见"));
    }

    /// y02：传输失败标记与生成面失败互斥——重试环据此立即上抛原错误。
    #[test]
    fn transport_failure_carries_the_original_error() {
        let fail = AttemptFail::transport(AppError::Timeout(
            "json coach call exceeded 480s".into(),
        ));
        assert!(fail.is_transport());
        assert!(!fail.guard);
        assert!(fail.review_history.is_none());
        assert_eq!(fail.report, "Request timeout: json coach call exceeded 480s");
        assert!(matches!(fail.transport, Some(AppError::Timeout(_))));
        assert!(!AttemptFail::generation("契约报告").is_transport());
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

    /// u01 开关的进程全局态守卫：单源在 [`crate::switches::switch_env_guard`]
    ///（四份脚手架收拢），本面便捷形。
    fn set_switch_env(env_var: &'static str, value: Option<&str>) -> impl Drop {
        crate::switches::switch_env_guard(env_var, value)
    }

    // ── u01：点修回填（NOMI_LEARNING_REDRAFT_SPLICE，开发期缺省 off）────────

    /// u01 主臂：同标题未点名节点的措辞/标点漂移被整对象回填（守卫归零，
    /// 不再消耗原地重试）；计数只数实际被抹平的节点；点名节点、新增节点
    /// 以候选稿为准。
    #[test]
    #[serial_test::serial]
    fn splice_reverts_unnamed_same_title_drift_and_counts_it() {
        // 守卫随开关机制内读数：回填后的守卫校验按基线文案臂——显式 off
        //（进程 env 全局态，serial 互斥；回填本身由 splice_unnamed_nodes
        // 纯函数承载，不读开关）。
        let _baseline = set_switch_env(super::super::REDRAFT_SPLICE_SWITCH_ENV, Some("off"));
        let previous = ProposedBatch {
            nodes: vec![node("甲", "p1"), node("乙", "粗/中/细")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        // 候选稿：甲被点名修改（保留）；乙标点漂移（真机 run-1793951137445
        // 同形：「粗/中/细」→「粗、中、细」+ scope 偷改）；丙新增。
        let mut drifted_b = node("乙", "粗、中、细");
        drifted_b.scope = "偷改的教学范围".into();
        let mut named_edit = node("甲", "p1");
        named_edit.purpose = "被点名的修改".into();
        let candidate = ProposedBatch {
            nodes: vec![named_edit, drifted_b, node("丙", "新铺垫")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let point_names = ["甲".to_owned()].into_iter().collect::<HashSet<_>>();
        let (spliced, count) = splice_unnamed_nodes(&previous, candidate, "", Some(&point_names));
        assert_eq!(count, 1, "只回填漂移的未点名乙：{count}");
        assert_eq!(spliced.nodes[0].purpose, "被点名的修改", "点名节点以候选稿为准");
        assert_eq!(spliced.nodes[1], node("乙", "粗/中/细"), "漂移节点整对象回填");
        assert_eq!(spliced.nodes[2].title, "丙", "新增节点保留");
        assert!(
            redraft_guard_violation(&previous, &spliced, "", Some(&point_names)).is_none(),
            "回填后守卫必须通过——同标题漂移不再消耗原地重试"
        );
    }

    /// u01：改名/删除的未点名节点不回填（候选稿无同标题），照旧走守卫——
    /// 避免「改名」被回填成「新旧并存」。
    #[test]
    #[serial_test::serial]
    fn splice_skips_missing_nodes_and_guard_still_fires() {
        let _baseline = set_switch_env(super::super::REDRAFT_SPLICE_SWITCH_ENV, Some("off"));
        let previous = ProposedBatch {
            nodes: vec![node("乙", "原用途")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let candidate = ProposedBatch {
            nodes: vec![node("乙二", "改名后的用途")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let (spliced, count) = splice_unnamed_nodes(&previous, candidate, "", None);
        assert_eq!(count, 0, "同标题缺失不回填：{count}");
        assert!(spliced.nodes.iter().any(|n| n.title == "乙二"), "改名稿保留");
        let violation = redraft_guard_violation(&previous, &spliced, "", None);
        assert!(
            violation.as_ref().is_some_and(|text| text.contains("乙") && text.contains("被移除")),
            "改名 = 删除，守卫照报：{violation:?}"
        );
    }

    /// u01 开关面：off 臂候选稿逐字节不动、计数恒零（守卫照旧全量比对 =
    /// 基线行为）；显式 off = off 臂（缺省已于 2026-11-06 A/B 后翻 on）；显式 on = 回填。
    #[test]
    #[serial_test::serial]
    fn splice_switch_off_arm_keeps_candidate_untouched() {
        let previous = ProposedBatch {
            nodes: vec![node("乙", "粗/中/细")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let candidate = ProposedBatch {
            nodes: vec![node("乙", "粗、中、细")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        {
            let _off = set_switch_env(super::super::REDRAFT_SPLICE_SWITCH_ENV, Some("off"));
            let (spliced, count) =
                splice_unnamed_nodes_if_enabled(&previous, candidate.clone(), "", None);
            assert_eq!(count, 0, "off 臂零回填：{count}");
            assert_eq!(spliced, candidate, "off 臂候选稿逐字节不动");
        }
        let (spliced, count) = splice_unnamed_nodes_if_enabled(&previous, candidate.clone(), "", None);
        assert_eq!(count, 1, "未设 = 缺省 on（2026-11-06 A/B 达标后翻 on）：{count}");
        assert_ne!(spliced, candidate, "未设 = 漂移被回填（on 臂同形）");
        let _on = set_switch_env(super::super::REDRAFT_SPLICE_SWITCH_ENV, Some("on"));
        let (spliced, count) = splice_unnamed_nodes_if_enabled(&previous, candidate.clone(), "", None);
        assert_eq!(count, 1, "显式 on = 回填：{count}");
        assert_ne!(spliced, candidate, "on 臂漂移被回填");
    }

    /// u01 on 臂守卫文案：指令式、节点名不作主语、给「从上一稿复制」的可
    /// 执行出口；条款码 `ng.redraft.scope` 两臂同在（下游 substring 断言
    /// 依赖）。移除臂嵌上一稿节点 JSON——原地重试的「上一稿」已是缺该节点
    /// 的候选稿，JSON 必须随文案携带才复制得了。
    #[test]
    #[serial_test::serial]
    fn guard_wording_direct_copy_arm_instructs_verbatim_copy() {
        // on 臂文案 = 指令式——显式 on（缺省臂测试住 switches.rs 纯函数面；
        // 进程 env 全局态，serial 互斥）。
        let _on = set_switch_env(super::super::REDRAFT_SPLICE_SWITCH_ENV, Some("on"));
        let previous = ProposedBatch {
            nodes: vec![node("甲", "p1"), node("乙", "乙的原用途"), node("丙", "丙的原用途")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let point_names = ["甲".to_owned()].into_iter().collect::<HashSet<_>>();
        // 改动臂：乙漂移。
        let candidate = ProposedBatch {
            nodes: vec![node("甲", "改了的 p1"), node("乙", "被偷改的用途"), node("丙", "丙的原用途")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let edited = redraft_guard_violation(&previous, &candidate, "", Some(&point_names))
            .expect("on 臂照报改动");
        assert!(edited.contains("ng.redraft.scope"), "{edited}");
        assert!(
            edited.contains("必须与上一稿逐字一致——直接从上一稿复制该节点 JSON，任何字段（含标点）都不要改：「乙」"),
            "指令式文案且节点名不作主语：{edited}"
        );
        assert!(
            !edited.contains("逐字保留（含 est/难度/teaches/assumes）"),
            "基线句不出现在 on 臂：{edited}"
        );
        // 移除臂：丙缺失——文案携带上一稿节点 JSON。
        let candidate = ProposedBatch {
            nodes: vec![node("甲", "改了的 p1"), node("乙", "乙的原用途")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let removed = redraft_guard_violation(&previous, &candidate, "", Some(&point_names))
            .expect("on 臂照报移除");
        assert!(removed.contains("「丙」") && removed.contains("被移除"), "{removed}");
        assert!(
            removed.contains(r#""title":"丙""#) && removed.contains(r#""purpose":"丙的原用途""#),
            "移除文案携带可复制的节点 JSON：{removed}"
        );
    }

    /// u01 off 臂守卫文案逐字回归：`direct_copy_wording=false` 时单违约文案
    /// 与历史逐字同形（下游 substring 断言依赖的基线钉）。
    #[test]
    #[serial_test::serial]
    fn guard_wording_off_arm_is_verbatim_historical() {
        let _baseline = set_switch_env(super::super::REDRAFT_SPLICE_SWITCH_ENV, Some("off"));
        let previous = ProposedBatch {
            nodes: vec![node("甲", "p1"), node("乙", "p2"), node("丙", "p3")],
            mints: Vec::new(),
            new_aliases: Vec::new(),
            note: String::new(),
        };
        let point_names = ["甲".to_owned()].into_iter().collect::<HashSet<_>>();
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
    }
}
