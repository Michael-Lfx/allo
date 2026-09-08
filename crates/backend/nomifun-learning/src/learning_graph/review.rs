//! 发布前的独立 LLM 终审（learnhub「检察官」子代理的轻量等价物）。
//!
//! 确定性审计覆盖不了语义/教学层面的问题（切分不当、前置关系画反、覆盖
//! 缺口、难度断崖）——终审用一次独立的单次模型调用、隔离的上下文（只有
//! 全图清单与范围参考，不带任何先前结论）从对抗视角补一次审查。
//!
//! 门性（2026-09 决策）：终审 findings 一律 warning 级咨询意见——第一次
//! `lg_finish` 过了确定性门禁后被弹回一次（findings 即修复输入），再次
//! finish 不再运行终审、直接发布；它永远不单独拦截发布，danger 硬门仍只
//! 属确定性审计。

use serde::{Deserialize, Serialize};

use crate::completer::LearningCompleter;

use super::{LearningGraphData, draft::DraftGraph};

/// 一次终审最多采纳的条数：超过说明图有系统性问题，条目只会互相稀释。
const MAX_REVIEW_FINDINGS: usize = 8;

/// 一条终审意见（咨询级：severity 只会是 warning / info）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewFinding {
    pub severity: String,
    pub message: String,
}

/// 终审是否值得跑：草稿已审过（上次 finish 弹回）或为空图时跳过。
pub(crate) fn needs_review(draft: &DraftGraph) -> bool {
    !draft.reviewed && !draft.graph.nodes.is_empty()
}

/// 运行终审：独立的单次调用，宽松解析 JSON 数组，任何失败都视为「无意见」
/// ——终审是增强层，不允许它的故障阻断发布路径。
pub(crate) async fn run_final_review(
    completer: &dyn LearningCompleter,
    draft: &DraftGraph,
) -> Vec<ReviewFinding> {
    let user = format!(
        "【学习目标】\n{}\n\n【范围参考】\n{}\n\n【全图清单】每行一个单元：名称 [分钟] <- 前置\n{}",
        draft.topic.trim(),
        draft
            .scope_reference()
            .unwrap_or_else(|| "（范围分析不可用，以下线覆盖为准）".into()),
        draft.compact_dump(),
    );
    let raw = completer
        .complete(None, REVIEW_SYSTEM, &user, REVIEW_MAX_TOKENS)
        .await;
    match raw {
        Ok(raw) => parse_findings(&raw),
        Err(_) => Vec::new(),
    }
}

/// 终审 system 提示词：对抗审查员视角 + 严格的输出契约。
const REVIEW_SYSTEM: &str = r#"你是一名学习图的对抗审查员（终审）。下面是一张已经通过全部确定性结构检查（连通性/DAG/容量/覆盖门）的学习图，你要从语义与教学角度找出确定性检查覆盖不了的问题，例如：
- 单元切分不当：过大（一个单元学不完）、过碎（5 分钟以下的碎片）、或不是动作句（"概率基础"这类名词不是可学习的会话）；
- 前置关系语义错误：方向画反、缺关键铺垫（学 A 之前其实必须先会 B，但图上没有这条边）；
- 覆盖缺口：学习目标/范围清单里的大块概念没有被真正落实为单元；
- 难度曲线突兀：相连单元之间缺中间层。
只报告你确定存在的问题，每条给出可执行的行动（为谁补什么边/拆哪个单元/补哪个主题）；不要泛泛的改进建议，不要复述图的内容。没有问题就返回空数组。
只输出一个 JSON 数组，形如 [{"severity":"warning","message":"…"}]，severity 只能是 "warning" 或 "info"；不要输出数组以外的任何内容。"#;

const REVIEW_MAX_TOKENS: u32 = 2048;

/// 宽松解析：取第一个 `[` 到最后一个 `]` 之间的片段按 JSON 数组解析；
/// 每条过滤空消息、severity 归一为 warning/info、截断到上限。任何解析
/// 失败返回空（视为「终审无意见」）。
fn parse_findings(raw: &str) -> Vec<ReviewFinding> {
    let Some(start) = raw.find('[') else {
        return Vec::new();
    };
    let Some(end) = raw.rfind(']') else {
        return Vec::new();
    };
    if start >= end {
        return Vec::new();
    }
    let parsed: Vec<ReviewFinding> = match serde_json::from_str(&raw[start..=end]) {
        Ok(findings) => findings,
        Err(_) => return Vec::new(),
    };
    parsed
        .into_iter()
        .filter(|finding| !finding.message.trim().is_empty())
        .map(|finding| ReviewFinding {
            severity: if finding.severity == "info" {
                "info".into()
            } else {
                "warning".into()
            },
            message: finding.message.trim().to_owned(),
        })
        .take(MAX_REVIEW_FINDINGS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 宽松解析：标准数组、带围栏/前后噪声的数组、非法 JSON 与空消息。
    #[test]
    fn parse_findings_tolerates_noise_and_filters_junk() {
        let parsed = parse_findings(
            r#"前导噪声
[
  {"severity": "warning", "message": "单元「导数基础」过大，应拆分为定义与运算两单元"},
  {"severity": "danger", "message": "缺关键铺垫：学泰勒展开前应先会高阶导数"},
  {"severity": "info", "message": "  "},
  {"severity": "other", "message": "第三个入口单元语义重复"}
]
尾部噪声"#,
        );
        assert_eq!(parsed.len(), 3, "{parsed:?}");
        assert_eq!(parsed[0].severity, "warning");
        // danger 一律降级为 warning（终审不构成硬门）。
        assert_eq!(parsed[1].severity, "warning");
        assert_eq!(parsed[2].severity, "warning");
        assert!(parsed.iter().all(|f| !f.message.trim().is_empty()));

        assert!(parse_findings("不是 JSON 的回复").is_empty());
        assert!(parse_findings("[]").is_empty());
        assert!(parse_findings("{\"message\": \"不是数组\"}").is_empty());
    }
}
