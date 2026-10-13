//! 知识结构生成门面（wayfinder 票 g2/g4）：① structure ② 前置地形串行双
//! 调用（json 接缝、thinking ON），任一产物违反契约或门口对账失败，两份
//! **一起**重新生成并把逐条款违规报告注入重试装配，≤3 轮，全成才返回快照
//! 信封（service 层单 UPDATE 原子落列）。provider 不支持 json_object 即整轮
//! 失败并明示原因（fail loud，不降级）——重试不耗时：provider 在流入口即拒。

use serde_json::Value;

use nomifun_common::AppError;

use super::{structure_contract, structure_prompts};
use crate::completer::LearningCompleter;

/// 整批重试预算（g2 裁决：learnhub MAX_ATTEMPTS 同款）。
pub(crate) const MAX_ATTEMPTS: usize = 3;

/// 会话级系统行（任务与纪律全部住提示词资产；这里只固定输出通道的基调）。
const STRUCTURE_SYSTEM: &str =
    "你是学习课程的知识结构生成引擎：严格按任务说明输出一个 JSON 对象，不输出任何其他字符。";

/// 建课三项输入（目标必填；程度/基础可选——缺省值在装配时映射成扩写/纯零
/// 基础指令，g2 输入最小化）。
#[derive(Debug, Clone)]
pub struct StructureInput {
    pub goal: String,
    pub proficiency_statement: Option<String>,
    pub learner_basis: Option<String>,
}

