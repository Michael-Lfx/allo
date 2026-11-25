#![allow(clippy::wildcard_imports)]
use super::contract::*;
use super::ooxml::*;
use super::provenance::evaluate_provenance;
use super::*;
use std::io::Write;
use zip::write::SimpleFileOptions;

fn anchor(name: &str, value: &str) -> DeckAnchor {
    DeckAnchor {
        anchor: name.to_owned(),
        value: value.to_owned(),
    }
}

fn pair(name: &str, value: &str) -> (String, String) {
    (name.to_owned(), value.to_owned())
}

fn write_zip(path: &Path, entries: &[(&str, &str)]) {
    let file = std::fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    for (name, body) in entries {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(body.as_bytes()).unwrap();
    }
    writer.finish().unwrap();
}

fn tool_context(root: &Path) -> ToolRunContext {
    ToolRunContext {
        signal: flowy_agent_core::CancelToken::new(),
        agent_id: Some("s-verify".to_owned()),
        cwd: Some(root.display().to_string()),
        todos: None,
        submission_id: Some("sub-verify".to_owned()),
        session_origin: flowy_agent_core::SessionOrigin::Home,
        output_root: None,
        artifact_delivery_available: true,
    }
}

#[test]
fn every_code_is_reported_distinctly() {
    let deck = vec![
        pair("S0-title", "T"),
        pair("S0-title", "T"),
        pair("S0-extra", "x"),
    ];
    let expected = vec![
        anchor("S0-title", "T"),
        anchor("S0-body", "one"),
        anchor("S0-value", "wrong"),
    ];
    let report = evaluate("deck.pptx", 2, 1, &deck, &expected);
    assert!(!report.ok);
    let codes: Vec<&str> =
        report.issues.iter().map(|issue| issue.code).collect();
    assert!(codes.contains(&STRUCTURE));
    assert!(codes.contains(&ANCHOR_DUPLICATE));
    assert!(codes.contains(&ANCHOR_MISSING));
    assert!(codes.contains(&ANCHOR_INVALID));
}

#[test]
fn brief_contract_extracts_counts_and_skips_ordinals() {
    let east = BriefContract::from_brief("请做一份3页华东区汇报 q3-east.pptx");
    assert_eq!(east.min_slides, Some(3));
    assert_eq!(east.expected_filenames, vec!["q3-east.pptx"]);

    assert_eq!(
        BriefContract::from_brief("制作两页总结").min_slides,
        Some(2)
    );
    assert_eq!(
        BriefContract::from_brief("Create a 4-slide review").min_slides,
        Some(4)
    );
    assert_eq!(
        BriefContract::from_brief("把第3页标题改成Q4").min_slides,
        None
    );
}

