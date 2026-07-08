//! Integration tests for Plan Mode prompts and file management (task 3.4).
//!
//! Tests are numbered to match the test-plan.md identifiers (TC-3.4-*).

use std::fs;
use std::path::Path;

use nomi_agent::context::{SystemPromptCache, build_system_prompt};
use nomi_agent::plan::file::{plan_file_path, read_plan, write_plan};
use nomi_agent::plan::prompt::plan_mode_instructions;

// ---------------------------------------------------------------------------
// TC-3.4-01  plan_mode_instructions content
// ---------------------------------------------------------------------------

#[test]
fn tc_3_4_01_instructions_not_empty() {
    let text = plan_mode_instructions();
    assert!(!text.is_empty(), "instructions should not be empty");
}

#[test]
fn tc_3_4_01_instructions_guide_code_reading() {
    let text = plan_mode_instructions();
    assert!(
        text.contains("Read") && text.contains("Grep") && text.contains("Glob"),
        "instructions should reference read-only tools"
    );
}

#[test]
fn tc_3_4_01_instructions_guide_plan_creation() {
    let text = plan_mode_instructions();
    // Should mention planning/design phases
    assert!(
        text.contains("plan") || text.contains("Plan"),
        "instructions should guide plan creation"
    );
}

#[test]
fn tc_3_4_01_instructions_mention_exit_tool() {
    let text = plan_mode_instructions();
    assert!(
        text.contains("ExitPlanMode"),
        "instructions should mention ExitPlanMode tool"
    );
}

#[test]
fn tc_3_4_01_instructions_forbid_writes() {
    let text = plan_mode_instructions();
    assert!(
        text.contains("MUST NOT") || text.contains("Forbidden"),
        "instructions should forbid write operations"
    );
}

// ---------------------------------------------------------------------------
// TC-3.4-03  Plan mode is NOT in system prompt (cache-stability invariant)
// ---------------------------------------------------------------------------
// Plan mode instructions now ride the turn tail (injected into the last user
// message by the engine) to keep the system prompt byte-stable for DeepSeek
// prefix caching.
// ---------------------------------------------------------------------------

#[test]
fn tc_3_4_03_plan_mode_not_in_system_prompt() {
    let result = build_system_prompt(
        &mut SystemPromptCache::new(),
        None,
        "/tmp",
        "test-model",
        &[],
        None,
        None,
        false,
        false,
    );

    // Plan mode instructions must NOT be in the system prompt — they ride
    // the turn tail to keep the prefix cache-stable.
    assert!(
        !result.contains("# Plan Mode"),
        "system prompt must NOT contain plan mode heading (it rides the turn tail)"
    );
    assert!(
        !result.contains("ExitPlanMode"),
        "system prompt must NOT reference ExitPlanMode tool"
    );
    assert!(
        !result.contains("MUST NOT"),
        "system prompt must NOT contain plan mode restrictions"
    );

    // Verify plan mode instructions still exist for turn-tail injection
    let plan_instructions = nomi_agent::plan::prompt::plan_mode_instructions();
    assert!(plan_instructions.contains("Plan Mode"));
    assert!(plan_instructions.contains("ExitPlanMode"));
}

// ---------------------------------------------------------------------------
// TC-3.4-04  System prompt without plan mode
// ---------------------------------------------------------------------------

#[test]
fn tc_3_4_04_system_prompt_excludes_plan_instructions_when_inactive() {
    let result = build_system_prompt(
        &mut SystemPromptCache::new(),
        None,
        "/tmp",
        "test-model",
        &[],
        None,
        None,
        false,
        false,
    );

    // Should NOT contain plan mode instructions
    assert!(
        !result.contains("# Plan Mode"),
        "system prompt should not contain plan mode heading"
    );
}

// ---------------------------------------------------------------------------
// TC-3.4-05  Plan file write
// ---------------------------------------------------------------------------

