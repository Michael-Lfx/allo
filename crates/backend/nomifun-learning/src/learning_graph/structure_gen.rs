//! 知识结构生成门面（wayfinder 票 g2/g4）：① 前置地形 ② structure 串行双
//! 调用（json 接缝、thinking ON；e2 判定输入通道：地形先产出，门口清单注入
//! 结构装配——door_refs 门位标签的判定源），任一产物违反契约或门口对账失
//! 败，两份**一起**重新生成并把逐条款违规报告注入重试装配，≤3 轮，全成才返
//! 回快照信封（service 层单 UPDATE 原子落列）。provider 不支持 json_object
//! 即整轮失败并明示原因（fail loud，不降级）——重试不耗时：provider 在流入
//! 口即拒。

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
/// 重试机制（二轮 t1）：同指纹早停——连败两次的逐条款报告相同即判「反馈
/// 回灌已无法改变结果」，立即耗尽（LLM 调用数 2 非 3）。真取消（三轮 t2）：
/// `cancel` 令牌在循环头检查、并与两次 LLM 调用 `select!` 竞速（中断 =
/// 断 HTTP）；快照落列前的取消检查在 service 层（写前取消 = 零快照）。
pub(crate) async fn generate_structure_artifacts(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    input: &StructureInput,
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> Result<StructureArtifacts, AppError> {
    let cancellation = cancel
        .map(|token| crate::service::CancellationCompleter::new(completer, token.clone()));
    let completer: &dyn LearningCompleter = match &cancellation {
        Some(wrapper) => wrapper,
        None => completer,
    };
    let mut feedback = "(无:首次)".to_owned();
    let mut last = String::from("(未执行)");
    let mut prev_fingerprint: Option<String> = None;
    // 提前停战轮次（e1 D2）：消息面如实区分「预算耗尽」与「同指纹早停」——
    // 早停后仍报满预算轮数是撒谎，读数与排障都依赖真实轮数。
    let mut early_stop: Option<usize> = None;
    for attempt in 1..=MAX_ATTEMPTS {
        if cancel.is_some_and(|token| token.is_cancelled()) {
            return Err(AppError::Conflict(
                crate::service::CANCELLED_MESSAGE.to_owned(),
            ));
        }
        match attempt_once(completer, model_override, input, &feedback).await {
            Ok(artifacts) => return Ok(artifacts),
            Err(report) => {
                tracing::warn!(
                    attempt,
                    report_head = %report.chars().take(400).collect::<String>(),
                    "structure generation rejected — regenerating both artifacts with clause feedback"
                );
                // 同指纹早停（e1 D2 通用化：判据走 growth_retry::
                // should_stop_early 单源——「两个循环共用指纹与判别口径」从
                // 注释约定升为代码事实）。上一轮也是这份逐条款报告 → 反馈回
                // 灌已无法改变结果（同装配同输出），立即耗尽不烧剩余轮次——
                // 省掉 E2E 实录的 4 分钟白跑。守卫豁免随共享判据自带；结构
                // 侧当前没有机械守卫失败面（attempt_once 的失败全是生成面：
                // 解析/契约/LLM 错误），guard 恒 false——判据先行统一，将来
                // 加守卫面时早停语义零改动。
                let fingerprint = super::growth_retry::normalize_report(&report);
                if super::growth_retry::should_stop_early(
                    attempt,
                    &fingerprint,
                    prev_fingerprint.as_deref(),
                    false,
                ) {
                    tracing::warn!(
                        attempt,
                        "structure generation produced the same violations twice — stopping early"
                    );
                    last = report;
                    early_stop = Some(attempt);
                    break;
                }
                prev_fingerprint = Some(fingerprint);
                last = report;
                // 重试反馈（契约 v2.1 §0 同款）：违规报告注入重试装配——无
                // 反馈的盲目重试零收敛。两产物共用同一份报告，各自认领本产
                // 的条款行。
                feedback = format!("上一轮产物违反契约,逐条修正后重新生成:\n{last}");
            }
        }
    }
    let message = match early_stop {
        Some(attempt) => format!(
            "知识结构生成连续两轮产出相同违约报告（重试反馈已无法收敛），\
             在第 {attempt}/{MAX_ATTEMPTS} 轮提前停止；课程已保留，可重试生成：\n{last}"
        ),
        None => format!(
            "知识结构生成 {MAX_ATTEMPTS} 轮均未通过（最后一轮原因见下）；课程已保留，可重试生成：\n{last}"
        ),
    };
    Err(AppError::UnprocessableEntity(message))
}

/// 门口清单装配（纯函数，learnhub `door_list_markdown` 域中立移植）：每门一行
/// `- [编号]（hard=硬前置/self_check=可自检）判据`，编号与判据逐字取自前置地形
/// 产物——结构侧课程门「逐字全收」与 door_refs 门位标签覆盖判定的判定源。
/// 行措辞与已会声明的 terrain_reference 同族（[编号]（语义）判据）。
pub(crate) fn door_list_markdown(terrain: &Value) -> String {
    let doors = terrain
        .get("doors")
        .and_then(Value::as_array)
        .map(|doors| doors.as_slice())
        .unwrap_or_default();
    if doors.is_empty() {
        return "（无：前置地形未产出门口清单）".to_owned();
    }
    doors
        .iter()
        .filter_map(|door| {
            let id = door.get("id").and_then(Value::as_str)?.trim();
            let criterion = door.get("criterion").and_then(Value::as_str)?.trim();
            let kind = match door.get("kind").and_then(Value::as_str).unwrap_or_default() {
                "hard" => "硬前置",
                "self_check" => "可自检",
                _ => "门口",
            };
            Some(format!("- [{id}]（{kind}）{criterion}"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 单次尝试：地形 → 结构 串行（e2 判定输入通道：结构生成必须看见门口清单
/// ——编号+判据是 door_refs 门位标签与课程门逐字全收的判定源，learnhub #161
/// 同构），双契约门 + 门口对账，全成才算过。
async fn attempt_once(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    input: &StructureInput,
    feedback: &str,
) -> Result<StructureArtifacts, String> {
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

    let proficiency = match input.proficiency_statement.as_deref().map(str::trim) {
        Some(text) if !text.is_empty() => text.to_owned(),
        _ => "（未提供——请依据目标扩写一句可检验的程度声明，写进 proficiency_statement）".to_owned(),
    };
    let door_list = door_list_markdown(&terrain);
    let structure_prompt = structure_prompts::assemble(
        &structure_prompts::GOAL_DECOMPOSITION,
        &[
            ("goal", input.goal.as_str()),
            ("proficiency_statement", proficiency.as_str()),
            ("door_list", door_list.as_str()),
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

    /// 脚本 completer：按序吐回包并留存每次收到的 prompt 全文（e2 判定输入
    /// 通道的断言面），只走 json 接缝；脚本耗尽即报错。留存的 prompt 数
    /// （= json 接缝调用次数）供早停红绿样断言「剩余预算没烧」。
    struct Scripted(std::sync::Mutex<Vec<String>>, std::sync::Mutex<Vec<String>>);

    impl Scripted {
        fn calls(&self) -> usize {
            self.1.lock().unwrap().len()
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
        ) -> Result<String, nomifun_common::AppError> {
            unreachable!("structure pipeline only uses the json seam")
        }
        async fn complete_json_with_thinking(
            &self,
            _: Option<(&str, &str)>,
            _: &str,
            prompt: &str,
            _: u32,
            _: Option<f32>,
        ) -> Result<String, nomifun_common::AppError> {
            let mut prompts = self.1.lock().unwrap();
            prompts.push(prompt.to_owned());
            drop(prompts);
            let mut replies = self.0.lock().unwrap();
            if replies.is_empty() {
                return Err(nomifun_common::AppError::Internal("script exhausted".into()));
            }
            Ok(replies.remove(0))
        }
    }

    fn scripted(replies: Vec<Value>) -> Scripted {
        Scripted(
            std::sync::Mutex::new(
                replies
                    .into_iter()
                    .map(|value| serde_json::to_string(&value).unwrap())
                    .collect(),
            ),
            std::sync::Mutex::new(Vec::new()),
        )
    }

    /// 过门回路：双产物契约全绿 + 门口对账通过，门面一轮即成功；且结构装配
    /// 收到的 prompt 携带门口清单（e2 判定输入通道：地形先行、编号+判据注入）。
    #[tokio::test]
    async fn golden_artifacts_pass_in_one_attempt() {
        let completer = scripted(vec![golden_terrain(), golden_structure()]);
        let artifacts = generate_structure_artifacts(
            &completer,
            None,
            &StructureInput {
                goal: "能独立完成一元二次方程的求解与检验".into(),
                proficiency_statement: None,
                learner_basis: None,
            },
            None,
        )
        .await
        .expect("golden artifacts pass");
        assert_eq!(artifacts.structure["goal_type"], json!("capability"));
        assert_eq!(artifacts.terrain["doors"][0]["id"], json!("G1"));
        // 判定输入通道：第一次调用 = 地形，第二次 = 结构且带门口清单原文。
        let prompts = completer.1.lock().unwrap();
        assert_eq!(prompts.len(), 2);
        assert!(prompts[0].contains("前置地形"), "{:#}", prompts[0]);
        assert!(
            prompts[1].contains("知识结构") && prompts[1].contains("门口清单"),
            "{:#}",
            prompts[1]
        );
        assert!(
            prompts[1].contains("能用配方法解形如 x²=p 的方程"),
            "结构装配必须携带门口判据原文（door_refs 判定源）：{:#}",
            prompts[1]
        );
        assert!(prompts[1].contains("- [G1]（硬前置）"), "{:#}", prompts[1]);
    }

    /// 门口清单装配（纯函数）：判据逐字、kind 双档措辞、空清单防御回退。
    #[test]
    fn door_list_markdown_renders_ids_kinds_and_criteria() {
        let two_doors = json!({
            "doors": [
                { "id": "G1", "criterion": "能用配方法解形如 x²=p 的方程", "kind": "hard" },
                { "id": "G2", "criterion": "可辨认等式两边同乘同除的合法性", "kind": "self_check" }
            ]
        });
        let list = door_list_markdown(&two_doors);
        assert!(
            list.contains("- [G1]（硬前置）能用配方法解形如 x²=p 的方程"),
            "{list}"
        );
        assert!(
            list.contains("- [G2]（可自检）可辨认等式两边同乘同除的合法性"),
            "{list}"
        );
        assert_eq!(door_list_markdown(&json!({ "doors": [] })), "（无：前置地形未产出门口清单）");
        assert_eq!(door_list_markdown(&json!({})), "（无：前置地形未产出门口清单）");
    }

    /// 首轮门口对账失败（结构引用了地形未定义的 G9）→ 报告进反馈 → 第二轮
    /// 脚本给出对齐产物 → 成功，且第二轮收到的装配值带反馈。
    #[tokio::test]
    async fn door_mismatch_triggers_feedback_retry() {
        let mut bad_structure = golden_structure();
        bad_structure["domain"]["prerequisite_doors"] = json!(["G1", "G9"]);
        let completer = scripted(vec![
            golden_terrain(),
            bad_structure,
            golden_terrain(),
            golden_structure(),
        ]);
        let artifacts = generate_structure_artifacts(
            &completer,
            None,
            &StructureInput {
                goal: "目标".into(),
                proficiency_statement: None,
                learner_basis: None,
            },
            None,
        )
        .await
        .expect("second attempt aligns the doors");
        assert_eq!(artifacts.structure["domain"]["prerequisite_doors"], json!(["G1"]));
    }

    /// m1 叉积核验走既有反馈重试回路（零新机制）：块 door_refs 引用了地形
    /// 未定义的 G9 → 首轮打回（gd.blocks.door_refs 进逐条款报告）→ 第二轮
    /// 对齐产物过门。
    #[tokio::test]
    async fn dangling_block_door_refs_triggers_feedback_retry() {
        let mut bad_structure = golden_structure();
        bad_structure["blocks"][0]["door_refs"] = json!(["G1", "G9"]);
        let mut aligned = golden_structure();
        aligned["blocks"][0]["door_refs"] = json!(["G1"]);
        let completer = scripted(vec![
            golden_terrain(),
            bad_structure,
            golden_terrain(),
            aligned,
        ]);
        let artifacts = generate_structure_artifacts(
            &completer,
            None,
            &StructureInput {
                goal: "目标".into(),
                proficiency_statement: None,
                learner_basis: None,
            },
            None,
        )
        .await
        .expect("second attempt aligns the block door refs");
        assert_eq!(artifacts.structure["blocks"][0]["door_refs"], json!(["G1"]));
    }

    /// 预算耗尽（全预算路径）：每轮**不同**违约（指纹永不重复）= 早停不
    /// 触发，明示失败烧满 3 轮（6 次调用），消息携带最后一轮逐条款报告。
    #[tokio::test]
    async fn exhausted_budget_fails_loud_with_clause_report() {
        let mut bad_a = golden_structure();
        bad_a["blocks"] = json!([]); // gd.blocks.count 红
        let mut bad_b = golden_structure();
        bad_b["domain"]["prerequisite_doors"] = json!(["G1", "G9"]); // 门口对账红
        let mut bad_c = golden_structure();
        bad_c["blocks"][0]["door_refs"] = json!(["G1", "G9"]); // gd.blocks.door_refs 红
        let completer = scripted(vec![
            golden_terrain(), bad_a, // 第 1 轮
            golden_terrain(), bad_b, // 第 2 轮（违约互异 → 早停不触发）
            golden_terrain(), bad_c, // 第 3 轮
        ]); // 3 轮 ×（地形 + 结构），每轮违约不同 = 指纹永不重复
        let error = generate_structure_artifacts(
            &completer,
            None,
            &StructureInput {
                goal: "目标".into(),
                proficiency_statement: None,
                learner_basis: None,
            },
            None,
        )
        .await
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("3 轮"), "{message}");
        assert!(message.contains("gd.blocks.door_refs"), "{message}");
        assert_eq!(completer.calls(), 6, "three full rounds x (structure + terrain)");
    }

    /// e1 D2 红绿样（早停臂）：同一违约报告连败两次 = 反馈回灌已无法改变
    /// 结果 → 第 2 轮即提前停止，剩余轮次不烧（4 次调用非 6）；消息如实
    /// 报「提前停止」并携带违约报告。
    #[tokio::test]
    async fn same_fingerprint_stops_early_and_saves_the_remaining_budget() {
        let mut bad = golden_structure();
        bad["blocks"] = json!([]); // gd.blocks.count 红，永不修正
        let completer = scripted(vec![
            golden_terrain(), bad.clone(), // 第 1 轮
            golden_terrain(), bad.clone(), // 第 2 轮（同指纹 → 早停）
            golden_terrain(), bad, // 不应到达（预算 proof：脚本有余量）
        ]);
        let error = generate_structure_artifacts(
            &completer,
            None,
            &StructureInput {
                goal: "目标".into(),
                proficiency_statement: None,
                learner_basis: None,
            },
            None,
        )
        .await
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("提前停止"), "{message}");
        assert!(message.contains("2/3"), "{message}");
        assert!(message.contains("gd.blocks.count"), "{message}");
        assert_eq!(
            completer.calls(),
            4,
            "early stop after the second identical fingerprint: 2 rounds x 2 calls, not 6"
        );
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