#[tokio::test]
async fn shrinking_deck_below_brief_min_slides_fails_even_when_worker_lowers_declared_slides()
 {
    let root = std::env::temp_dir()
        .join(format!("flowy-verify-anticheat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let deck = root.join("q3-east.pptx");
    crate::pptx::write_deck(
        &deck,
        "华东区Q3汇报",
        &["上海6.1亿".to_owned(), "江苏4.2亿".to_owned()],
    )
    .unwrap();
    let store = DeliveryStore::new(root.join("delivery"));
    let contract = BriefContract::from_brief(
        "请生成一份3页华东区汇报 q3-east.pptx 并核验后交付",
    );
    let tool = definition_with_semantic_and_contract(
        Some(store.clone()),
        None,
        Some(contract),
    );
    let args = ToolCallArgs {
        call_id: "call-cheat".to_owned(),
        name: "verify_deck".to_owned(),
        arguments: json!({
            "path": "q3-east.pptx",
            "slides": 2,
            "anchors": [
                {"anchor": "S0-title", "value": "华东区Q3汇报"},
                {"anchor": "S0-body", "value": "上海6.1亿"},
                {"anchor": "S1-title", "value": "华东区Q3汇报 · 2"},
                {"anchor": "S1-body", "value": "江苏4.2亿"},
            ],
        }),
    };
    let ToolExecutionResult::Success { value, .. } =
        (tool.execute)(args, tool_context(&root)).await
    else {
        panic!("failed verification must return a structured report");
    };
    assert_eq!(value["ok"], false);
    assert_eq!(value["next_action"], "clarify_or_rebuild");
    let issues = value["issues"].as_array().unwrap();
    assert!(issues.iter().any(|item| {
        item["code"] == STRUCTURE && item["field"] == "brief_min_slides"
    }));
    assert_eq!(
        store.check(&deck).unwrap(),
        Some(delivery::BlockReason::VerificationFailed)
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn docx_and_xlsx_verifiers_pass_valid_ooxml_and_block_broken_archives() {
    let root = std::env::temp_dir()
        .join(format!("flowy-verify-ooxml-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let store = DeliveryStore::new(root.join("delivery"));

    let docx = root.join("memo.docx");
    write_zip(
        &docx,
        &[(
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="0" w:name="summary"/><w:r><w:t>Q3 revenue &amp; growth</w:t></w:r></w:p></w:body></w:document>"#,
        )],
    );
    let xlsx = root.join("sales.xlsx");
    write_zip(
        &xlsx,
        &[
            ("xl/workbook.xml", "<workbook/>"),
            (
                "xl/sharedStrings.xml",
                "<sst><si><t>Region</t></si><si><t>Revenue</t></si><si><t>East</t></si></sst>",
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>6.1</v></c></row></sheetData></worksheet>"#,
            ),
        ],
    );
    let broken = root.join("broken.xlsx");
    std::fs::write(&broken, b"hello").unwrap();

    let tool = artifact_definition(Some(store.clone()), None);
    let docx_res = (tool.execute)(
            ToolCallArgs {
                call_id: "c-docx".to_owned(),
                name: "verify_artifact".to_owned(),
                arguments: json!({
                    "path": "memo.docx",
                    "anchors": [{"anchor": "summary", "value": "Q3 revenue & growth"}],
                }),
            },
            tool_context(&root),
        )
        .await;
    let ToolExecutionResult::Success { value: dv, .. } = docx_res else {
        panic!("docx verification failed");
    };
    assert_eq!(dv["ok"], true);
    assert_eq!(dv["anchor"], "bookmark");
    assert_eq!(store.check(&docx).unwrap(), None);

    let xlsx_res = (tool.execute)(
        ToolCallArgs {
            call_id: "c-xlsx".to_owned(),
            name: "verify_artifact".to_owned(),
            arguments: json!({
                "path": "sales.xlsx",
                "anchors": [
                    {"anchor": "East:Revenue", "value": "6.1"},
                    {"anchor": "B2", "value": "6.1"},
                    {"anchor": "East", "value": "East | 6.1"},
                ],
            }),
        },
        tool_context(&root),
    )
    .await;
    let ToolExecutionResult::Success { value: xv, .. } = xlsx_res else {
        panic!("xlsx verification failed");
    };
    assert_eq!(xv["ok"], true);
    assert_eq!(xv["anchor"], "row_key");
    assert_eq!(store.check(&xlsx).unwrap(), None);

    let broken_res = (tool.execute)(
        ToolCallArgs {
            call_id: "c-broken".to_owned(),
            name: "verify_artifact".to_owned(),
            arguments: json!({ "path": "broken.xlsx" }),
        },
        tool_context(&root),
    )
    .await;
    let ToolExecutionResult::Success { value: bv, .. } = broken_res else {
        panic!("broken archive must return structured failed report");
    };
    assert_eq!(bv["ok"], false);
    assert_eq!(
        store.check(&broken).unwrap(),
        Some(delivery::BlockReason::VerificationFailed)
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn followup_contract_inherits_slides_and_quote_while_updating_date() {
    let root = std::env::temp_dir()
        .join(format!("flowy-verify-followup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let store = DeliveryStore::new(root.join("delivery"));

    // Reference document count ("参考那份20页的旧报告") must not override target "3页".
    let rev1 = BriefContract::from_brief(
        "参考那份20页的旧报告，请做报价表＋3页汇报，报价 35 万，日期 2026-11-25",
    );
    assert_eq!(rev1.revision, 1);
    assert_eq!(rev1.min_slides, Some(3));
    assert_eq!(rev1.required_items.len(), 2);
    assert_eq!(
        rev1.pinned_facts.get("quote").map(String::as_str),
        Some("35万")
    );
    assert_eq!(
        rev1.pinned_facts.get("date").map(String::as_str),
        Some("2026-11-25")
    );

    // Turn 2 only edits the date: must keep min_slides=3, required_items=2, quote=35万.
    let rev2 = BriefContract::evolve(
        Some(&rev1.to_work_contract()),
        "只改一下日期为 2026-12-01",
    );
    assert_eq!(rev2.revision, 2);
    assert_eq!(rev2.min_slides, Some(3));
    assert_eq!(rev2.required_items.len(), 2);
    assert_eq!(
        rev2.pinned_facts.get("quote").map(String::as_str),
        Some("35万")
    );
    assert_eq!(
        rev2.pinned_facts.get("date").map(String::as_str),
        Some("2026-12-01")
    );

    let tool = definition_with_options(
        Some(store.clone()),
        None,
        Some(rev2.clone()),
        None,
    );

    // Wrong quote (38万 instead of inherited 35万) must fail VALUE_MISMATCH/brief_quote.
    let wrong_quote = root.join("wrong-quote.pptx");
    crate::pptx::write_deck(
        &wrong_quote,
        "项目汇报",
        &[
            "日期 2026-12-01".to_owned(),
            "报价 38万".to_owned(),
            "结论".to_owned(),
        ],
    )
    .unwrap();
    let ToolExecutionResult::Success { value: wq, .. } = (tool.execute)(
        ToolCallArgs {
            call_id: "c-wq".to_owned(),
            name: "verify_deck".to_owned(),
            arguments: json!({
                "path": "wrong-quote.pptx",
                "slides": 3,
                "anchors": [
                    {"anchor": "S0-title", "value": "项目汇报"},
                    {"anchor": "S0-body", "value": "日期 2026-12-01"},
                    {"anchor": "S1-title", "value": "项目汇报 · 2"},
                    {"anchor": "S1-body", "value": "报价 38万"},
                    {"anchor": "S2-title", "value": "项目汇报 · 3"},
                    {"anchor": "S2-body", "value": "结论"},
                ],
            }),
        },
        tool_context(&root),
    )
    .await
    else {
        panic!("expected structured report");
    };
    assert_eq!(wq["ok"], false);
    assert_eq!(wq["contract_revision"], 2);
    assert!(wq["issues"].as_array().unwrap().iter().any(|i| {
        i["code"] == VALUE_MISMATCH && i["field"] == "brief_quote"
    }));

    // Correct 3 slides + inherited 35万 + updated 2026-12-01 passes and tags rev=2.
    let good = root.join("good-rev2.pptx");
    crate::pptx::write_deck(
        &good,
        "项目汇报",
        &[
            "日期 2026-12-01".to_owned(),
            "报价 35万".to_owned(),
            "结论".to_owned(),
        ],
    )
    .unwrap();
    let ToolExecutionResult::Success { value: ok_val, .. } = (tool.execute)(
        ToolCallArgs {
            call_id: "c-ok".to_owned(),
            name: "verify_deck".to_owned(),
            arguments: json!({
                "path": "good-rev2.pptx",
                "slides": 3,
                "anchors": [
                    {"anchor": "S0-title", "value": "项目汇报"},
                    {"anchor": "S0-body", "value": "日期 2026-12-01"},
                    {"anchor": "S1-title", "value": "项目汇报 · 2"},
                    {"anchor": "S1-body", "value": "报价 35万"},
                    {"anchor": "S2-title", "value": "项目汇报 · 3"},
                    {"anchor": "S2-body", "value": "结论"},
                ],
            }),
        },
        tool_context(&root),
    )
    .await
    else {
        panic!("expected structured report");
    };
    assert_eq!(ok_val["ok"], true);
    assert_eq!(ok_val["contract_revision"], 2);
    assert_eq!(
        store
            .latest_report(&good)
            .unwrap()
            .unwrap()
            .contract_revision,
        Some(2)
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn empty_anchors_blocked_and_multisheet_xlsx_isolates_headers_and_formulas()
 {
    let root = std::env::temp_dir()
        .join(format!("flowy-verify-multisheet-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let store = DeliveryStore::new(root.join("delivery"));

    let multi = root.join("quote.xlsx");
    write_zip(
        &multi,
        &[
            (
                "xl/workbook.xml",
                r#"<workbook><sheets><sheet name="Summary" sheetId="1"/><sheet name="Detail" sheetId="2"/></sheets></workbook>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Item</t></is></c><c r="B1" t="inlineStr"><is><t>Price</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>Total</t></is></c><c r="B2"><f>SUM(Detail!B2:B3)</f></c></row></sheetData></worksheet>"#,
            ),
            (
                "xl/worksheets/sheet2.xml",
                r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>SKU</t></is></c><c r="B1" t="inlineStr"><is><t>Cost</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>A-1</t></is></c><c r="B2"><v>350000</v></c></row></sheetData></worksheet>"#,
            ),
        ],
    );

    let tool = artifact_definition(Some(store.clone()), None);
    // Empty anchors must be refused as STRUCTURE/anchors.
    let ToolExecutionResult::Success {
        value: empty_res, ..
    } = (tool.execute)(
        ToolCallArgs {
            call_id: "c-empty".to_owned(),
            name: "verify_artifact".to_owned(),
            arguments: json!({
                "path": "quote.xlsx",
                "anchors": [],
            }),
        },
        tool_context(&root),
    )
    .await
    else {
        panic!("expected structured failed report");
    };
    assert_eq!(empty_res["ok"], false);
    assert!(
        empty_res["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| { i["code"] == STRUCTURE && i["field"] == "anchors" })
    );

    // Sheet-qualified cells, isolated Sheet 2 headers, and formula cell without <v> all pass.
    let ToolExecutionResult::Success { value: ok_res, .. } = (tool.execute)(
        ToolCallArgs {
            call_id: "c-multi".to_owned(),
            name: "verify_artifact".to_owned(),
            arguments: json!({
                "path": "quote.xlsx",
                "anchors": [
                    {"anchor": "Summary!A1", "value": "Item"},
                    {"anchor": "Detail!A1", "value": "SKU"},
                    {"anchor": "Detail!A-1:Cost", "value": "350000"},
                    {"anchor": "Summary!B2", "value": "=SUM(Detail!B2:B3)"},
                ],
            }),
        },
        tool_context(&root),
    )
    .await
    else {
        panic!("expected structured report");
    };
    assert_eq!(ok_res["ok"], true, "got {ok_res}");
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn second_value_mismatch_locks_rerender_once_to_clarify_or_rebuild() {
    let root = std::env::temp_dir()
        .join(format!("flowy-verify-rerender-cap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let deck = root.join("cap.pptx");
    crate::pptx::write_deck(&deck, "T", &["actual".to_owned()]).unwrap();
    let failures: VerifyFailureTracker = Arc::new(Mutex::new(HashMap::new()));
    let tool = definition_with_options(None, None, None, Some(failures));
    let call = || ToolCallArgs {
        call_id: "c-cap".to_owned(),
        name: "verify_deck".to_owned(),
        arguments: json!({
            "path": "cap.pptx",
            "slides": 1,
            "anchors": [
                {"anchor": "S0-title", "value": "T"},
                {"anchor": "S0-body", "value": "expected-different"},
            ],
        }),
    };
    let ToolExecutionResult::Success { value: first, .. } =
        (tool.execute)(call(), tool_context(&root)).await
    else {
        panic!("first report");
    };
    assert_eq!(first["ok"], false);
    assert_eq!(first["next_action"], "rerender_once");

    let ToolExecutionResult::Success { value: second, .. } =
        (tool.execute)(call(), tool_context(&root)).await
    else {
        panic!("second report");
    };
    assert_eq!(second["ok"], false);
    assert_eq!(
        second["next_action"], "clarify_or_rebuild",
        "second failure on same artifact in one turn must lock next_action"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn provenance_missing_and_model_recall_warn_without_blocking() {
    let root = std::env::temp_dir()
        .join(format!("flowy-prov-warn-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let deck = root.join("deck.pptx");
    crate::pptx::write_deck(&deck, "T", &["body".to_owned()]).unwrap();
    let store = DeliveryStore::new(root.join("delivery"));
    let tool = definition(Some(store));

    // Omitting provenance warns but still passes.
    let ToolExecutionResult::Success { value: missing, .. } = (tool.execute)(
        ToolCallArgs {
            call_id: "c-prov-1".to_owned(),
            name: "verify_deck".to_owned(),
            arguments: json!({
                "path": "deck.pptx",
                "slides": 1,
                "anchors": [
                    {"anchor": "S0-title", "value": "T"},
                    {"anchor": "S0-body", "value": "body"},
                ],
            }),
        },
        tool_context(&root),
    )
    .await
    else {
        panic!("expected report");
    };
    assert_eq!(missing["ok"], true);
    assert_eq!(
        missing["provenance_warnings"],
        json!(["provenance_missing"])
    );

    // Explicit model_recall warns with field tag and still passes.
    let ToolExecutionResult::Success { value: recall, .. } = (tool.execute)(
        ToolCallArgs {
            call_id: "c-prov-2".to_owned(),
            name: "verify_deck".to_owned(),
            arguments: json!({
                "path": "deck.pptx",
                "slides": 1,
                "anchors": [
                    {"anchor": "S0-title", "value": "T"},
                    {"anchor": "S0-body", "value": "body"},
                ],
                "provenance": [
                    {"field": "S0-body", "kind": "model_recall"}
                ],
            }),
        },
        tool_context(&root),
    )
    .await
    else {
        panic!("expected report");
    };
    assert_eq!(recall["ok"], true);
    assert_eq!(
        recall["provenance_warnings"],
        json!(["model_recall:S0-body"])
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn provenance_user_file_validates_existence_confinement_and_sha256() {
    let root = std::env::temp_dir()
        .join(format!("flowy-prov-userfile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let ws = root.join("ws");
    let outside = root.join("outside.txt");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(&outside, b"secret outside").unwrap();
    let src = ws.join("source.csv");
    std::fs::write(&src, b"region,sales\nEast,12.4\n").unwrap();
    let src_hash = delivery::hash_bytes(b"region,sales\nEast,12.4\n");

    let deck = ws.join("deck.pptx");
    crate::pptx::write_deck(&deck, "T", &["body".to_owned()]).unwrap();
    let store = DeliveryStore::new(root.join("delivery"));
    let tool = definition(Some(store.clone()));

    // 1) Valid user_file with matching SHA-256 passes cleanly with 0 warnings.
    let ToolExecutionResult::Success { value: ok_res, .. } = (tool.execute)(
        ToolCallArgs {
            call_id: "c-ok".to_owned(),
            name: "verify_deck".to_owned(),
            arguments: json!({
                "path": "deck.pptx",
                "slides": 1,
                "anchors": [
                    {"anchor": "S0-title", "value": "T"},
                    {"anchor": "S0-body", "value": "body"},
                ],
                "provenance": [
                    {"field": "S0-body", "kind": "user_file", "path": "source.csv", "sha256": src_hash}
                ],
            }),
        },
        tool_context(&ws),
    )
    .await
    else {
        panic!("expected report");
    };
    assert_eq!(ok_res["ok"], true, "got {ok_res}");
    assert_eq!(ok_res["provenance_warnings"], json!([]));

    // 2) Missing file, tampered SHA-256, and escaping path all hard-block.
    for (case, prov) in [
        (
            "missing",
            json!([{"field": "S0-body", "kind": "user_file", "path": "no-such.csv"}]),
        ),
        (
            "bad-sha",
            json!([{"field": "S0-body", "kind": "user_file", "path": "source.csv", "sha256": "0000deadbeef"}]),
        ),
        (
            "escape",
            json!([{"field": "S0-body", "kind": "user_file", "path": "../outside.txt"}]),
        ),
    ] {
        let ToolExecutionResult::Success { value: blocked, .. } = (tool
            .execute)(
            ToolCallArgs {
                call_id: format!("c-{case}"),
                name: "verify_deck".to_owned(),
                arguments: json!({
                    "path": "deck.pptx",
                    "slides": 1,
                    "anchors": [
                        {"anchor": "S0-title", "value": "T"},
                        {"anchor": "S0-body", "value": "body"},
                    ],
                    "provenance": prov,
                }),
            },
            tool_context(&ws),
        )
        .await
        else {
            panic!("expected blocked report for {case}");
        };
        assert_eq!(blocked["ok"], false, "case {case} must fail: {blocked}");
        assert_eq!(blocked["next_action"], "clarify_or_rebuild");
        assert!(
            blocked["issues"].as_array().unwrap().iter().any(|issue| {
                issue["code"] == PROVENANCE_INVALID
                    && issue["field"] == "S0-body"
            }),
            "case {case} must emit PROVENANCE_INVALID: {blocked}"
        );
    }

    // DeliveryStore must also block delivery because the latest persisted report failed.
    let delivery = store
        .deliver_with(&deck, false, 100, |_| Ok(()))
        .expect("deliver_with");
    assert!(!delivery.outcome.delivered);
    assert_eq!(
        delivery.outcome.blocked,
        Some(delivery::BlockReason::VerificationFailed)
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn followup_contract_handles_quote_date_rename_input_sources_and_chinese_teens()
{
    // 1) Input source file "根据 source.csv 做 q3.pptx" must NOT require source.csv.
    let c1 = BriefContract::from_brief(
        "根据 source.csv 做一份 十五页 PPT 叫 q3.pptx，报价 35 万，日期 2026-11-25",
    );
    assert_eq!(c1.min_slides, Some(15));
    assert_eq!(c1.expected_filenames, vec!["q3.pptx"]);
    assert_eq!(
        c1.pinned_facts.get("quote").map(String::as_str),
        Some("35万")
    );
    assert_eq!(
        c1.pinned_facts.get("date").map(String::as_str),
        Some("2026-11-25")
    );

    // Ordinal "第十二页" must NOT be extracted as 2 slides or 12 slides.
    let ord = BriefContract::from_brief("修改第十二页标题即可");
    assert_eq!(ord.min_slides, None);

    // 2) Multi-turn quote & date updates + rename replace old values instead of
    //    keeping stale quote/date/filename.
    let w1 = c1.to_work_contract();
    let c2 = BriefContract::evolve(
        Some(&w1),
        "把报价改成 40 万，日期从 2026-11-25 改成 2026-12-01，并改名为 q4.pptx",
    );
    assert_eq!(c2.revision, 2);
    assert_eq!(c2.min_slides, Some(15));
    assert_eq!(c2.expected_filenames, vec!["q4.pptx"]);
    assert_eq!(
        c2.pinned_facts.get("quote").map(String::as_str),
        Some("40万")
    );
    assert_eq!(
        c2.pinned_facts.get("date").map(String::as_str),
        Some("2026-12-01")
    );

    // 3) Quote boundary matching must not false-match "135万" or "3500000" for "35万".
    let mut issues = Vec::new();
    c1.check_pinned_facts("报价 135万，日期 2026-11-25", &mut issues);
    assert!(
        issues.iter().any(|i| i.field == "brief_quote"),
        "35万 must not match 135万: {issues:?}"
    );
    let mut ok_issues = Vec::new();
    c1.check_pinned_facts("报价 35万，日期 2026-11-25", &mut ok_issues);
    assert!(ok_issues.is_empty(), "exact quote must pass: {ok_issues:?}");
}

#[test]
fn provenance_invalid_maps_to_dedicated_ui_key_and_fails_closed_without_cwd() {
    assert_eq!(ui_key(PROVENANCE_INVALID), "verify.provenance_invalid");
    let eval = evaluate_provenance(
        &[ProvenanceClaim {
            field: "S0-body".to_owned(),
            kind: super::provenance::ProvenanceKind::UserFile,
            path: Some("data.csv".to_owned()),
            sha256: None,
            note: None,
        }],
        None,
    );
    assert!(
        eval.issues.iter().any(|i| i.code == PROVENANCE_INVALID),
        "user_file provenance with cwd=None must fail closed: {eval:?}"
    );
}

#[test]
fn round2_contract_edge_cases_quotes_dates_renames_and_slide_reduction() {
    // 1. Thousand-separated quotes, trailing sentence period, and trailing non-currency numbers
    assert_eq!(
        extract_quote_fact("报价 350,000 元").as_deref(),
        Some("350000元")
    );
    assert_eq!(
        extract_quote_fact("Update price to 400000.").as_deref(),
        Some("400000")
    );
    assert_eq!(
        extract_quote_fact("报价 35 万（有效期至 2026-12-01）").as_deref(),
        Some("35万")
    );
    assert_eq!(
        extract_quote_fact("报价 35 万含 2 年维保，分 3 期付款").as_deref(),
        Some("35万")
    );
    // 2. `.00` currency and sentence-ending amounts match; `.5` still rejected
    assert!(quote_matches(&compact_text("总价 ¥350,000.00"), "35万"));
    assert!(quote_matches(
        &compact_text("Total price is 350000."),
        "35万"
    ));
    assert!(!quote_matches(&compact_text("总价 ¥350,000.50"), "35万"));
    // 3. Date boundary prevents `2026-12-01` from falsely matching `2026-12-15`
    assert!(!date_matches(
        &compact_text("日期 2026-12-15"),
        "2026-12-01"
    ));
    assert!(date_matches(
        &compact_text("Date: 2026-12-01."),
        "2026-12-01"
    ));
    // 4. Word-bounded input/output markers & targeted rename
    let spreadsheet =
        BriefContract::from_brief("Make a spreadsheet named q3.xlsx");
    assert_eq!(spreadsheet.expected_filenames, vec!["q3.xlsx".to_owned()]);
    let read_named =
        BriefContract::from_brief("读取名为 source.csv 的表格生成 q3.pptx");
    assert_eq!(read_named.expected_filenames, vec!["q3.pptx".to_owned()]);
    let rename_from = BriefContract::evolve(
        Some(&read_named.to_work_contract()),
        "把文件名从 q3.pptx 改成 q4.pptx",
    );
    assert_eq!(rename_from.expected_filenames, vec!["q4.pptx".to_owned()]);
    let multi = BriefContract::from_brief("生成 east.pptx 和 west.pptx");
    let renamed_one = BriefContract::evolve(
        Some(&multi.to_work_contract()),
        "把 west.pptx 改名为 west-v2.pptx",
    );
    assert_eq!(
        renamed_one.expected_filenames,
        vec!["east.pptx".to_owned(), "west-v2.pptx".to_owned()]
    );
    let paren_rename = BriefContract::evolve(
        Some(&read_named.to_work_contract()),
        "改名为 q4.pptx（原 q3.pptx）",
    );
    assert_eq!(paren_rename.expected_filenames, vec!["q4.pptx".to_owned()]);
    // 5. Slide-count reduction picks the target count
    let reduced = BriefContract::from_brief("把 5 页改成 3 页");
    assert_eq!(reduced.min_slides, Some(3));
}

#[test]
fn round4_laoyeye_adversarial_contract_and_pptx_preflight_cases() {
    // 1. Mixed Chinese + ASCII filename & pure Chinese filename
    let mixed = BriefContract::from_brief("帮我做一份季度汇报_2026.pptx");
    assert_eq!(
        mixed.expected_filenames,
        vec!["季度汇报_2026.pptx".to_owned()]
    );
    let cn_only =
        BriefContract::from_brief("打开 data.xlsx 看一下数据，写一份总结.pptx");
    assert_eq!(cn_only.expected_filenames, vec!["总结.pptx".to_owned()]);
    let uploaded = BriefContract::from_brief("我上传了 raw.csv，帮我做PPT");
    assert!(uploaded.expected_filenames.is_empty());

    // 2. Non-PPT "页" and upper bounds never fabricate phantom `.pptx` or inflate `min_slides`
    let word_doc =
        BriefContract::from_brief("帮我写一份5页的Word报告 report.docx");
    assert_eq!(word_doc.min_slides, None);
    assert_eq!(word_doc.expected_filenames, vec!["report.docx".to_owned()]);
    assert!(
        word_doc
            .required_items
            .iter()
            .all(|i| i.extension != "pptx")
    );
    let distilled =
        BriefContract::from_brief("把附件里的15页财报提炼成一份5页PPT");
    assert_eq!(distilled.min_slides, Some(5));
    let upper_bound =
        BriefContract::from_brief("做一个3页以内的PPT，不要超过5页");
    assert_eq!(upper_bound.min_slides, None);

    // 3. Multi-keyword quote contrast, `元` equivalence, English non-currency filter, and `of`/currency prefixes
    assert_eq!(
        extract_quote_fact("去年预算20万，今年预算改为50万").as_deref(),
        Some("50万")
    );
    assert_eq!(extract_quote_fact("Quote: 3 options"), None);
    assert_eq!(
        extract_quote_fact("Create a quote of $350,000").as_deref(),
        Some("350000")
    );
    assert_eq!(
        extract_quote_fact("报价 RMB 350,000").as_deref(),
        Some("350000")
    );
    assert!(quote_matches(&compact_text("报价：¥350,000"), "350000元"));
    assert!(quote_matches(&compact_text("总价 35万"), "350000元"));

    // 4. Date ranges are not pinned; dotted & English month formats match
    assert_eq!(
        extract_date_fact("复盘2025-01-01上线以来到2026-06-30的增长"),
        None
    );
    assert!(date_matches(
        &compact_text("September 15, 2026"),
        "2026-09-15"
    ));
    assert!(date_matches(&compact_text("2026.09.15"), "2026-09-15"));

    // 5. Explicit revocation in `BriefContract::evolve`
    let base = BriefContract::from_brief(
        "做 4 页 PPT 和一份报价表，报价 35 万，日期 2026-11-25",
    );
    let revoked = BriefContract::evolve(
        Some(&base.to_work_contract()),
        "去掉具体日期，不用做报价表了，只做PPT",
    );
    assert!(!revoked.pinned_facts.contains_key("date"));
    assert_eq!(
        revoked.pinned_facts.get("quote").map(String::as_str),
        Some("35万")
    );
    assert!(revoked.required_items.iter().all(|i| i.extension != "xlsx"));

    // 6. PPTX ZIP-bomb preflight rejects oversized XML entries before rpptx parse
    let dir = std::env::temp_dir()
        .join(format!("flowy-pptx-bomb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bomb = dir.join("bomb.pptx");
    let huge_xml = format!("<root>{}</root>", " ".repeat(17 * 1024 * 1024));
    write_zip(&bomb, &[("ppt/slides/slide1.xml", &huge_xml)]);
    let err = verify_deck_file_with_shapes(&bomb, 1, &[], None)
        .err()
        .expect(
            "oversized XML inside pptx must be rejected before rpptx parse",
        );
    assert!(
        err.contains("budget") || err.contains("limit"),
        "unexpected err: {err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