#[test]
fn tc_3_4_05_write_plan_creates_file_and_parents() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("deep").join("nested").join("plan.md");

    write_plan(&path, "# My Plan\n\n## Steps\n1. Do thing").unwrap();

    assert!(path.exists(), "plan file should be created");
    let content = fs::read_to_string(&path).unwrap();
    assert_eq!(content, "# My Plan\n\n## Steps\n1. Do thing");
}

// ---------------------------------------------------------------------------
// TC-3.4-06  Plan file read
// ---------------------------------------------------------------------------

#[test]
fn tc_3_4_06_read_plan_returns_content() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("plan.md");
    fs::write(&path, "# My Plan\nStep 1").unwrap();

    let result = read_plan(&path).unwrap();
    assert_eq!(result, Some("# My Plan\nStep 1".to_string()));
}

// ---------------------------------------------------------------------------
// TC-3.4-07  Plan file read when not exists
// ---------------------------------------------------------------------------

#[test]
fn tc_3_4_07_read_plan_nonexistent_returns_none() {
    let result = read_plan(Path::new("/nonexistent/path/plan.md")).unwrap();
    assert_eq!(result, None, "reading nonexistent plan should return None");
}

// ---------------------------------------------------------------------------
// TC-3.4-08  Plan file path generation
// ---------------------------------------------------------------------------

#[test]
fn tc_3_4_08_plan_file_path_format() {
    let path = plan_file_path(Path::new("/tmp/plans"), "session-abc");
    assert_eq!(
        path,
        std::path::PathBuf::from("/tmp/plans/session-abc.md"),
        "plan file path should be {{dir}}/{{session_id}}.md"
    );
}

// ---------------------------------------------------------------------------
// TC-3.4-09  No bb brand identifiers in plan mode instructions
// ---------------------------------------------------------------------------

#[test]
fn tc_3_4_09_no_bb_brand_in_instructions() {
    let text = plan_mode_instructions();
    assert!(
        !text.contains("Claude"),
        "instructions should not contain Claude brand"
    );
    assert!(
        !text.contains("claude"),
        "instructions should not contain lowercase claude"
    );
    assert!(
        !text.contains("~/.claude"),
        "instructions should not contain bb config path"
    );
}

// ---------------------------------------------------------------------------
// Additional: write-then-read roundtrip
// ---------------------------------------------------------------------------

#[test]
fn write_then_read_roundtrip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("plans");
    let path = plan_file_path(&dir, "test-session");

    let content = "# Implementation Plan\n\n## Context\nRefactor auth module\n\n## Files\n- src/auth.rs\n- src/middleware.rs";
    write_plan(&path, content).unwrap();

    let result = read_plan(&path).unwrap();
    assert_eq!(result, Some(content.to_string()));
}

// ---------------------------------------------------------------------------
// Additional: plan mode instructions NOT in system prompt (cache-stable)
// ---------------------------------------------------------------------------

#[test]
fn plan_mode_not_in_system_prompt_with_memory() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mem_dir = tmp.path().join("memory");
    std::fs::create_dir_all(&mem_dir).unwrap();
    std::fs::write(mem_dir.join("MEMORY.md"), "- [A](a.md) \u{2014} test\n").unwrap();

    let result = build_system_prompt(
        &mut SystemPromptCache::new(),
        None,
        "/tmp",
        "test-model",
        &[],
        None,
        Some(&mem_dir),
        false,
        false,
    );

    // Memory should be present
    let memory_pos = result
        .find("auto memory")
        .expect("memory section should be present");

    // Plan mode must NOT be in the system prompt (rides turn tail instead)
    assert!(
        !result.contains("# Plan Mode"),
        "system prompt must NOT contain plan mode instructions (cache-stable prefix)"
    );

    // Memory position is valid — just verify it's present
    assert!(memory_pos < result.len());
}

// ---------------------------------------------------------------------------
// Additional: write_plan overwrites existing
// ---------------------------------------------------------------------------

#[test]
fn write_plan_overwrites_existing_content() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("plan.md");

    write_plan(&path, "version 1").unwrap();
    write_plan(&path, "version 2").unwrap();

    let result = read_plan(&path).unwrap();
    assert_eq!(result, Some("version 2".to_string()));
}