impl StructureInput {
    /// 归一构造：trim 后空串视为未提供。
    pub fn new(
        goal: &str,
        proficiency_statement: Option<&str>,
        learner_basis: Option<&str>,
    ) -> Self {
        let trim = |value: Option<&str>| {
            value
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        Self {
            goal: goal.trim().to_owned(),
            proficiency_statement: trim(proficiency_statement),
            learner_basis: trim(learner_basis),
        }
    }
}

/// 一轮成功的双产物（未落盘的中间值；落盘 = service 层快照原子更新）。
#[derive(Debug, Clone)]
pub(crate) struct StructureArtifacts {
    pub structure: Value,
    pub terrain: Value,
}

/// 运行结构生成：双调用 + 契约门 + 门口对账 + 整批重试带反馈。
/// 返回 Err = 预算耗尽（消息携带最后一轮的逐条款报告或 LLM 错误）。
pub async fn generate_structure_artifacts(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    input: &StructureInput,
) -> Result<StructureArtifacts, AppError> {
    let mut feedback = "(无:首次)".to_owned();
    let mut last = String::from("(未执行)");
    for attempt in 1..=MAX_ATTEMPTS {
        match attempt_once(completer, model_override, input, &feedback).await {
            Ok(artifacts) => return Ok(artifacts),
            Err(report) => {
                tracing::warn!(
                    attempt,
                    report_head = %report.chars().take(400).collect::<String>(),
                    "structure generation rejected — regenerating both artifacts with clause feedback"
                );
                last = report;
                // 重试反馈（契约 v2.1 §0 同款）：违规报告注入重试装配——无
                // 反馈的盲目重试零收敛。两产物共用同一份报告，各自认领本产
                // 的条款行。
                feedback = format!("上一轮产物违反契约,逐条修正后重新生成:\n{last}");
            }
        }
    }
    Err(AppError::UnprocessableEntity(format!(
        "知识结构生成 {MAX_ATTEMPTS} 轮均未通过（最后一轮原因见下）；课程已保留，可重试生成：\n{last}"
    )))
}

/// 单次尝试：结构 → 地形 串行，双契约门 + 门口对账，全成才算过。
async fn attempt_once(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    input: &StructureInput,
    feedback: &str,
) -> Result<StructureArtifacts, String> {
    let proficiency = match input.proficiency_statement.as_deref().map(str::trim) {
        Some(text) if !text.is_empty() => text.to_owned(),
        _ => "（未提供——请依据目标扩写一句可检验的程度声明，写进 proficiency_statement）".to_owned(),
    };
    let structure_prompt = structure_prompts::assemble(
        &structure_prompts::GOAL_DECOMPOSITION,
        &[
            ("goal", input.goal.as_str()),
            ("proficiency_statement", proficiency.as_str()),
            ("retry_feedback", feedback),
        ],
    )
    .map_err(|error| error.to_string())?;
    let structure_raw = crate::generation::complete_json_coach(
        completer,
        model_override,
        STRUCTURE_SYSTEM,
        &structure_prompt.text,
        crate::generation::STRUCTURE_MAX_TOKENS,
        Some(structure_prompts::GOAL_DECOMPOSITION.temperature),
    )
    .await
    .map_err(|error| error.to_string())?;
    let structure = parse_artifact(&structure_raw, "知识结构")?;

    let basis = match input.learner_basis.as_deref().map(str::trim) {
        Some(text) if !text.is_empty() => text.to_owned(),
        _ => "（未提供——按纯零基础处理）".to_owned(),
    };
    let terrain_prompt = structure_prompts::assemble(
        &structure_prompts::PREREQUISITE_TERRAIN,
        &[
            ("goal", input.goal.as_str()),
            ("learner_basis", basis.as_str()),
            ("retry_feedback", feedback),
        ],
    )
    .map_err(|error| error.to_string())?;
    let terrain_raw = crate::generation::complete_json_coach(
        completer,
        model_override,
        STRUCTURE_SYSTEM,
        &terrain_prompt.text,
        crate::generation::STRUCTURE_MAX_TOKENS,
        Some(structure_prompts::PREREQUISITE_TERRAIN.temperature),
    )
    .await
    .map_err(|error| error.to_string())?;
    let terrain = parse_artifact(&terrain_raw, "前置地形")?;

    // 契约门：双产物逐条款执法 + 门口对账（g2：structure 的门口只准引用
    // 地形门口）。警告级只透出，不触发重试。
    let mut violations = structure_contract::validate_structure(&structure);
    violations.extend(structure_contract::validate_terrain(&terrain));
    violations.extend(structure_contract::reconcile_doors(&structure, &terrain));
    let warnings: Vec<String> = violations
        .iter()
        .filter(|violation| violation.warning)
        .map(|violation| format!("[{}] {}", violation.code, violation.message))
        .collect();
    if !warnings.is_empty() {
        tracing::info!(warnings = ?warnings, "structure generation warnings (non-rejecting)");
    }
    let red: Vec<&structure_contract::ContractViolation> =
        violations.iter().filter(|violation| !violation.warning).collect();
    if !red.is_empty() {
        let owned: Vec<structure_contract::ContractViolation> =
            red.into_iter().cloned().collect();
        return Err(structure_contract::format_violations(&owned));
    }
    Ok(StructureArtifacts { structure, terrain })
}

/// 解析一份模型回包为 JSON 对象（json 模式保证合法性；这里再兜一层剥围栏
/// 解析，防御忽略 response_format 的怪癖 provider）。
fn parse_artifact(raw: &str, label: &str) -> Result<Value, String> {
    let value = crate::generation::parse_json_object::<Value>(raw)
        .map_err(|error| format!("{label}解析失败（非合法 JSON 对象）：{error}"))?;
    if !value.is_object() {
        let head: String = raw.trim().chars().take(200).collect();
        return Err(format!("{label}必须是单个 JSON 对象，得到：{head}"));
    }
    Ok(value)
}

/// 快照信封（契约 §5）：version + generated_at + 双产物，单 UPDATE 原子写列。
pub(crate) fn snapshot_envelope(structure: &Value, terrain: &Value, generated_at: i64) -> Value {
    serde_json::json!({
        "version": 1,
        "generated_at": generated_at,
        "structure": structure,
        "terrain": terrain,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 最小过门样本（与 structure_contract 测试同款构型，供门面回路测试）。
    fn golden_structure() -> Value {
        let spine_names = ["主干一", "主干二", "主干三", "主干四", "主干五", "主干六"];
        let mut blocks = Vec::new();
        for (spine_index, spine) in spine_names.iter().enumerate() {
            for block_index in 0..7 {
                let ordinal = spine_index * 7 + block_index;
                let stage = ["入门", "进阶", "高级"][ordinal % 3];
                blocks.push(json!({
                    "name": format!("块{ordinal:02}"),
                    "slug": format!("block-{ordinal:02}"),
                    "spine": spine,
                    "stage": stage,
                    "note": format!("第 {ordinal} 个主题的一句话描述"),
                }));
            }
        }
        json!({
            "goal": "能独立完成一元二次方程的求解与检验",
            "goal_type": "capability",
            "proficiency_statement": "达成后能独立解任意一元二次方程并讲解每一步依据",
            "domain_type": "理论",
            "form_reference": "Flowy 学习结构模板 v1",
            "source_tier": "名录",
            "generated_by": "test-model",
            "disclaimer": "快照为生成时点参考地图，可能存在盲区",
            "domain": {
                "core_definition": "一元二次方程的代数求解体系",
                "includes": ["方程变形", "求根公式", "判别式"],
                "excludes": ["三次方程", "复数域", "数值分析"],
                "prerequisite_doors": ["G1"],
                "audience": "零基础成年学习者",
                "scenarios": ["解题应用", "讲解他人"],
                "granularity": "每块一个可独立教学的解题动作"
            },
            "spines": spine_names.iter().map(|spine| json!({
                "name": spine,
                "definition": format!("{spine}的一句话定义"),
                "key_points": ["要点一"],
                "instance": format!("达标{spine}时能独立完成一次求解"),
            })).collect::<Vec<_>>(),
            "blocks": blocks,
            "edges": [
                { "source": "块01", "target": "块00", "type": "assumes", "tier": "know", "note": "先认后用" }
            ],
            "gaps": [
                { "item": "应用题情境覆盖不足", "direction": "补行程类情境" },
                { "item": "图像法未展开", "direction": "补抛物线图像判读" },
                { "item": "近似解法缺位", "direction": "补估算策略" }
            ],
            "quality": [
                { "check": "主干数在 6–12 档", "status": "met" },
                { "check": "块名一物一名", "status": "met" },
                { "check": "边端点全部可解析", "status": "met" },
                { "check": "无顺序承诺", "status": "met" },
                { "check": "盲区已标注", "status": "unmet", "reason": "应用情境块待补" }
            ]
        })
    }

    fn golden_terrain() -> Value {
        json!({
            "goal": "能独立完成一元二次方程的求解与检验",
            "learner_basis_assumed": "纯零基础（未声明）",
            "generated_by": "test-model",
            "disclaimer": "门口判据为生成时点快照，供自检参考",
            "doors": [
                { "id": "G1", "criterion": "能用配方法解形如 x²=p 的方程", "kind": "hard" }
            ]
        })
    }

    /// 脚本 completer：按序吐回包，只走 json 接缝；脚本耗尽即报错。
    struct Scripted(std::sync::Mutex<Vec<String>>);

    #[async_trait::async_trait]
    impl LearningCompleter for Scripted {
        async fn complete(
            &self,
            _: Option<(&str, &str)>,
            _: &str,
            _: &str,
            _: u32,
        ) -> Result<String, nomifun_common::AppError> {
            unreachable!("structure pipeline only uses the json seam")
        }
        async fn complete_json_with_thinking(
            &self,
            _: Option<(&str, &str)>,
            _: &str,
            _: &str,
            _: u32,
            _: Option<f32>,
        ) -> Result<String, nomifun_common::AppError> {
            let mut replies = self.0.lock().unwrap();
            if replies.is_empty() {
                return Err(nomifun_common::AppError::Internal("script exhausted".into()));
            }
            Ok(replies.remove(0))
        }
    }

    fn scripted(replies: Vec<Value>) -> Scripted {
        Scripted(std::sync::Mutex::new(
            replies
                .into_iter()
                .map(|value| serde_json::to_string(&value).unwrap())
                .collect(),
        ))
    }

    /// 过门回路：双产物契约全绿 + 门口对账通过，门面一轮即成功。
    #[tokio::test]
    async fn golden_artifacts_pass_in_one_attempt() {
        let completer = scripted(vec![golden_structure(), golden_terrain()]);
        let artifacts = generate_structure_artifacts(
            &completer,
            None,
            &StructureInput {
                goal: "能独立完成一元二次方程的求解与检验".into(),
                proficiency_statement: None,
                learner_basis: None,
            },
        )
        .await
        .expect("golden artifacts pass");
        assert_eq!(artifacts.structure["goal_type"], json!("capability"));
        assert_eq!(artifacts.terrain["doors"][0]["id"], json!("G1"));
    }

    /// 首轮门口对账失败（结构引用了地形未定义的 G9）→ 报告进反馈 → 第二轮
    /// 脚本给出对齐产物 → 成功，且第二轮收到的装配值带反馈。
    #[tokio::test]
    async fn door_mismatch_triggers_feedback_retry() {
        let mut bad_structure = golden_structure();
        bad_structure["domain"]["prerequisite_doors"] = json!(["G1", "G9"]);
        let completer = scripted(vec![
            bad_structure,
            golden_terrain(),
            golden_structure(),
            golden_terrain(),
        ]);
        let artifacts = generate_structure_artifacts(
            &completer,
            None,
            &StructureInput {
                goal: "目标".into(),
                proficiency_statement: None,
                learner_basis: None,
            },
        )
        .await
        .expect("second attempt aligns the doors");
        assert_eq!(artifacts.structure["domain"]["prerequisite_doors"], json!(["G1"]));
    }

    /// 预算耗尽：持续违约 = 明示失败，消息携带最后一轮逐条款报告。
    #[tokio::test]
    async fn exhausted_budget_fails_loud_with_clause_report() {
        let mut bad = golden_structure();
        bad["blocks"] = json!([]); // gd.blocks.count 红，永不修正
        let completer = scripted(vec![bad; 6]); // 3 轮 ×（结构 + 地形）
        let error = generate_structure_artifacts(
            &completer,
            None,
            &StructureInput {
                goal: "目标".into(),
                proficiency_statement: None,
                learner_basis: None,
            },
        )
        .await
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("3 轮"), "{message}");
        assert!(message.contains("gd.blocks.count"), "{message}");
    }

    /// 快照信封：version/generated_at/双产物成形，且是 JSON 对象（列 CHECK）。
    #[test]
    fn envelope_carries_both_artifacts() {
        let envelope = snapshot_envelope(&golden_structure(), &golden_terrain(), 1000);
        assert!(envelope.is_object());
        assert_eq!(envelope["version"], json!(1));
        assert_eq!(envelope["generated_at"], json!(1000));
        assert!(envelope["structure"].is_object());
        assert!(envelope["terrain"].is_object());
    }
}
