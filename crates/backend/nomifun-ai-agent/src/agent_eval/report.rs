//! Three-column T01/T02/T03 report: goal achievement first, agent-loop efficiency second.

use nomifun_api_types::{
    EvalBusinessMatrixRow, EvalBusinessReport, EvalBusinessTaskReport, EvalCaseView, EvalRunView,
    EvalScorerView,
};

pub fn build_business_report(view: &EvalRunView) -> EvalBusinessReport {
    let tasks: Vec<EvalBusinessTaskReport> = ["t01", "t02", "t03"]
        .into_iter()
        .filter_map(|id| view.cases.iter().find(|case| case.case_id == id))
        .map(task_from_case)
        .collect();
    let t01 = cell(&tasks, "t01");
    let t02 = cell(&tasks, "t02");
    let t03 = cell(&tasks, "t03");
    let unique = tasks.len();
    let passed = tasks.iter().filter(|task| task.success).count();
    let failed = unique.saturating_sub(passed);

    EvalBusinessReport {
        run_id: view.run_id.clone(),
        suite: view.suite.clone(),
        model: view.model.clone(),
        provider_id: view.provider_id.clone(),
        status: view.status.clone(),
        passed_cases: passed,
        failed_cases: failed,
        unique_cases: unique,
        goal_rows: vec![
            matrix_row(
                "指定产物",
                artifact_label(t01, "AI_PC_Procurement_Brief.md", &["AI_PC_Procurement_Brief.md"]),
                artifact_label(
                    t02,
                    "Project_Summary.md + Action_Items.xlsx",
                    &["Project_Summary.md", "Action_Items.xlsx"],
                ),
                artifact_label(
                    t03,
                    "Management_Summary.md + Management_Summary.xlsx",
                    &["Management_Summary.md", "Management_Summary.xlsx"],
                ),
                "题目要求写到评测工作区的文件。✓ 已写出，✗ 缺失。",
            ),
            matrix_row(
                "结构 Gate",
                gate_summary(t01),
                gate_summary(t02),
                gate_summary(t03),
                "计入本题是否通过的硬性检查（文件是否存在、关键词、表结构等）。",
            ),
            matrix_row(
                "允许的缺口",
                gap_label(t01, "字段可「未查到」"),
                gap_label(t02, "冲突点名 → advisory"),
                gap_label(t03, "算术精度 → advisory"),
                "参考项（advisory），失败不改变本题 success。",
            ),
            matrix_row(
                "本题结果",
                success_label(t01),
                success_label(t02),
                success_label(t03),
                "只由结构 Gate 决定。参考项失败仍可通过。",
            ),
            matrix_row(
                "套件合计",
                if unique == 0 {
                    "—".into()
                } else {
                    format!("{passed}/{unique} 题通过")
                },
                "—".into(),
                "—".into(),
                "T01–T03 通过题数 / 已跑题数。数字只写在 T01 列，不是 T01 单独的分数。",
            ),
        ],
        efficiency_rows: vec![
            matrix_row(
                "总耗时",
                elapsed_label(t01),
                elapsed_label(t02),
                elapsed_label(t03),
                "本题从发起到评分结束的墙钟时间。",
            ),
            matrix_row(
                "对话轮次",
                turns_label(t01),
                turns_label(t02),
                turns_label(t03),
                "模型回复轮数。轮次高通常说明绕路或反复改文件。",
            ),
            matrix_row(
                "工具调用",
                tools_label(t01),
                tools_label(t02),
                tools_label(t03),
                "工具被调用的次数，以及其中报错的次数。报错多说明读文件或搜索不稳定。",
            ),
            matrix_row(
                "关键工具",
                key_tools_label(t01, &["web_search", "web_extract"]),
                key_tools_label(t02, &["Read", "Write"]),
                key_tools_label(t03, &["Read", "Write"]),
                "本任务期望用到的工具。未调用不一定判失败，只帮助判断路径是否对。",
            ),
            matrix_row(
                "Token 用量",
                tokens_label(t01),
                tokens_label(t02),
                tokens_label(t03),
                "输入 / 输出 token。用来看成本和上下文压力，不计入本题对错。",
            ),
            matrix_row(
                "结束原因",
                stop_label(t01),
                stop_label(t02),
                stop_label(t03),
                "说明 Agent 为什么停下来，不等于本题对错。正常结束＝模型主动收尾；输出被截断 / 轮次用尽＝被上限打断，产物可能不完整；出错＝评测进程失败。",
            ),
        ],
        tasks,
    }
}

