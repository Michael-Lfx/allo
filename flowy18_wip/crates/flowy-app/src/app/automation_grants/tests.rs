use super::*;
use flowy_automation::GenerationEpoch;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug)]
pub struct TestRoot(PathBuf);

impl TestRoot {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "flowy-grant-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    panic!("create isolated grant directory: {error}")
                }
            }
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn request() -> GrantRequest {
    GrantRequest {
        principal: PrincipalId::cloud("account.example", 23).unwrap(),
        account_generation: AccountGeneration {
            epoch: GenerationEpoch::generate(),
            sequence: 1,
        },
        request_key: RequestKey::parse("host-issued-command").unwrap(),
        content_digest: [7; 32],
        expected_revision: Revision::new(2),
        requested_at_ms: 10,
        request_expires_at_ms: 100,
        grant_expires_at_ms: 1_000,
        scope: CapabilityRequest {
            tools: vec!["read_file".into()],
            resources: vec!["project-a".into()],
            grant_ref: None,
        },
    }
}

fn approve(ledger: &GrantLedger, request: &GrantRequest) -> GrantBinding {
    let GrantStatus::Approved(binding) =
        ledger.decide(request.clone(), true, 20).unwrap()
    else {
        panic!("expected approval");
    };
    binding
}

#[test]
fn absent_ledger_never_supplies_an_implicit_grant() {
    let root = TestRoot::new();
    let ledger = GrantLedger::open(root.path()).unwrap();
    let request = request();
    assert_eq!(ledger.inventory().len(), 0);
    assert_eq!(
        ledger.status_for_request(request.clone(), 20).unwrap(),
        None,
    );
    assert!(matches!(
        ledger.binding(&request.principal, &GrantId::generate(), 20),
        Err(GrantError::UnknownGrant),
    ));
    assert!(!root.path().join(RELATIVE_PATH).exists());
}

#[test]
fn concurrent_duplicate_approvals_persist_one_uuid_and_decision() {
    let root = TestRoot::new();
    let ledger = GrantLedger::open(root.path()).unwrap();
    let request = request();
    let threads = (0..8)
        .map(|_| {
            let ledger = ledger.clone();
            let request = request.clone();
            std::thread::spawn(move || approve(&ledger, &request))
        })
        .collect::<Vec<_>>();
    let mut bindings = Vec::new();
    for thread in threads {
        bindings.push(thread.join().unwrap());
    }
    let expected = bindings.first().unwrap();
    assert!(bindings.iter().all(|binding| binding == expected));
    assert!(GrantId::parse(expected.grant_id.as_str()).is_ok());
    assert_eq!(ledger.inventory().len(), 1);
    let reopened = GrantLedger::open(root.path()).unwrap();
    assert_eq!(
        reopened.status_for_request(request, 200).unwrap(),
        Some(GrantStatus::Approved(expected.clone())),
    );
}

#[test]
fn approval_request_binding_is_immutable_and_denial_cannot_be_overwritten() {
    let root = TestRoot::new();
    let ledger = GrantLedger::open(root.path()).unwrap();
    let request = request();
    approve(&ledger, &request);
    let mut changed = request.clone();
    changed.content_digest[0] ^= 1;
    assert!(matches!(
        ledger.decide(changed, true, 30),
        Err(GrantError::RequestConflict),
    ));
    let mut changed = request.clone();
    changed.expected_revision = Revision::new(3);
    assert!(matches!(
        ledger.status_for_request(changed, 30),
        Err(GrantError::RequestConflict),
    ));
    let mut changed = request.clone();
    changed.grant_expires_at_ms += 1;
    assert!(matches!(
        ledger.decide(changed, true, 30),
        Err(GrantError::RequestConflict),
    ));
    let mut changed = request.clone();
    changed.account_generation.sequence += 1;
    assert!(matches!(
        ledger.decide(changed, true, 30),
        Err(GrantError::RequestConflict),
    ));
    assert!(matches!(
        ledger.decide(request.clone(), false, 30),
        Err(GrantError::RequestConflict),
    ));
    let mut denied = request;
    denied.request_key = RequestKey::parse("declined-command").unwrap();
    assert_eq!(
        ledger.decide(denied.clone(), false, 20).unwrap(),
        GrantStatus::Denied,
    );
    assert!(matches!(
        ledger.decide(denied.clone(), true, 30),
        Err(GrantError::RequestConflict),
    ));
    assert_eq!(
        GrantLedger::open(root.path())
            .unwrap()
            .status_for_request(denied, 40)
            .unwrap(),
        Some(GrantStatus::Denied),
    );
}

