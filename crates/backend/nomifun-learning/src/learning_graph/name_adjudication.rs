//! 名字捆绑批量裁决（y01，learnhub #199 `name_adjudication` 域中立移植）：
//! 机械点名之后的**不可判定面**批量中立裁决。
//!
//! # 三层分工
//!
//! - **L0 硬门（免费）**：纯标点并列符 `、`/`，` 命中 = 直接定案违规
//!   （[`structure_contract::hard_name_mark`]）。
//! - **L1 点名器（免费）**：`与/和/及/半角逗号/斜杠` 命中 = 可疑、不终审
//!   （[`structure_contract::soft_name_marks`]）——开放课程域下任何词表/位置
//!   启发式都不可先验封闭（「求和公式」「调和级数」与「双进程与窗口」双向
//!   反例）；机械层只负责筛查完备。
//! - **L2 批量裁决（付费，稀疏）**：只在「L1 非空 且 所有红条为空」时触发；
//!   一次小调用批量裁全部命中，输出逐名字 `bundle` + 理由。
//!
//! # 容错语义
//!
//! 裁决器不可用（传输/解析/形状违约）→ 调用方把命中降为**提示档**（报告
//! 留痕、不阻断），绝不因裁决器死亡杀运行；同名运行内 memo，跨 attempt
//! 只裁一次。判捆绑 → 红条带 reason 回灌重试；判单一主题术语 → 放行。
//!
//! # 不做
//!
//! 全量名字语义审查（变相评审器）、跨运行持久 verdict 缓存（状态面/陈旧
//! 度不值得）、把位置/产物面/机械门结论带进裁决输入（锚定）。

use std::collections::HashMap;

use nomifun_common::ProviderId;
use serde::Deserialize;
use serde_json::Value;

use super::structure_contract::NameSuspect;
use super::structure_prompts;
use crate::completer::LearningCompleter;

/// 裁决会话系统行（任务与判据全部住提示词资产；这里只固定输出通道基调）。
const ADJUDICATION_SYSTEM: &str =
    "你是术语判定员：严格按任务说明输出一个 JSON 对象，不输出任何其他字符。";

/// 一条名字裁决（learnhub `NameVerdict` 同形）：输入名字逐字照抄。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct NameVerdict {
    /// 输入名字逐字照抄（输出契约按名字去重）。
    pub(crate) name: String,
    /// true = 多个主题捆绑（维持违规）；false = 单一主题术语（摘除）。
    pub(crate) bundle: bool,
    /// 一句话理由（判捆绑时进违规报告）。
    pub(crate) reason: String,
}

/// 一次批量裁决的结果面。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Adjudication {
    /// 裁决成功：逐名字结论（覆盖全部输入名字）。
    Resolved(Vec<NameVerdict>),
    /// 裁决器不可用 → 调用方降级为提示、不阻断。原因只进日志，不进重试
    /// 反馈（不得把裁决器错误写进生成重试反馈）。
    Degraded(String),
}

/// L2 批量裁决器（持 completer/model_override 与**运行内** memo）：同名跨
/// attempt 只裁一次——重试环里同名字反复出现不重复付费。
pub(crate) struct NameAdjudicator<'a> {
    completer: &'a dyn LearningCompleter,
    model_override: Option<(&'a ProviderId, &'a str)>,
    /// 名字 → 结论（memo 键 = 名字；输出契约按名字去重）。
    memo: HashMap<String, NameVerdict>,
}

impl<'a> NameAdjudicator<'a> {
    pub(crate) fn new(
        completer: &'a dyn LearningCompleter,
        model_override: Option<(&'a ProviderId, &'a str)>,
    ) -> Self {
        Self {
            completer,
            model_override,
            memo: HashMap::new(),
        }
    }

