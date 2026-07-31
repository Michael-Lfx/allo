#![cfg(feature = "managed-search")]

use nomifun_ai_agent::ManagedWebHandle;

#[test]
fn managed_web_handle_constructs_offline() {
    let handle = ManagedWebHandle::keyless_default(false).expect("offline construction");
    let _provider = handle.search_provider();
    assert!(handle.extract_coordinator().is_none());
}

#[test]
fn managed_web_handle_can_compose_extract_capability() {
    let handle = ManagedWebHandle::keyless_default(true).expect("offline construction");
    assert!(handle.extract_coordinator().is_some());
}

#[test]
fn managed_web_handle_supports_ddg_only_fallback() {
    let handle = ManagedWebHandle::ddg_only().expect("offline construction");
    assert!(handle.extract_coordinator().is_none());
}