fn task_from_case(case: &EvalCaseView) -> EvalBusinessTaskReport {
    EvalBusinessTaskReport {
        case_id: case.case_id.clone(),
        title: task_title(&case.case_id),
        category: case.category.clone(),
        success: case.success,
        elapsed_ms: case.elapsed_ms,
        turns: case.turns,
        tool_call_count: case.tool_call_count,
        tool_error_count: case.tool_error_count,
        tool_names: case.tool_names.clone(),
        input_tokens: case.input_tokens,
        output_tokens: case.output_tokens,
        stop_reason: case.stop_reason.clone(),
        error: case.error.clone(),
        gate: case.scorer_results.clone(),
        advisory: case.advisory_results.clone(),
    }
}

fn task_title(case_id: &str) -> String {
    match case_id {
        "t01" => "联网调研".into(),
        "t02" => "项目汇总".into(),
        "t03" => "销售分析".into(),
        other => other.to_owned(),
    }
}

fn cell<'a>(tasks: &'a [EvalBusinessTaskReport], id: &str) -> Option<&'a EvalBusinessTaskReport> {
    tasks.iter().find(|task| task.case_id == id)
}

fn matrix_row(
    label: &str,
    t01: String,
    t02: String,
    t03: String,
    hint: &str,
) -> EvalBusinessMatrixRow {
    EvalBusinessMatrixRow {
        label: label.to_owned(),
        t01,
        t02,
        t03,
        hint: Some(hint.to_owned()),
    }
}

fn success_label(task: Option<&EvalBusinessTaskReport>) -> String {
    match task {
        Some(task) if task.success => "Gate 全过".into(),
        Some(_) => "未通过".into(),
        None => "未跑".into(),
    }
}

fn gate_summary(task: Option<&EvalBusinessTaskReport>) -> String {
    let Some(task) = task else {
        return "未跑".into();
    };
    if task.gate.is_empty() {
        return if task.success {
            "无结构项".into()
        } else {
            "未评分".into()
        };
    }
    format_scorers(&task.gate)
}

fn gap_label(task: Option<&EvalBusinessTaskReport>, policy: &str) -> String {
    match task {
        Some(task) if task.advisory.is_empty() => policy.to_owned(),
        Some(task) => format!("{policy} · {}", format_scorers(&task.advisory)),
        None => policy.to_owned(),
    }
}