    /// 批量裁决给定点名。memo 命中零调用；失败 = [`Adjudication::Degraded`]
    /// （调用方降级为提示，绝不杀运行）。
    pub(crate) async fn adjudicate(&mut self, suspects: &[NameSuspect]) -> Adjudication {
        // 待裁集：memo 未命中且批内未重复（输出契约按名字去重）。
        let mut pending: Vec<&NameSuspect> = Vec::new();
        for suspect in suspects {
            if self.memo.contains_key(&suspect.name)
                || pending.iter().any(|seen| seen.name == suspect.name)
            {
                continue;
            }
            pending.push(suspect);
        }
        if !pending.is_empty() {
            let name_list = render_names(&pending);
            let prompt = match structure_prompts::assemble(
                &structure_prompts::NAME_BUNDLE,
                &[("name_list", name_list.as_str())],
            ) {
                Ok(prompt) => prompt,
                Err(error) => {
                    let reason = format!("裁决 prompt 装配失败:{error}");
                    tracing::warn!(error = %error, "name adjudication prompt assembly failed — degrading to hints");
                    return Adjudication::Degraded(reason);
                }
            };
            let completer = self.completer;
            let raw = completer
                .complete_json(
                    self.model_override.map(|(id, model)| (id.as_str(), model)),
                    ADJUDICATION_SYSTEM,
                    &prompt.text,
                    crate::generation::NAME_ADJUDICATION_MAX_TOKENS,
                    Some(structure_prompts::NAME_BUNDLE.temperature),
                )
                .await;
            let raw = match raw {
                Ok(raw) => raw,
                Err(error) => {
                    let reason = format!("裁决调用失败:{error}");
                    tracing::warn!(
                        error = %error,
                        suspects = pending.len(),
                        "name adjudicator unavailable — suspects degrade to hints (generation continues)"
                    );
                    return Adjudication::Degraded(reason);
                }
            };
            let value = match crate::generation::parse_json_object::<Value>(&raw) {
                Ok(value) => value,
                Err(error) => {
                    let reason = format!("裁决输出解析失败:{error}");
                    tracing::warn!(
                        error = %error,
                        suspects = pending.len(),
                        "name adjudication reply is not a JSON object — degrading to hints"
                    );
                    return Adjudication::Degraded(reason);
                }
            };
            let verdicts = match parse_verdicts(&value, &pending) {
                Ok(verdicts) => verdicts,
                Err(reason) => {
                    tracing::warn!(
                        reason = %reason,
                        suspects = pending.len(),
                        "name adjudication output violated its contract — degrading to hints"
                    );
                    return Adjudication::Degraded(reason);
                }
            };
            for suspect in &pending {
                // 防御面：机械校验保证每名恰一条（:parse_verdicts），这里缺
                // 结论只可能是上游违约——走既有 Degraded 通道降级为提示，
                // 绝不 panic（图模块生产代码零 panic 红线）。
                let Some(verdict) = verdicts.iter().find(|verdict| verdict.name == suspect.name)
                else {
                    let reason = format!("裁决结果缺名字「{}」的结论", suspect.name);
                    tracing::warn!(
                        reason = %reason,
                        suspects = pending.len(),
                        "name adjudication missing a verdict row — degrading to hints"
                    );
                    return Adjudication::Degraded(reason);
                };
                self.memo.insert(suspect.name.clone(), verdict.clone());
            }
        }
        Adjudication::Resolved(
            suspects
                .iter()
                .filter_map(|suspect| self.memo.get(&suspect.name).cloned())
                .collect(),
        )
    }
}