#[test]
fn revocation_survives_reopen_and_old_approval_replay() {
    let root = TestRoot::new();
    let ledger = GrantLedger::open(root.path()).unwrap();
    let request = request();
    let binding = approve(&ledger, &request);
    assert!(
        ledger
            .revoke(&request.principal, &binding.grant_id, 30)
            .unwrap()
    );
    assert!(
        !ledger
            .revoke(&request.principal, &binding.grant_id, 40)
            .unwrap()
    );
    assert!(matches!(
        ledger.check(&request.principal, &binding, &request.scope, 40),
        Err(GrantError::Revoked),
    ));
    let reopened = GrantLedger::open(root.path()).unwrap();
    assert_eq!(
        reopened.decide(request.clone(), true, 50).unwrap(),
        GrantStatus::Revoked,
    );
    assert!(matches!(
        reopened.binding(&request.principal, &binding.grant_id, 50),
        Err(GrantError::Revoked),
    ));
    assert_eq!(reopened.inventory().len(), 1);
}

#[test]
fn scope_and_version_checks_narrow_approved_capabilities() {
    let root = TestRoot::new();
    let ledger = GrantLedger::open(root.path()).unwrap();
    let request = request();
    let binding = approve(&ledger, &request);
    assert!(
        ledger
            .check(&request.principal, &binding, &request.scope, 30)
            .is_ok()
    );
    let mut requested = request.scope.clone();
    requested.tools.push("write_file".into());
    assert!(matches!(
        ledger.check(&request.principal, &binding, &requested, 30),
        Err(GrantError::ScopeDenied),
    ));
    requested = request.scope.clone();
    requested.resources.push("project-b".into());
    assert!(matches!(
        ledger.check(&request.principal, &binding, &requested, 30),
        Err(GrantError::ScopeDenied),
    ));
    requested = request.scope.clone();
    requested.grant_ref = Some(GrantId::generate());
    assert!(matches!(
        ledger.check(&request.principal, &binding, &requested, 30),
        Err(GrantError::ScopeDenied),
    ));
    let mut old_version = binding.clone();
    old_version.version += 1;
    assert!(matches!(
        ledger.check(&request.principal, &old_version, &request.scope, 30),
        Err(GrantError::VersionMismatch),
    ));
    let foreign = PrincipalId::cloud("account.example", 24).unwrap();
    assert!(matches!(
        ledger.binding(&foreign, &binding.grant_id, 30),
        Err(GrantError::UnknownGrant),
    ));
    assert!(matches!(
        ledger.revoke(&foreign, &binding.grant_id, 30),
        Err(GrantError::UnknownGrant),
    ));
}

#[test]
fn shells_and_recursive_management_are_never_implicitly_authorized() {
    let root = TestRoot::new();
    let ledger = GrantLedger::open(root.path()).unwrap();
    for tool in [
        "shell",
        "bash",
        "sh",
        "powershell",
        "pwsh",
        "automation_run_now",
    ] {
        let mut request = request();
        request.scope.tools = vec![tool.into()];
        assert!(matches!(
            ledger.decide(request, true, 20),
            Err(GrantError::ScopeDenied)
        ));
    }
    assert_eq!(ledger.inventory().len(), 0);
}

#[test]
fn expiry_observation_is_durable_even_after_wall_clock_rollback() {
    let root = TestRoot::new();
    let ledger = GrantLedger::open(root.path()).unwrap();
    let request = request();
    let binding = approve(&ledger, &request);
    assert!(matches!(
        ledger.binding(&request.principal, &binding.grant_id, 1_000),
        Err(GrantError::Expired),
    ));
    let reopened = GrantLedger::open(root.path()).unwrap();
    assert!(matches!(
        reopened.check(&request.principal, &binding, &request.scope, 30),
        Err(GrantError::Expired),
    ));
    assert_eq!(
        reopened.decide(request, true, 30).unwrap(),
        GrantStatus::Expired
    );
}