fn artifact_label(
    task: Option<&EvalBusinessTaskReport>,
    fallback: &str,
    files: &[&str],
) -> String {
    let Some(task) = task else {
        return fallback.to_owned();
    };
    files
        .iter()
        .map(|name| {
            let ok = task.gate.iter().any(|row| {
                row.scorer_type == "file_exists"
                    && row.passed
                    && row
                        .detail
                        .as_deref()
                        .is_some_and(|detail| detail.contains(&format!("path={name}")))
            });
            format!("{} {name}", if ok { "✓" } else { "✗" })
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

fn format_scorers(rows: &[EvalScorerView]) -> String {
    rows.iter()
        .map(|row| {
            let mark = if row.passed { "✓" } else { "✗" };
            format!("{mark} {}", scorer_label(&row.scorer_type))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn scorer_label(scorer_type: &str) -> String {
    match scorer_type {
        "file_exists" => "产物".into(),
        "keyword_coverage" => "关键词".into(),
        "file_regex" => "来源".into(),
        "xlsx_headers" => "列名".into(),
        "xlsx_sheets" => "工作表".into(),
        "xlsx_totals_close" => "算术".into(),
        "tool_called" => "工具".into(),
        other => other.to_owned(),
    }
}

fn elapsed_label(task: Option<&EvalBusinessTaskReport>) -> String {
    match task {
        Some(task) => format_elapsed(task.elapsed_ms),
        None => "—".into(),
    }
}

fn format_elapsed(ms: u128) -> String {
    if ms >= 60_000 {
        format!("{:.1} 分钟", ms as f64 / 60_000.0)
    } else if ms >= 1000 {
        format!("{:.1} 秒", ms as f64 / 1000.0)
    } else {
        format!("{ms} 毫秒")
    }
}

fn turns_label(task: Option<&EvalBusinessTaskReport>) -> String {
    match task {
        Some(task) => format!("{} 轮", task.turns),
        None => "—".into(),
    }
}

fn tools_label(task: Option<&EvalBusinessTaskReport>) -> String {
    match task {
        Some(task) => format!("调用 {} · 失败 {}", task.tool_call_count, task.tool_error_count),
        None => "—".into(),
    }
}

fn tokens_label(task: Option<&EvalBusinessTaskReport>) -> String {
    match task {
        Some(task) => format!("入 {} · 出 {}", task.input_tokens, task.output_tokens),
        None => "—".into(),
    }
}

fn human_stop_reason(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "end_turn" | "completed" => "正常结束（模型主动收尾）".into(),
        "tool_use" => "停在工具调用（循环未收口）".into(),
        "max_tokens" => "输出被截断（达到 token 上限）".into(),
        "max_turns" => "轮次用尽被掐断".into(),
        "refusal" => "模型拒绝作答".into(),
        "cancelled" | "canceled" => "评测被取消".into(),
        other => format!("其他（{other}）"),
    }
}

fn stop_label(task: Option<&EvalBusinessTaskReport>) -> String {
    let Some(task) = task else {
        return "未跑".into();
    };
    let reason = human_stop_reason(task.stop_reason.as_deref().unwrap_or(""));
    match task.error.as_deref().map(str::trim).filter(|error| !error.is_empty()) {
        Some(error) => format!("出错：{error}\n结束原因：{reason}"),
        None => reason,
    }
}

fn key_tools_label(task: Option<&EvalBusinessTaskReport>, want: &[&str]) -> String {
    let Some(task) = task else {
        return "—".into();
    };
    want.iter()
        .map(|name| {
            let hit = task
                .tool_names
                .iter()
                .any(|have| have.eq_ignore_ascii_case(name));
            format!("{}{}", name, if hit { " ✓" } else { " ✗" })
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_api_types::EvalRunView;

    #[test]
    fn three_column_report_marks_missing_tasks() {
        let report = build_business_report(&EvalRunView {
            run_id: "r1".into(),
            status: "completed".into(),
            suite: "imported-demo".into(),
            model: Some("cloud-model".into()),
            provider_id: Some("p".into()),
            planned: 1,
            completed: 1,
            passed: 1,
            failed: 0,
            current_case_id: None,
            error: None,
            summary: None,
            cases: vec![EvalCaseView {
                case_id: "t01".into(),
                category: "web_research".into(),
                success: true,
                elapsed_ms: 120_000,
                turns: 8,
                tool_call_count: 4,
                input_tokens: 100,
                output_tokens: 50,
                tool_error_count: 0,
                stop_reason: Some("end_turn".into()),
                error: None,
                scorer_results: vec![EvalScorerView {
                    scorer_type: "file_exists".into(),
                    passed: true,
                    detail: Some("path=AI_PC_Procurement_Brief.md exists=true".into()),
                }],
                advisory_results: vec![],
                trial: 1,
                prompt: None,
                trajectory_event_count: 0,
                artifact_count: 1,
                has_trace: false,
                conversation_id: None,
                tool_names: vec!["web_search".into(), "Write".into()],
            }],
            current_trace: None,
            current_conversation_id: None,
            workspace_label: None,
            workspace_path: None,
        });
        assert_eq!(report.unique_cases, 1);
        assert_eq!(report.passed_cases, 1);
        assert_eq!(report.goal_rows[3].t01, "Gate 全过");
        assert_eq!(report.goal_rows[3].t02, "未跑");
        assert!(report.goal_rows[0].t01.contains("✓ AI_PC_Procurement_Brief.md"));
        assert!(report.goal_rows[1].t01.contains("✓ 产物"));
        assert!(report.efficiency_rows[3].t01.contains("web_search ✓"));
        let stop = report
            .efficiency_rows
            .iter()
            .find(|row| row.label == "结束原因")
            .expect("stop row");
        assert!(stop.t01.contains("正常结束"));
        assert_eq!(stop.t02, "未跑");
        assert!(
            stop.hint
                .as_deref()
                .is_some_and(|hint| hint.contains("不等于本题对错"))
        );
        assert_eq!(report.model.as_deref(), Some("cloud-model"));
    }

    #[test]
    fn stop_row_prefers_runtime_error_over_stop_reason() {
        let report = build_business_report(&EvalRunView {
            run_id: "r2".into(),
            status: "failed".into(),
            suite: "imported-demo".into(),
            model: None,
            provider_id: None,
            planned: 1,
            completed: 1,
            passed: 0,
            failed: 1,
            current_case_id: None,
            error: None,
            summary: None,
            cases: vec![EvalCaseView {
                case_id: "t03".into(),
                category: "sales".into(),
                success: false,
                elapsed_ms: 1_000,
                turns: 2,
                tool_call_count: 1,
                input_tokens: 10,
                output_tokens: 4,
                tool_error_count: 1,
                stop_reason: Some("max_tokens".into()),
                error: Some("provider timeout".into()),
                scorer_results: vec![],
                advisory_results: vec![],
                trial: 1,
                prompt: None,
                trajectory_event_count: 0,
                artifact_count: 0,
                has_trace: false,
                conversation_id: None,
                tool_names: vec![],
            }],
            current_trace: None,
            current_conversation_id: None,
            workspace_label: None,
            workspace_path: None,
        });
        let stop = report
            .efficiency_rows
            .iter()
            .find(|row| row.label == "结束原因")
            .expect("stop row");
        assert!(stop.t03.contains("出错：provider timeout"));
        assert!(stop.t03.contains("输出被截断"));
        assert!(report.efficiency_rows[2].t03.contains("失败 1"));
    }
}