/// 裁决输入的中立渲染：只给名字 + 被点字符，不告知位置、产物面与机械门
/// 结论（免锚定）。
fn render_names(suspects: &[&NameSuspect]) -> String {
    fn mark_label(c: char) -> String {
        match c {
            ',' => "半角逗号".to_owned(),
            '/' => "斜杠".to_owned(),
            other => format!("「{other}」"),
        }
    }
    suspects
        .iter()
        .map(|suspect| format!("- 名字:「{}」\n  被点字符:{}", suspect.name, mark_label(suspect.mark)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 裁决输出机械校验（形状/覆盖/理由）：任何不满足 = 整次裁决不可用（调用
/// 方降级）。每个提交名恰一条、无未提交名、无重复、理由非空。
fn parse_verdicts(
    value: &Value,
    pending: &[&NameSuspect],
) -> Result<Vec<NameVerdict>, String> {
    #[derive(Deserialize)]
    struct Doc {
        verdicts: Vec<NameVerdict>,
    }
    let doc: Doc = serde_json::from_value(value.clone())
        .map_err(|error| format!("裁决输出形状违约:{error}"))?;
    if doc.verdicts.is_empty() {
        return Err("裁决输出 verdicts 为空".to_owned());
    }
    let names: Vec<&str> = pending.iter().map(|suspect| suspect.name.as_str()).collect();
    for verdict in &doc.verdicts {
        if !names.contains(&verdict.name.as_str()) {
            return Err(format!("裁决输出了未提交的名字「{}」", verdict.name));
        }
        if verdict.reason.trim().is_empty() {
            return Err(format!("裁决理由为空「{}」", verdict.name));
        }
    }
    for name in names {
        let hits = doc
            .verdicts
            .iter()
            .filter(|verdict| verdict.name == name)
            .count();
        if hits != 1 {
            return Err(format!("裁决对名字「{name}」输出 {hits} 条（须恰好 1 条）"));
        }
    }
    Ok(doc.verdicts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_common::AppError;

    /// 脚本 completer：按序吐回包并留存每次收到的 prompt 全文（中立输入面
    /// 的断言面），只走无思考的 json 接缝。
    struct Scripted(
        std::sync::Mutex<Vec<Result<String, AppError>>>,
        std::sync::Mutex<Vec<String>>,
    );

    impl Scripted {
        fn new(steps: Vec<Result<String, AppError>>) -> Self {
            Self(std::sync::Mutex::new(steps), std::sync::Mutex::new(Vec::new()))
        }

        fn calls(&self) -> usize {
            self.1.lock().unwrap().len()
        }

        fn prompts(&self) -> Vec<String> {
            self.1.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl LearningCompleter for Scripted {
        async fn complete(
            &self,
            _: Option<(&str, &str)>,
            _: &str,
            _: &str,
            _: u32,
        ) -> Result<String, AppError> {
            unreachable!("adjudication only uses the json seam")
        }

        async fn complete_json(
            &self,
            _: Option<(&str, &str)>,
            _: &str,
            prompt: &str,
            _: u32,
            _: Option<f32>,
        ) -> Result<String, AppError> {
            self.1.lock().unwrap().push(prompt.to_owned());
            let mut replies = self.0.lock().unwrap();
            if replies.is_empty() {
                return Err(AppError::Internal("script exhausted".into()));
            }
            replies.remove(0)
        }
    }

    fn suspect(name: &str, mark: char) -> NameSuspect {
        NameSuspect {
            name: name.into(),
            mark,
            location: "blocks[0]".into(),
        }
    }

    fn verdicts(rows: &[(&str, bool, &str)]) -> String {
        serde_json::json!({
            "verdicts": rows
                .iter()
                .map(|(name, bundle, reason)| serde_json::json!({
                    "name": name,
                    "bundle": bundle,
                    "reason": reason,
                }))
                .collect::<Vec<_>>()
        })
        .to_string()
    }

    fn resolved(adjudication: Adjudication) -> Vec<NameVerdict> {
        match adjudication {
            Adjudication::Resolved(verdicts) => verdicts,
            Adjudication::Degraded(reason) => panic!("expected a resolved adjudication: {reason}"),
        }
    }

    fn degraded(adjudication: Adjudication) -> String {
        match adjudication {
            Adjudication::Degraded(reason) => reason,
            Adjudication::Resolved(verdicts) => panic!("expected degradation: {verdicts:?}"),
        }
    }

    /// 一批裁完：非捆绑放行、捆绑维持；一次调用覆盖全部点名。
    #[tokio::test]
    async fn adjudicates_a_batch_in_one_call() {
        let completer = Scripted::new(vec![Ok(verdicts(&[
            ("求和公式", false, "固定术语"),
            ("双进程与窗口", true, "两个主题"),
        ]))]);
        let mut adjudicator = NameAdjudicator::new(&completer, None);
        let suspects = [suspect("求和公式", '和'), suspect("双进程与窗口", '与')];
        let rows = resolved(adjudicator.adjudicate(&suspects).await);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "求和公式");
        assert!(!rows[0].bundle);
        assert!(rows[1].bundle);
        assert_eq!(completer.calls(), 1);
    }

    /// 中立输入面：只给名字 + 被点字符——位置、产物面、机械门结论都不进
    /// 裁决输入（免锚定）。
    #[tokio::test]
    async fn input_carries_only_name_and_mark() {
        let completer = Scripted::new(vec![Ok(verdicts(&[("求和公式", false, "固定术语")]))]);
        let mut adjudicator = NameAdjudicator::new(&completer, None);
        resolved(adjudicator.adjudicate(&[suspect("求和公式", '和')]).await);
        let prompt = completer.prompts().remove(0);
        assert!(prompt.contains("求和公式") && prompt.contains("「和」"), "{prompt}");
        assert!(!prompt.contains("blocks["), "位置不得进裁决输入: {prompt}");
        assert!(!prompt.contains("gd."), "条款编号不得进裁决输入: {prompt}");
        assert!(!prompt.contains("违规"), "机械门结论不得进裁决输入: {prompt}");
        assert!(!prompt.contains("同步块"), "产物面不得进裁决输入: {prompt}");
        assert!(!prompt.contains("{{"), "占位符已装配: {prompt}");
    }

    /// 运行内 memo：同名跨调用只裁一次；结果逐行返回。
    #[tokio::test]
    async fn memoizes_by_name_across_calls() {
        let completer = Scripted::new(vec![Ok(verdicts(&[
            ("求和公式", false, "固定术语"),
            ("双进程与窗口", true, "两个主题"),
        ]))]);
        let mut adjudicator = NameAdjudicator::new(&completer, None);
        let suspects = [suspect("求和公式", '和'), suspect("双进程与窗口", '与')];
        let first = resolved(adjudicator.adjudicate(&suspects).await);
        let second = resolved(adjudicator.adjudicate(&suspects).await);
        assert_eq!(first, second);
        assert_eq!(completer.calls(), 1, "同名跨 attempt 只裁一次");
    }

    /// 同名重复只送一条（输出契约按名字去重），返回覆盖每个输入行。
    #[tokio::test]
    async fn duplicate_suspect_names_are_sent_once() {
        let completer = Scripted::new(vec![Ok(verdicts(&[("求和公式", false, "固定术语")]))]);
        let mut adjudicator = NameAdjudicator::new(&completer, None);
        let suspects = [suspect("求和公式", '和'), suspect("求和公式", '和')];
        let rows = resolved(adjudicator.adjudicate(&suspects).await);
        assert_eq!(rows.len(), 2);
        let prompt = completer.prompts().remove(0);
        assert_eq!(prompt.matches("求和公式").count(), 1, "{prompt}");
    }

    /// 裁决器故障（传输/解析/形状违约）一律 Degraded——调用方降级为提示。
    #[tokio::test]
    async fn transport_failure_is_degraded() {
        let completer = Scripted::new(vec![Err(AppError::BadGateway(
            "LLM provider error: API error 502".into(),
        ))]);
        let mut adjudicator = NameAdjudicator::new(&completer, None);
        let reason = degraded(adjudicator.adjudicate(&[suspect("求和公式", '和')]).await);
        assert!(reason.contains("裁决调用失败"), "{reason}");
    }

    #[tokio::test]
    async fn unparseable_reply_is_degraded() {
        let completer = Scripted::new(vec![Ok("not json at all".into())]);
        let mut adjudicator = NameAdjudicator::new(&completer, None);
        let reason = degraded(adjudicator.adjudicate(&[suspect("求和公式", '和')]).await);
        assert!(reason.contains("解析失败"), "{reason}");
    }

    /// 机械校验：空表/未提交名/重复/空理由/缺名 → 整次不可用（Degraded）。
    #[tokio::test]
    async fn verdict_shape_violations_are_degraded() {
        let cases: Vec<(&str, String)> = vec![
            ("空表", serde_json::json!({ "verdicts": [] }).to_string()),
            (
                "未提交名",
                verdicts(&[("别的名字", false, "x")]),
            ),
            (
                "重复",
                verdicts(&[("求和公式", false, "a"), ("求和公式", true, "b")]),
            ),
            ("空理由", verdicts(&[("求和公式", true, " " )])),
            ("缺名", r#"{"verdicts":[{"bundle":true,"reason":"x"}]}"#.to_string()),
        ];
        for (label, reply) in cases {
            let completer = Scripted::new(vec![Ok(reply)]);
            let mut adjudicator = NameAdjudicator::new(&completer, None);
            let reason = degraded(adjudicator.adjudicate(&[suspect("求和公式", '和')]).await);
            assert!(!reason.is_empty(), "{label}: 必须给出降级原因");
        }

        // 缺输入名的覆盖：提交了别的名字 = 未提交名，同上；再单独核「输出
        // 恰一条 / 每个提交名恰一条」——空覆盖单独由上面的「空表」覆盖。
        let completer = Scripted::new(vec![Ok(verdicts(&[("求和公式", false, "固定术语")]))]);
        let mut adjudicator = NameAdjudicator::new(&completer, None);
        let reason = degraded(
            adjudicator
                .adjudicate(&[suspect("求和公式", '和'), suspect("双进程与窗口", '与')])
                .await,
        );
        assert!(reason.contains("恰好 1 条") || reason.contains("0 条"), "{reason}");
    }

    /// 半角逗号/斜杠的标记标注面（裁决输入的人读措辞）。
    #[tokio::test]
    async fn mark_labels_are_spelled_out_for_ascii_marks() {
        let completer = Scripted::new(vec![Ok(verdicts(&[
            ("甲,乙", true, "两个主题"),
            ("甲/乙", true, "两个主题"),
        ]))]);
        let mut adjudicator = NameAdjudicator::new(&completer, None);
        resolved(
            adjudicator
                .adjudicate(&[suspect("甲,乙", ','), suspect("甲/乙", '/')])
                .await,
        );
        let prompt = completer.prompts().remove(0);
        assert!(prompt.contains("半角逗号") && prompt.contains("斜杠"), "{prompt}");
    }
}