#[test]
fn approval_storage_failure_never_publishes_a_memory_only_grant() {
    let root = TestRoot::new();
    fs::write(root.path().join("automation"), b"occupied-parent").unwrap();
    let ledger = GrantLedger::open(root.path());
    // A non-directory parent is an unavailable store, not an absent ledger.
    assert!(matches!(ledger, Err(GrantError::Storage(_))));
    fs::remove_file(root.path().join("automation")).unwrap();
    let ledger = GrantLedger::open(root.path()).unwrap();
    fs::write(root.path().join("automation"), b"occupied-parent").unwrap();
    assert!(matches!(
        ledger.decide(request(), true, 20),
        Err(GrantError::Storage(_))
    ));
    assert_eq!(ledger.inventory().len(), 0);
    assert_eq!(
        fs::read(root.path().join("automation")).unwrap(),
        b"occupied-parent"
    );
    fs::remove_file(root.path().join("automation")).unwrap();
    approve(&ledger, &request());
    assert_eq!(ledger.inventory().len(), 1);
}

#[test]
fn failed_revocation_preserves_disk_and_fails_closed_for_new_dispatch() {
    let root = TestRoot::new();
    let ledger = GrantLedger::open(root.path()).unwrap();
    let request = request();
    let binding = approve(&ledger, &request);
    let original = fs::read(root.path().join(RELATIVE_PATH)).unwrap();
    fs::rename(
        root.path().join("automation"),
        root.path().join("preserved"),
    )
    .unwrap();
    fs::write(root.path().join("automation"), b"occupied-parent").unwrap();
    assert!(matches!(
        ledger.revoke(&request.principal, &binding.grant_id, 30),
        Err(GrantError::Storage(_)),
    ));
    assert!(!ledger.available());
    assert!(matches!(
        ledger.check(&request.principal, &binding, &request.scope, 40),
        Err(GrantError::Unavailable),
    ));
    assert_eq!(
        fs::read(root.path().join("preserved/grants.json")).unwrap(),
        original
    );
    // The error is not a successful durable revocation. Keep the old file
    // intact so the user can repair storage and explicitly retry it.
    fs::remove_file(root.path().join("automation")).unwrap();
    fs::rename(
        root.path().join("preserved"),
        root.path().join("automation"),
    )
    .unwrap();
    let reopened = GrantLedger::open(root.path()).unwrap();
    assert!(
        reopened
            .revoke(&request.principal, &binding.grant_id, 50)
            .unwrap()
    );
}

#[test]
fn invalid_or_future_ledgers_are_preserved_and_never_reinitialized() {
    let root = TestRoot::new();
    fs::create_dir(root.path().join("automation")).unwrap();
    for (bytes, future) in [
        (b"broken-json".as_slice(), false),
        (b"{\"format_version\":999,\"entries\":[]}".as_slice(), true),
    ] {
        fs::write(root.path().join(RELATIVE_PATH), bytes).unwrap();
        let result = GrantLedger::open(root.path());
        assert!(if future {
            matches!(result, Err(GrantError::UnsupportedVersion))
        } else {
            matches!(result, Err(GrantError::Corrupt))
        });
        assert_eq!(fs::read(root.path().join(RELATIVE_PATH)).unwrap(), bytes);
    }
}

#[test]
fn duplicate_and_invalid_durable_decisions_are_rejected() {
    let root = TestRoot::new();
    let ledger = GrantLedger::open(root.path()).unwrap();
    approve(&ledger, &request());
    let bytes = fs::read(root.path().join(RELATIVE_PATH)).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let mut duplicate = value.clone();
    let entries = duplicate["entries"].as_array_mut().unwrap();
    entries.push(entries.first().unwrap().clone());
    assert!(matches!(
        decode(&serde_json::to_vec(&duplicate).unwrap()),
        Err(GrantError::Corrupt)
    ));
    let mut invalid = value.clone();
    invalid["entries"][0]["decision"]["version"] = serde_json::json!(9);
    assert!(matches!(
        decode(&serde_json::to_vec(&invalid).unwrap()),
        Err(GrantError::Corrupt)
    ));
    let mut invalid = value;
    invalid["entries"][0]["request"]["scope"]["tools"] =
        serde_json::json!(["shell"]);
    assert!(matches!(
        decode(&serde_json::to_vec(&invalid).unwrap()),
        Err(GrantError::Corrupt)
    ));
}

#[test]
fn expired_and_future_approval_callbacks_do_not_create_decisions() {
    let root = TestRoot::new();
    let ledger = GrantLedger::open(root.path()).unwrap();
    assert!(matches!(
        ledger.decide(request(), true, 100),
        Err(GrantError::RequestExpired)
    ));
    assert!(matches!(
        ledger.decide(request(), true, 9),
        Err(GrantError::InvalidRequest)
    ));
    assert_eq!(ledger.inventory().len(), 0);
}
