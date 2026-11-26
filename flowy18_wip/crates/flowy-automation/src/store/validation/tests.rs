use super::*;
use crate::store::records::*;
use crate::*;
use rusqlite::params;

fn fixture() -> (Connection, Vec<AttemptRecord>) {
    let mut connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(super::super::schema::SCHEMA_SQL)
        .unwrap();
    let principal = PrincipalId::cloud("validation.fixture", 7).unwrap();
    let automation_id = AutomationId::generate();
    let revision = Revision::new(1).unwrap();
    let definition = ManualAutomationInput {
        schedule: None,
        title: PrivateText::new("validation fixture"),
        action: ActionSpec::AgentTask {
            prompt: PrivateText::new("fixture prompt"),
        },
        workspace: None,
        capabilities: CapabilityRequest::default(),
        model: Some(ModelBinding {
            selected: ModelRef {
                provider: "script".into(),
                model: "fixture".into(),
            },
            allowed_fallbacks: vec![],
        }),
        budget: RunBudget::new(1_000, 2, 3),
    };
    let automation = AutomationRecord {
        automation_id: automation_id.clone(),
        principal: principal.clone(),
        current_revision: revision,
        state: DefinitionState::Enabled,
        title: definition.title.clone(),
        created_at_ms: 0,
        updated_at_ms: 0,
        source_removed: false,
    };
    let revision_record = RevisionRecord {
        automation_id: automation_id.clone(),
        principal: principal.clone(),
        revision,
        definition: definition.clone(),
        context: RevisionContext::default(),
        grant_ref: None,
        created_at_ms: 0,
    };
    let mut mutations = vec![StoreMutation::CreateAutomation {
        automation,
        revision: revision_record,
    }];
    let mut attempts = Vec::new();
    for number in 0..2 {
        let run_id = RunId::generate();
        let run = RunRecord {
            run_id: run_id.clone(),
            automation_id: automation_id.clone(),
            revision,
            principal: principal.clone(),
            current_attempt_id: None,
            retry_of_run_id: None,
            occurrence_key: None,
            phase: RunPhase::Queued,
            outcome: None,
            latest_start_at_ms: 1_000,
            source_removed: false,
            spec: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        let spec = RunSpec {
            automation_id: automation_id.clone(),
            run_id: run_id.clone(),
            attempt_id: AttemptId::generate(),
            revision,
            principal: principal.clone(),
            account_generation: AccountGeneration {
                epoch: GenerationEpoch::generate(),
                sequence: 1,
            },
            session_id: Some(
                SessionRef::parse(format!("validation-session-{number}"))
                    .unwrap(),
            ),
            submission_id: Some(
                SubmissionRef::parse(format!("validation-submission-{number}"))
                    .unwrap(),
            ),
            source_session_id: None,
            action: definition.action.clone(),
            context_summary: None,
            skill_refs: vec![],
            workspace: None,
            capabilities: definition.capabilities.clone(),
            model: definition.model.clone(),
            credential_ref: Some(CredentialRef {
                principal: principal.clone(),
                provider: "script".into(),
                scope_ref: "fixture".into(),
                credential_key: "reference-only".into(),
            }),
            budget: definition.budget.clone(),
            latest_start_at_ms: 1_000,
        };
        let attempt = AttemptRecord {
            spec,
            started_at_ms: 10,
            completion: None,
        };
        mutations.extend([
            StoreMutation::InsertRun(run),
            StoreMutation::InsertAttempt(attempt.clone()),
            StoreMutation::SetCurrentAttempt {
                run_id,
                expected: None,
                attempt_id: attempt.spec.attempt_id.clone(),
            },
        ]);
        attempts.push(attempt);
    }
    let run_id = attempts[0].spec.run_id.clone();
    mutations.extend([
        StoreMutation::InsertReceipt(ReceiptRecord {
            principal: principal.clone(),
            envelope: CommandEnvelope {
                request_key: RequestKey::parse("validation-request").unwrap(),
                issued_at_ms: 0,
                expires_at_ms: 1_000,
                content_digest: [0; 32],
                command: AutomationCommand::CancelRun {
                    run_id: run_id.clone(),
                },
            },
            state: ReceiptState::Applied,
            outcome: Some(CommandOutcome::CancelRequested {
                run_id: run_id.clone(),
                disposition: CancelDisposition::Accepted,
            }),
            source_removed: false,
            created_at_ms: 0,
            updated_at_ms: 0,
        }),
        StoreMutation::InsertOutbox(OutboxRecord {
            principal: principal.clone(),
            delivery_key: "validation-delivery".into(),
            run_id: run_id.clone(),
            target: "card".into(),
            message_kind: "result".into(),
            state: OutboxState::Pending,
            payload: PrivateText::new("fixture result"),
            next_attempt_at_ms: 0,
            attempt_count: 0,
            created_at_ms: 0,
            updated_at_ms: 0,
        }),
        StoreMutation::InsertDeletionPlan(DeletionPlanRecord {
            deletion_plan_id: DeletionPlanId::generate(),
            principal: principal.clone(),
            run_id,
            state: DeletionPlanState::Planned,
            plan: DeletionTargets::default(),
            created_at_ms: 0,
            updated_at_ms: 0,
        }),
    ]);
    repository::commit(&mut connection, &principal, mutations).unwrap();
    assert_eq!(validate(&connection).unwrap().as_slice(), []);
    (connection, attempts)
}

fn side(store: SideStore, attempt: &AttemptRecord) -> SideRecord {
    let spec = &attempt.spec;
    SideRecord {
        store,
        automation_owned: true,
        principal: Some(spec.principal.clone()),
        automation_id: Some(spec.automation_id.clone()),
        run_id: Some(spec.run_id.clone()),
        attempt_id: Some(spec.attempt_id.clone()),
        session_id: spec.session_id.clone(),
        submission_id: spec.submission_id.clone(),
        grant_id: None,
        source_removed: false,
    }
}

#[test]
fn syntactically_valid_wrong_json_types_fail_full_typed_validation() {
    for mutation in [
        "UPDATE automations SET title_json='7'",
        "UPDATE automation_revisions SET context_json='{}'",
        "UPDATE runs SET spec_json='{}'",
        "UPDATE run_attempts SET completion_json='{}'",
        "UPDATE command_receipts SET command_json='{}'",
        "UPDATE delivery_outbox SET payload_json='{}'",
        "UPDATE deletion_plans SET plan_json='{}'",
    ] {
        let (connection, _) = fixture();
        // The table CHECK can already reject wrong SQLite-level shapes. For
        // object-shaped but invalid domain JSON, the typed audit must reject it.
        if connection.execute(mutation, []).is_ok() {
            if mutation.contains("context_json")
                || mutation.contains("plan_json")
            {
                // Empty context/targets are valid values. Exercise an actual
                // domain-type mismatch while preserving a JSON object.
                let bad = mutation.replace(
                    "'{}'",
                    "'{\"source_session_id\":7,\"sessions\":7}'",
                );
                connection.execute(&bad, []).unwrap();
            }
            assert!(validate(&connection).is_err(), "{mutation}");
        }
    }
}

#[test]
fn run_and_completion_json_cannot_change_the_bound_principal_or_turn() {
    let (connection, attempts) = fixture();
    let original = &attempts[0].spec;
    let mut altered = original.clone();
    altered.principal = PrincipalId::cloud("another-authority", 7).unwrap();
    altered.credential_ref.as_mut().unwrap().principal =
        altered.principal.clone();
    connection
        .execute(
            "UPDATE runs SET spec_json=?1 WHERE run_id=?2",
            params![
                repository::encode(&altered).unwrap(),
                original.run_id.as_str()
            ],
        )
        .unwrap();
    assert!(
        validate(&connection)
            .unwrap()
            .iter()
            .any(|finding| finding.code == "run_spec_binding_mismatch")
    );
    connection
        .execute("UPDATE runs SET spec_json=NULL", [])
        .unwrap();
    let completion = RunCompletion {
        execution_outcome: ExecutionOutcome::Completed,
        history_commit: HistoryCommitState::Committed,
        artifact_state: ArtifactState::NotRequired,
        commit_ref: Some(CommitRef {
            session_id: attempts[1].spec.session_id.clone().unwrap(),
            submission_id: attempts[1].spec.submission_id.clone().unwrap(),
        }),
        result_ref: None,
        error_code: None,
        stop_reason: None,
    };
    connection
        .execute(
            "UPDATE run_attempts SET completion_json=?1 WHERE attempt_id=?2",
            params![
                repository::encode(&completion).unwrap(),
                original.attempt_id.as_str()
            ],
        )
        .unwrap();
    assert!(validate(&connection).unwrap().iter().any(|finding| finding.code == "attempt_completion_commit_mismatch"));
}

#[test]
fn swapped_sessions_and_crossed_side_metadata_cannot_satisfy_separate_edges() {
    let (connection, attempts) = fixture();
    let history = attempts
        .iter()
        .map(|attempt| side(SideStore::AgentHistory, attempt))
        .collect::<Vec<_>>();
    let mut inventory = SideStoreInventory {
        completeness: vec![],
        records: history,
    };
    assert_eq!(
        validate_side_bindings(&connection, &inventory)
            .unwrap()
            .as_slice(),
        []
    );
    inventory.records[0].session_id = attempts[1].spec.session_id.clone();
    inventory.records[0].submission_id = attempts[1].spec.submission_id.clone();
    inventory.records[1].session_id = attempts[0].spec.session_id.clone();
    inventory.records[1].submission_id = attempts[0].spec.submission_id.clone();
    let findings = validate_side_bindings(&connection, &inventory).unwrap();
    assert_eq!(findings.iter().filter(|finding| finding.code == "history_attempt_binding_mismatch").count(), 2);
    inventory.records = attempts
        .iter()
        .map(|attempt| side(SideStore::AgentHistory, attempt))
        .collect();
    for store in [
        SideStore::AgentIndex,
        SideStore::ArtifactManifest,
        SideStore::PrivateWorkspace,
    ] {
        let mut crossed = side(store, &attempts[0]);
        crossed.run_id = Some(attempts[1].spec.run_id.clone());
        crossed.attempt_id = Some(attempts[1].spec.attempt_id.clone());
        inventory.records.push(crossed);
    }
    let findings = validate_side_bindings(&connection, &inventory).unwrap();
    assert_eq!(
        findings
            .iter()
            .filter(|finding| finding.code == "side_execution_binding_mismatch")
            .count(),
        3
    );
    inventory.records = vec![SideRecord {
        automation_owned: false,
        principal: None,
        ..side(SideStore::AgentHistory, &attempts[0])
    }];
    assert_eq!(
        validate_side_bindings(&connection, &inventory)
            .unwrap()
            .as_slice(),
        []
    );
}

#[test]
fn applied_receipt_and_current_title_must_match_their_frozen_targets() {
    let (connection, attempts) = fixture();
    let command = AutomationCommand::CancelRun {
        run_id: attempts[1].spec.run_id.clone(),
    };
    connection
        .execute(
            "UPDATE command_receipts SET command_json=?1",
            [repository::encode(&command).unwrap()],
        )
        .unwrap();
    assert!(
        validate(&connection)
            .unwrap()
            .iter()
            .any(|finding| finding.code == "receipt_command_target_mismatch")
    );
    connection
        .execute(
            "UPDATE automations SET title_json=?1",
            [
                repository::encode(&PrivateText::new(
                    "changed without revision",
                ))
                .unwrap(),
            ],
        )
        .unwrap();
    assert!(
        validate(&connection)
            .unwrap()
            .iter()
            .any(|finding| finding.code == "current_revision_title_mismatch")
    );
}
