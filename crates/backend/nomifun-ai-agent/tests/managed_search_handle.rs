#![cfg(feature = "managed-search")]

use std::sync::Arc;

use nomifun_ai_agent::ManagedSearchHandle;

#[test]
fn provider_clones_share_one_process_service_without_network() {
    let handle = ManagedSearchHandle::keyless_default().expect("offline construction");
    let left = handle.provider();
    let right = handle.provider();
    assert!(Arc::ptr_eq(&left, &right));
}
