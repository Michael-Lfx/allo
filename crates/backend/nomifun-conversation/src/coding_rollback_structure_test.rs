//! Structural guard: coding turn rollback must stay wired on the public
//! conversation router and use idle transcript truncation (not the admitted
//! edit fence).

#[test]
fn coding_rollback_route_and_service_contract() {
    let routes = include_str!("routes.rs");
    assert!(
        routes.contains(
            "\"/api/conversations/{conversation_id}/messages/{message_id}/coding-rollback\""
        ),
        "coding-rollback route must remain registered"
    );
    assert!(
        routes.contains("get(coding_rollback_availability).post(coding_rollback)"),
        "availability GET and rollback POST must share the coding-rollback path"
    );

    let service = include_str!("service.rs");
    assert!(
        service.contains("truncate_messages_for_coding_rollback"),
        "idle coding rollback must truncate via coding-rollback repo helper"
    );
    assert!(
        service.contains("rewind_persisted_nomi_session"),
        "coding rollback must rewind persisted Nomi session context"
    );
    assert!(
        service.contains("maybe_restore_coding_turn_checkpoint"),
        "edit-resubmit must restore coding checkpoints when present"
    );
    assert!(
        service.contains("maybe_create_coding_turn_checkpoint"),
        "coding turns must soft-create checkpoints after the user message lands"
    );
}
