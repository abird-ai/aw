use aw_agent_core::PrincipalId;
use aw_store::{
    ApplyContext, ApplyMode, CommitEvent, MaterializedContextRequest, SemanticSyncBatch,
    SessionBinding, SessionStore, WorkspaceKind,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

fn binding() -> SessionBinding {
    SessionBinding {
        working_directory: PathBuf::from("/fixture"),
        tool_root: PathBuf::from("/fixture"),
        system_prompt: String::new(),
        profile_hint: "fixture".into(),
        model_hint: "m".into(),
        destination_fingerprint: "0".repeat(64),
    }
}
fn actor() -> PrincipalId {
    PrincipalId::new("unix:1000").unwrap()
}

#[test]
fn local_workspace_is_created_and_sessions_belong_to_it() {
    let store = SessionStore::in_memory().unwrap();
    let actor = actor();
    let local = store.local_workspace(&actor, None).unwrap();
    assert_eq!(local.name, "local");
    assert_eq!(local.kind, WorkspaceKind::Hosted);
    let (session, _) = store.create(&actor, "hello", binding()).unwrap();
    assert_eq!(session.workspace_id, local.id);
    let target = store.session_target(&actor, &session.id).unwrap();
    assert!(matches!(
        target,
        aw_store::SessionExecutionTarget::Local { .. }
    ));
}

#[test]
fn workspace_creation_is_domain_scoped_and_local_is_reserved() {
    let store = SessionStore::in_memory().unwrap();
    let actor = actor();
    let domain = store.default_domain(&actor).unwrap();
    assert!(store
        .create_workspace(&actor, Some(&domain.id), "local")
        .is_err());
    let ws = store
        .create_workspace(&actor, Some(&domain.id), "research")
        .unwrap();
    assert_eq!(ws.domain_id, domain.id);
    assert_eq!(
        store
            .workspace(&actor, Some(&domain.id), "research")
            .unwrap()
            .id,
        ws.id
    );
}

#[test]
fn fleet_workspace_is_virtual_and_pins_fleet_target() {
    let store = SessionStore::in_memory().unwrap();
    let actor = actor();
    let domain = store.default_domain(&actor).unwrap();
    let fleet = store
        .create_fleet(&actor, Some(&domain.id), "gpu-lab")
        .unwrap();
    let local = store.local_machine().unwrap();
    store
        .add_fleet_member(&actor, &fleet.id, &local.id)
        .unwrap();
    let ws = store
        .create_fleet_workspace(&actor, Some(&domain.id), "gpu-dev", &fleet.id)
        .unwrap();
    assert_eq!(ws.kind, WorkspaceKind::FleetVirtual);
    let (session, _) = store
        .create_in_workspace(
            &actor,
            "fleet task",
            binding(),
            Some(&domain.id),
            Some(&ws.id),
            None,
        )
        .unwrap();
    match store.session_target(&actor, &session.id).unwrap() {
        aw_store::SessionExecutionTarget::Fleet {
            fleet_id,
            membership_revision,
        } => {
            assert_eq!(fleet_id, fleet.id);
            assert_eq!(membership_revision, Some(2));
        }
        other => panic!("unexpected target {other:?}"),
    }
}

#[test]
fn new_session_from_existing_session_keeps_workspace() {
    let store = SessionStore::in_memory().unwrap();
    let actor = actor();
    let ws = store.create_workspace(&actor, None, "isolated").unwrap();
    let (session, _) = store
        .create_in_workspace(&actor, "one", binding(), None, Some(&ws.id), None)
        .unwrap();
    let target = store.session_target(&actor, &session.id).unwrap();
    let (second, _) = store
        .create_in_workspace(
            &actor,
            "two",
            binding(),
            None,
            Some(&session.workspace_id),
            Some(target),
        )
        .unwrap();
    assert_eq!(second.workspace_id, session.workspace_id);
}

#[test]
fn domain_selector_disambiguates_workspace_and_fleet_names() {
    let store = SessionStore::in_memory().unwrap();
    let actor = actor();
    let first = store.default_domain(&actor).unwrap();
    let second = store.create_domain(&actor, "Second", None).unwrap();
    let first_ws = store
        .create_workspace(&actor, Some(&first.id), "shared")
        .unwrap();
    let second_ws = store
        .create_workspace(&actor, Some(&second.id), "shared")
        .unwrap();
    assert!(store.workspace(&actor, None, "shared").is_err());
    let (selected, _) = store
        .create_in_workspace(
            &actor,
            "selected",
            binding(),
            Some(&second.id),
            Some("shared"),
            None,
        )
        .unwrap();
    assert_eq!(selected.workspace_id, second_ws.id);
    assert_ne!(selected.workspace_id, first_ws.id);
    let second_local = store.local_workspace(&actor, Some(&second.id)).unwrap();
    let (domain_default, _) = store
        .create_in_workspace(
            &actor,
            "domain local",
            binding(),
            Some(&second.id),
            None,
            None,
        )
        .unwrap();
    assert_eq!(domain_default.workspace_id, second_local.id);
    let first_fleet = store
        .create_fleet(&actor, Some(&first.id), "workers")
        .unwrap();
    let second_fleet = store
        .create_fleet(&actor, Some(&second.id), "workers")
        .unwrap();
    assert!(store.fleet(&actor, "workers").is_err());
    assert_eq!(
        store
            .fleet_in_domain(&actor, Some(&second.id), "workers")
            .unwrap()
            .id,
        second_fleet.id
    );
    assert_ne!(first_fleet.id, second_fleet.id);
    let virtual_ws = store
        .create_fleet_workspace(&actor, Some(&second.id), "virtual", "workers")
        .unwrap();
    assert_eq!(virtual_ws.domain_id, second.id);
}

#[test]
fn explicit_targets_cannot_cross_workspace_domain_or_fleet_binding() {
    let store = SessionStore::in_memory().unwrap();
    let actor = actor();
    let first = store.default_domain(&actor).unwrap();
    let second = store.create_domain(&actor, "Second", None).unwrap();
    let second_ws = store
        .create_workspace(&actor, Some(&second.id), "work")
        .unwrap();
    let foreign = store
        .create_fleet(&actor, Some(&first.id), "foreign")
        .unwrap();
    assert!(store
        .create_in_workspace(
            &actor,
            "bad foreign target",
            binding(),
            Some(&second.id),
            Some(&second_ws.id),
            Some(aw_store::SessionExecutionTarget::Fleet {
                fleet_id: foreign.id,
                membership_revision: Some(1)
            })
        )
        .is_err());
    let fleet = store
        .create_fleet(&actor, Some(&second.id), "bound")
        .unwrap();
    let other = store
        .create_fleet(&actor, Some(&second.id), "other")
        .unwrap();
    let virtual_ws = store
        .create_fleet_workspace(&actor, Some(&second.id), "virtual", &fleet.id)
        .unwrap();
    assert!(store
        .create_in_workspace(
            &actor,
            "bad binding",
            binding(),
            Some(&second.id),
            Some(&virtual_ws.id),
            Some(aw_store::SessionExecutionTarget::Fleet {
                fleet_id: other.id,
                membership_revision: Some(1)
            })
        )
        .is_err());
    assert!(store
        .create_in_workspace(
            &actor,
            "future revision",
            binding(),
            Some(&second.id),
            Some(&second_ws.id),
            Some(aw_store::SessionExecutionTarget::Fleet {
                fleet_id: fleet.id,
                membership_revision: Some(999)
            })
        )
        .is_err());
}

#[test]
fn local_node_identity_is_stable_once_bound() {
    let store = SessionStore::in_memory().unwrap();
    let node = format!("node_{:x}", Sha256::digest([7u8; 32]));
    let fingerprint = format!("sha256:{:x}", Sha256::digest([7u8; 32]));
    let mismatched = format!("sha256:{:x}", Sha256::digest([6u8; 32]));
    assert!(store.bind_local_node_identity(&node, &mismatched).is_err());
    let bound = store.bind_local_node_identity(&node, &fingerprint).unwrap();
    assert_eq!(bound.node_id, node);
    assert_eq!(bound.key_fingerprint.as_deref(), Some(fingerprint.as_str()));
    assert_eq!(
        store
            .bind_local_node_identity(&node, &fingerprint)
            .unwrap()
            .node_id,
        node
    );
    let other_node = format!("node_{:x}", Sha256::digest([8u8; 32]));
    let other_fingerprint = format!("sha256:{:x}", Sha256::digest([8u8; 32]));
    assert!(store
        .bind_local_node_identity(&other_node, &other_fingerprint)
        .is_err());
}

#[test]
fn trusted_peer_registration_creates_domain_remote_machine_explicitly() {
    let store = SessionStore::in_memory().unwrap();
    let actor = actor();
    let domain = store.default_domain(&actor).unwrap();
    let public = [8u8; 32];
    let public_hex = public
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let node = format!("node_{:x}", Sha256::digest(public));

    assert!(store
        .register_remote_machine(&actor, Some(&domain.id), &node, None)
        .is_err());

    store
        .trust_awnet_peer(&node, &public_hex, "worker-eight")
        .unwrap();
    let machine = store
        .register_remote_machine(&actor, Some(&domain.id), &node, None)
        .unwrap();
    assert_eq!(machine.kind, "remote");
    assert_eq!(
        machine.owning_domain_id.as_deref(),
        Some(domain.id.as_str())
    );
    assert_eq!(machine.node_id, node);

    let retry = store
        .register_remote_machine(&actor, Some(&domain.id), &node, Some("worker-renamed"))
        .unwrap();
    assert_eq!(retry.id, machine.id);
    assert_eq!(retry.display_name, "worker-renamed");

    let fleet = store
        .create_fleet(&actor, Some(&domain.id), "remote-workers")
        .unwrap();
    let member = store
        .add_fleet_member_in_domain(&actor, Some(&domain.id), &fleet.id, &machine.id)
        .unwrap();
    assert_eq!(member.machine_id, machine.id);
}

#[test]
fn peer_trust_requires_node_id_to_match_ed25519_public_key() {
    let store = SessionStore::in_memory().unwrap();
    let public = [9u8; 32];
    let public_hex = public
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let node = format!("node_{:x}", Sha256::digest(public));
    let peer = store
        .trust_awnet_peer(&node, &public_hex, "peer-nine")
        .unwrap();
    assert_eq!(peer.node_id, node);
    assert_eq!(peer.public_key, public_hex);
    assert_eq!(peer.trust_state, "trusted");
    assert_eq!(
        store
            .trust_awnet_peer(&node, &peer.public_key, "peer-nine-renamed")
            .unwrap()
            .id,
        peer.id
    );
    let tampered = format!("node_{}", "0".repeat(64));
    assert!(store
        .trust_awnet_peer(&tampered, &peer.public_key, "tampered")
        .is_err());
}

#[test]
fn peer_workspace_and_remote_session_creation_are_stable_and_idempotent() {
    let store = SessionStore::in_memory().unwrap();
    let actor = actor();
    let first = store
        .create_peer_session_idempotent(
            &actor,
            "store_source",
            "dom_source",
            "ws_source",
            "aw-dev",
            "node_source",
            None,
            None,
            "ro_fixture",
            &"a".repeat(64),
            "remote child",
            binding(),
        )
        .unwrap();
    assert!(!first.reused);
    assert_eq!(first.workspace.origin_kind, "peer_created");
    assert!(first.workspace.name.starts_with("peer:aw-dev-"));
    assert_eq!(first.session.workspace_id, first.workspace.id);

    let second = store
        .create_peer_session_idempotent(
            &actor,
            "store_source",
            "dom_source",
            "ws_source",
            "aw-dev",
            "node_source",
            None,
            None,
            "ro_fixture",
            &"a".repeat(64),
            "changed title is ignored on exact logical retry",
            binding(),
        )
        .unwrap();
    assert!(second.reused);
    assert_eq!(second.workspace.id, first.workspace.id);
    assert_eq!(second.session.id, first.session.id);
    assert_eq!(second.branch.id, first.branch.id);

    assert!(store
        .create_peer_session_idempotent(
            &actor,
            "store_source",
            "dom_source",
            "ws_source",
            "aw-dev",
            "node_source",
            None,
            None,
            "ro_fixture",
            &"b".repeat(64),
            "changed immutable intent",
            binding(),
        )
        .is_err());
}

#[test]
fn remote_session_ref_tracks_pending_unknown_and_active_without_retargeting() {
    let store = SessionStore::in_memory().unwrap();
    let actor = actor();
    let workspace = store.local_workspace(&actor, None).unwrap();
    let public = [5u8; 32];
    let public_hex = public
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let node = format!("node_{:x}", Sha256::digest(public));
    store
        .trust_awnet_peer(&node, &public_hex, "remote-five")
        .unwrap();

    let pending = store
        .begin_remote_session_ref(
            &actor,
            "ro_source_fixture",
            &"c".repeat(64),
            &workspace.id,
            None,
            &node,
        )
        .unwrap();
    assert_eq!(pending.state, "pending");
    let unknown = store
        .mark_remote_session_ref_unknown(&actor, &pending.operation_id)
        .unwrap();
    assert_eq!(unknown.state, "unknown");
    let active = store
        .resolve_remote_session_ref(
            &actor,
            &pending.operation_id,
            "store_remote",
            "ws_remote",
            "s_remote",
            "b_remote",
        )
        .unwrap();
    assert_eq!(active.state, "active");
    assert_eq!(active.remote_session_id.as_deref(), Some("s_remote"));
    assert_eq!(
        store
            .begin_remote_session_ref(
                &actor,
                "ro_source_fixture",
                &"c".repeat(64),
                &workspace.id,
                None,
                &node,
            )
            .unwrap()
            .state,
        "active"
    );
    assert!(store
        .resolve_remote_session_ref(
            &actor,
            &pending.operation_id,
            "store_remote",
            "ws_remote",
            "s_other",
            "b_remote",
        )
        .is_err());
}

#[test]
fn fleet_operation_pins_membership_revision_across_retry() {
    let store = SessionStore::in_memory().unwrap();
    let actor = actor();
    let domain = store.default_domain(&actor).unwrap();
    let machine = store.local_machine().unwrap();
    let fleet = store
        .create_fleet(&actor, Some(&domain.id), "workers")
        .unwrap();
    store
        .add_fleet_member_in_domain(&actor, Some(&domain.id), &fleet.id, &machine.id)
        .unwrap();
    let workspace = store.local_workspace(&actor, Some(&domain.id)).unwrap();

    let first = store
        .create_fleet_operation(
            &actor,
            &fleet.id,
            Some(&workspace.id),
            "fo_fixture",
            &"a".repeat(64),
            "all",
        )
        .unwrap();
    assert!(!first.reused);
    assert_eq!(first.members.len(), 1);
    let pinned = first.operation.membership_revision;

    // Reactivating an existing member advances the Fleet revision but must not
    // rewrite the already-created logical operation snapshot.
    store
        .add_fleet_member_in_domain(&actor, Some(&domain.id), &fleet.id, &machine.id)
        .unwrap();
    let current = store.fleet(&actor, &fleet.id).unwrap();
    assert!(current.membership_revision > pinned);

    let retry = store
        .create_fleet_operation(
            &actor,
            &fleet.id,
            Some(&workspace.id),
            "fo_fixture",
            &"a".repeat(64),
            "all",
        )
        .unwrap();
    assert!(retry.reused);
    assert_eq!(retry.operation.membership_revision, pinned);
    assert_eq!(retry.members[0].machine_id, machine.id);
    assert!(store
        .create_fleet_operation(
            &actor,
            &fleet.id,
            Some(&workspace.id),
            "fo_fixture",
            &"b".repeat(64),
            "all",
        )
        .is_err());

    let completed = store
        .update_fleet_operation_member(
            &actor,
            "fo_fixture",
            &machine.id,
            "completed",
            None,
            Some(&"c".repeat(64)),
        )
        .unwrap();
    assert_eq!(completed.operation.state, "completed");
}

#[test]
fn federation_lease_is_action_scoped_and_revocation_invalidates_it() {
    let store = SessionStore::in_memory().unwrap();
    let actor = actor();
    let domain = store.default_domain(&actor).unwrap();
    let public = [9u8; 32];
    let public_hex = public
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let node = format!("node_{:x}", Sha256::digest(public));
    store
        .trust_awnet_peer(&node, &public_hex, "peer-nine")
        .unwrap();

    let federation = store
        .create_domain_federation(
            &actor,
            Some(&domain.id),
            &node,
            "remote-domain",
            "outbound",
            serde_json::json!({"workspaces":"all"}),
            serde_json::json!(["sessions.read", "sessions.create"]),
            false,
            false,
        )
        .unwrap();
    let lease = store
        .issue_federation_lease(&actor, &federation.id, 60)
        .unwrap();
    assert!(store
        .validate_federation_lease(&lease.id, &node, "sessions.read")
        .is_ok());
    assert!(store
        .validate_federation_lease(&lease.id, &node, "files.write")
        .is_err());

    store
        .revoke_domain_federation(&actor, &federation.id)
        .unwrap();
    assert!(store
        .validate_federation_lease(&lease.id, &node, "sessions.read")
        .is_err());
}

#[test]
fn semantic_sync_is_idempotent_and_does_not_rebind_source_facts() {
    let store = SessionStore::in_memory().unwrap();
    let actor = actor();
    let domain = store.default_domain(&actor).unwrap();
    let public = [11u8; 32];
    let public_hex = public
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let node = format!("node_{:x}", Sha256::digest(public));
    store
        .trust_awnet_peer(&node, &public_hex, "peer-eleven")
        .unwrap();
    let federation = store
        .create_domain_federation(
            &actor,
            Some(&domain.id),
            &node,
            "remote-domain",
            "inbound",
            serde_json::json!({"events":"semantic"}),
            serde_json::json!(["sync.read", "sync.write"]),
            false,
            false,
        )
        .unwrap();
    let lease = store
        .issue_federation_lease(&actor, &federation.id, 60)
        .unwrap();

    let event = CommitEvent {
        seq: 5,
        event_id: "ev_remote_5".into(),
        kind: "session.observed".into(),
        entity_type: "session".into(),
        entity_id: "s_remote".into(),
        actor: Some("remote:user".into()),
        payload: serde_json::json!({"fact":"remote evidence"}),
        created_at: 123,
    };
    let batch = SemanticSyncBatch {
        source_store_id: "store_remote".into(),
        after_seq: 0,
        through_seq: 5,
        events: vec![event.clone()],
    };
    let first = store.semantic_sync_apply(&lease.id, &node, &batch).unwrap();
    assert_eq!(first.imported, 1);
    assert!(!first.reused);

    let retry = store.semantic_sync_apply(&lease.id, &node, &batch).unwrap();
    assert!(retry.reused);
    assert_eq!(retry.imported, 0);

    let inbox = store
        .semantic_sync_inbox(&actor, &federation.id, "store_remote", 0, 10)
        .unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].event_id, event.event_id);

    let mut changed = batch.clone();
    changed.events[0].payload = serde_json::json!({"fact":"tampered"});
    assert!(store
        .semantic_sync_apply(&lease.id, &node, &changed)
        .is_err());

    store
        .revoke_domain_federation(&actor, &federation.id)
        .unwrap();
    let next = SemanticSyncBatch {
        source_store_id: "store_remote".into(),
        after_seq: 5,
        through_seq: 9,
        events: vec![CommitEvent {
            seq: 9,
            event_id: "ev_remote_9".into(),
            kind: "session.observed".into(),
            entity_type: "session".into(),
            entity_id: "s_remote".into(),
            actor: None,
            payload: serde_json::json!({"fact":"after revoke"}),
            created_at: 456,
        }],
    };
    assert!(store.semantic_sync_apply(&lease.id, &node, &next).is_err());
}
#[test]
fn semantic_sync_cursor_may_advance_across_filtered_source_rows() {
    let store = SessionStore::in_memory().unwrap();
    let actor = actor();
    let domain = store.default_domain(&actor).unwrap();
    let public = [12u8; 32];
    let public_hex = public
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let node = format!("node_{:x}", Sha256::digest(public));
    store
        .trust_awnet_peer(&node, &public_hex, "peer-twelve")
        .unwrap();
    let federation = store
        .create_domain_federation(
            &actor,
            Some(&domain.id),
            &node,
            "remote-domain",
            "inbound",
            serde_json::json!({"events":"semantic"}),
            serde_json::json!(["sync.read", "sync.write"]),
            false,
            false,
        )
        .unwrap();
    let lease = store
        .issue_federation_lease(&actor, &federation.id, 60)
        .unwrap();

    let cursor_only = SemanticSyncBatch {
        source_store_id: "store_filtered_remote".into(),
        after_seq: 0,
        through_seq: 4,
        events: vec![],
    };
    let advanced = store
        .semantic_sync_apply(&lease.id, &node, &cursor_only)
        .unwrap();
    assert_eq!(advanced.through_seq, 4);
    assert_eq!(advanced.imported, 0);

    let with_trailing_filtered_rows = SemanticSyncBatch {
        source_store_id: "store_filtered_remote".into(),
        after_seq: 4,
        through_seq: 9,
        events: vec![CommitEvent {
            seq: 7,
            event_id: "ev_filtered_7".into(),
            kind: "session.observed".into(),
            entity_type: "session".into(),
            entity_id: "s_remote".into(),
            actor: Some("remote:user".into()),
            payload: serde_json::json!({"fact":"visible inside scanned page"}),
            created_at: 123,
        }],
    };
    let applied = store
        .semantic_sync_apply(&lease.id, &node, &with_trailing_filtered_rows)
        .unwrap();
    assert_eq!(applied.through_seq, 9);
    assert_eq!(applied.imported, 1);

    let retry = store
        .semantic_sync_apply(&lease.id, &node, &with_trailing_filtered_rows)
        .unwrap();
    assert!(retry.reused);
    assert_eq!(retry.through_seq, 9);
}

#[test]
fn semantic_materialization_is_explicit_idempotent_and_revocable() {
    let store = SessionStore::in_memory().unwrap();
    let actor = actor();
    let outsider = PrincipalId::new("unix:2000").unwrap();
    let domain = store.default_domain(&actor).unwrap();
    let public = [13u8; 32];
    let public_hex = public
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let node = format!("node_{:x}", Sha256::digest(public));
    store
        .trust_awnet_peer(&node, &public_hex, "peer-thirteen")
        .unwrap();
    let federation = store
        .create_domain_federation(
            &actor,
            Some(&domain.id),
            &node,
            "remote-domain",
            "inbound",
            serde_json::json!({"events":"semantic"}),
            serde_json::json!(["sync.read", "sync.write"]),
            false,
            false,
        )
        .unwrap();
    let lease = store
        .issue_federation_lease(&actor, &federation.id, 60)
        .unwrap();
    let batch = SemanticSyncBatch {
        source_store_id: "store_materialization_remote".into(),
        after_seq: 0,
        through_seq: 6,
        events: vec![
            CommitEvent {
                seq: 5,
                event_id: "ev_materialize_5".into(),
                kind: "session.observed".into(),
                entity_type: "session".into(),
                entity_id: "s_remote_materialized".into(),
                actor: Some("remote:alice".into()),
                payload: serde_json::json!({"fact":"alpha"}),
                created_at: 123,
            },
            CommitEvent {
                seq: 6,
                event_id: "ev_materialize_6".into(),
                kind: "session.observed".into(),
                entity_type: "session".into(),
                entity_id: "s_remote_materialized".into(),
                actor: Some("remote:alice".into()),
                payload: serde_json::json!({"fact":"beta"}),
                created_at: 124,
            },
        ],
    };
    store.semantic_sync_apply(&lease.id, &node, &batch).unwrap();

    assert!(store
        .create_semantic_materialization_policy(
            &outsider,
            &federation.id,
            &batch.source_store_id,
            "session.observed",
            "session",
            "s_remote_materialized",
        )
        .is_err());

    let created = store
        .create_semantic_materialization_policy(
            &actor,
            &federation.id,
            &batch.source_store_id,
            "session.observed",
            "session",
            "s_remote_materialized",
        )
        .unwrap();
    assert!(!created.reused);
    assert_eq!(created.policy.status, "active");
    let reused_policy = store
        .create_semantic_materialization_policy(
            &actor,
            &federation.id,
            &batch.source_store_id,
            "session.observed",
            "session",
            "s_remote_materialized",
        )
        .unwrap();
    assert!(reused_policy.reused);
    assert_eq!(reused_policy.policy.id, created.policy.id);

    assert!(store
        .semantic_materialized_facts(&actor, &federation.id, &batch.source_store_id, 0, 10,)
        .unwrap()
        .is_empty());

    let first = store
        .materialize_semantic_fact(&actor, &created.policy.id, 5)
        .unwrap();
    assert!(!first.reused);
    assert_eq!(first.fact.source_event_id, "ev_materialize_5");
    assert_eq!(
        first.fact.source_payload,
        serde_json::json!({"fact":"alpha"})
    );
    assert_eq!(first.fact.policy_id, created.policy.id);
    assert_eq!(first.fact.policy_revision, 1);

    let retried = store
        .materialize_semantic_fact(&actor, &created.policy.id, 5)
        .unwrap();
    assert!(retried.reused);
    assert_eq!(retried.fact.id, first.fact.id);

    let revoked = store
        .revoke_semantic_materialization_policy(&actor, &created.policy.id)
        .unwrap();
    assert_eq!(revoked.status, "revoked");
    assert_eq!(revoked.revision, 2);
    assert!(store
        .materialize_semantic_fact(&actor, &created.policy.id, 6)
        .is_err());

    let replacement = store
        .create_semantic_materialization_policy(
            &actor,
            &federation.id,
            &batch.source_store_id,
            "session.observed",
            "session",
            "s_remote_materialized",
        )
        .unwrap();
    assert_ne!(replacement.policy.id, created.policy.id);
    let second = store
        .materialize_semantic_fact(&actor, &replacement.policy.id, 6)
        .unwrap();
    assert_eq!(second.fact.source_event_id, "ev_materialize_6");

    let facts = store
        .semantic_materialized_facts(&actor, &federation.id, &batch.source_store_id, 0, 10)
        .unwrap();
    assert_eq!(facts.len(), 2);
    assert_eq!(facts[0].source_seq, 5);
    assert_eq!(facts[1].source_seq, 6);

    store
        .revoke_domain_federation(&actor, &federation.id)
        .unwrap();
    assert!(store
        .materialize_semantic_fact(&actor, &replacement.policy.id, 6)
        .is_err());
    assert_eq!(
        store
            .semantic_materialized_facts(&actor, &federation.id, &batch.source_store_id, 0, 10,)
            .unwrap()
            .len(),
        2
    );
}

fn fixture_authority_key(byte: u8) -> SigningKey {
    SigningKey::from_bytes(&[byte; 32])
}

fn fixture_authority_node(byte: u8) -> (String, String) {
    let public = fixture_authority_key(byte).verifying_key().to_bytes();
    let public_hex = public
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    (format!("node_{:x}", Sha256::digest(public)), public_hex)
}

fn bind_fixture_node(store: &SessionStore, byte: u8) -> String {
    let (node, _) = fixture_authority_node(byte);
    let fingerprint = format!("sha256:{}", &node[5..]);
    store.bind_local_node_identity(&node, &fingerprint).unwrap();
    node
}

fn trust_fixture_peer(store: &SessionStore, byte: u8, name: &str) -> String {
    let (node, public_hex) = fixture_authority_node(byte);
    store.trust_awnet_peer(&node, &public_hex, name).unwrap();
    node
}

#[test]
fn domain_authority_quorum_fences_control_writes_and_remote_double_votes() {
    let a = SessionStore::in_memory().unwrap();
    let b = SessionStore::in_memory().unwrap();
    let actor = actor();
    let node_a = bind_fixture_node(&a, 21);
    let node_b = bind_fixture_node(&b, 22);
    let node_c = trust_fixture_peer(&a, 23, "authority-c");
    let node_b_on_a = trust_fixture_peer(&a, 22, "authority-b");
    assert_eq!(node_b_on_a, node_b);
    let node_a_on_b = trust_fixture_peer(&b, 21, "authority-a");
    assert_eq!(node_a_on_b, node_a);
    let node_c_on_b = trust_fixture_peer(&b, 23, "authority-c");
    assert_eq!(node_c_on_b, node_c);

    let domain = a.default_domain(&actor).unwrap();
    a.register_remote_machine(&actor, Some(&domain.id), &node_b, None)
        .unwrap();
    a.register_remote_machine(&actor, Some(&domain.id), &node_c, None)
        .unwrap();

    let initial = a.domain_authority_set(&actor, Some(&domain.id)).unwrap();
    assert_eq!(initial.quorum, 1);
    assert_eq!(initial.members, vec![node_a.clone()]);

    let configured = a
        .configure_domain_authority(
            &actor,
            Some(&domain.id),
            &[node_a.clone(), node_b.clone(), node_c.clone()],
            2,
        )
        .unwrap();
    assert_eq!(configured.quorum, 2);
    assert_eq!(configured.members.len(), 3);
    assert!(a
        .create_workspace(&actor, Some(&domain.id), "blocked-without-quorum")
        .is_err());

    let bundle = a
        .domain_authority_grant_bundle(&actor, Some(&domain.id))
        .unwrap();
    let grant = b
        .accept_domain_authority_peer_grant(&actor, &node_a, &bundle)
        .unwrap();
    assert_eq!(grant.status, "active");

    let election = a
        .begin_domain_authority_election(&actor, Some(&domain.id), 5)
        .unwrap();
    assert_eq!(election.vote_count, 1);
    assert!(a
        .activate_domain_authority_election(&actor, &election.id)
        .is_err());

    let vote = b
        .prepare_domain_authority_peer_vote(
            &node_a,
            &bundle.authority_domain_id,
            bundle.set_revision,
            &bundle.members_sha256,
            bundle.quorum,
            election.term,
            &election.id,
            &node_a,
            election.requested_ttl_seconds,
        )
        .unwrap();
    let vote_signature = STANDARD.encode(
        fixture_authority_key(22)
            .sign(&aw_store::domain_authority_vote_transcript(&vote))
            .to_bytes(),
    );
    a.record_domain_authority_vote(
        &actor,
        &election.id,
        &vote.voter_node_id,
        vote.expires_at,
        &vote_signature,
    )
    .unwrap();
    let active = a
        .activate_domain_authority_election(&actor, &election.id)
        .unwrap();
    assert_eq!(active.state, "active");
    assert!(active.expires_at.is_some());
    a.create_workspace(&actor, Some(&domain.id), "allowed-with-quorum")
        .unwrap();

    let federation = a
        .create_domain_federation(
            &actor,
            Some(&domain.id),
            &node_b,
            "remote-b",
            "outbound",
            serde_json::json!({"sessions":"read"}),
            serde_json::json!(["sessions.read"]),
            false,
            false,
        )
        .unwrap();
    let admitted = a
        .issue_federation_lease(&actor, &federation.id, 60)
        .unwrap();

    let conflicting = b.prepare_domain_authority_peer_vote(
        &node_c,
        &bundle.authority_domain_id,
        bundle.set_revision,
        &bundle.members_sha256,
        bundle.quorum,
        election.term + 1,
        "aelect_conflicting_candidate",
        &node_c,
        30,
    );
    assert!(conflicting.is_err());

    // Authority is CP: once the short quorum lease expires, new control-plane
    // writes stop. Already-admitted bounded work remains available until its
    // own independent lease expires.
    std::thread::sleep(std::time::Duration::from_millis(5_200));
    assert!(a
        .create_workspace(&actor, Some(&domain.id), "blocked-after-quorum-loss")
        .is_err());
    assert!(a
        .validate_federation_lease(&admitted.id, &node_b, "sessions.read")
        .is_ok());
}

#[test]
fn authority_replica_requires_fresh_quorum_after_source_lease_expires() {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use ed25519_dalek::{Signer, SigningKey};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn micros_after(seconds: u64) -> i64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as i64;
        now + (seconds as i64 * 1_000_000)
    }
    fn bind(store: &SessionStore, seed: u8) -> (SigningKey, String, String) {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let public = key.verifying_key().to_bytes();
        let public_hex = public
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let digest = Sha256::digest(public);
        let node = format!("node_{digest:x}");
        store
            .bind_local_node_identity(&node, &format!("sha256:{digest:x}"))
            .unwrap();
        (key, node, public_hex)
    }

    let source = SessionStore::in_memory().unwrap();
    let standby = SessionStore::in_memory().unwrap();
    let source_actor = PrincipalId::new("source:admin").unwrap();
    let standby_actor = PrincipalId::new("standby:operator").unwrap();
    let (source_key, source_node, source_public) = bind(&source, 41);
    let (standby_key, standby_node, standby_public) = bind(&standby, 42);

    source
        .trust_awnet_peer(&standby_node, &standby_public, "standby")
        .unwrap();
    standby
        .trust_awnet_peer(&source_node, &source_public, "source")
        .unwrap();

    let domain = source.default_domain(&source_actor).unwrap();
    source
        .register_remote_machine(
            &source_actor,
            Some(&domain.id),
            &standby_node,
            Some("standby"),
        )
        .unwrap();
    let set = source
        .configure_domain_authority(
            &source_actor,
            Some(&domain.id),
            &[source_node.clone(), standby_node.clone()],
            2,
        )
        .unwrap();
    let bundle = source
        .domain_authority_grant_bundle(&source_actor, Some(&domain.id))
        .unwrap();
    standby
        .accept_domain_authority_peer_grant(&standby_actor, &source_node, &bundle)
        .unwrap();

    let source_election = source
        .begin_domain_authority_election(&source_actor, Some(&domain.id), 5)
        .unwrap();
    let source_vote_expiry = micros_after(5);
    let source_vote = aw_store::DomainAuthorityPeerVoteInfo {
        authority_domain_id: domain.id.clone(),
        set_revision: set.set_revision,
        members_sha256: set.members_sha256.clone(),
        term: source_election.term,
        election_id: source_election.id.clone(),
        voter_node_id: standby_node.clone(),
        candidate_node_id: source_node.clone(),
        ttl_seconds: 5,
        expires_at: source_vote_expiry,
        created_at: 0,
    };
    let source_vote_signature = STANDARD.encode(
        standby_key
            .sign(&aw_store::domain_authority_vote_transcript(&source_vote))
            .to_bytes(),
    );
    source
        .record_domain_authority_vote(
            &source_actor,
            &source_election.id,
            &standby_node,
            source_vote_expiry,
            &source_vote_signature,
        )
        .unwrap();
    source
        .activate_domain_authority_election(&source_actor, &source_election.id)
        .unwrap();

    let snapshot = source
        .export_domain_authority_snapshot(&source_actor, Some(&domain.id))
        .unwrap();
    let snapshot_signature = STANDARD.encode(
        source_key
            .sign(&aw_store::domain_authority_snapshot_transcript(&snapshot))
            .to_bytes(),
    );
    let imported = standby
        .import_domain_authority_replica_snapshot(
            &standby_actor,
            &source_node,
            &snapshot,
            &snapshot_signature,
        )
        .unwrap();
    assert_eq!(imported.head_status, "standby");
    assert_eq!(imported.snapshot.authority_domain_id, domain.id);

    let promotion_election = standby
        .begin_domain_authority_replica_election(&standby_actor, &domain.id, 10)
        .unwrap();
    assert!(standby
        .activate_domain_authority_replica_election(&standby_actor, &promotion_election.id)
        .is_err());

    let source_leader_expiry = snapshot.leader_expires_at.unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros() as i64;
    if source_leader_expiry > now {
        thread::sleep(Duration::from_micros(
            (source_leader_expiry - now) as u64 + 150_000,
        ));
    }

    let promotion_vote_expiry = micros_after(8);
    let promotion_vote = aw_store::DomainAuthorityPeerVoteInfo {
        authority_domain_id: domain.id.clone(),
        set_revision: set.set_revision,
        members_sha256: set.members_sha256.clone(),
        term: promotion_election.term,
        election_id: promotion_election.id.clone(),
        voter_node_id: source_node.clone(),
        candidate_node_id: standby_node.clone(),
        ttl_seconds: 10,
        expires_at: promotion_vote_expiry,
        created_at: 0,
    };
    let promotion_vote_signature = STANDARD.encode(
        source_key
            .sign(&aw_store::domain_authority_vote_transcript(&promotion_vote))
            .to_bytes(),
    );
    standby
        .record_domain_authority_replica_vote(
            &standby_actor,
            &promotion_election.id,
            &source_node,
            promotion_vote_expiry,
            &promotion_vote_signature,
        )
        .unwrap();
    let active = standby
        .activate_domain_authority_replica_election(&standby_actor, &promotion_election.id)
        .unwrap();
    assert_eq!(active.state, "active");
    assert_eq!(active.vote_count, 2);

    let promotion = standby
        .promote_domain_authority_replica(&standby_actor, &promotion_election.id)
        .unwrap();
    assert_eq!(promotion.authority_domain_id, domain.id);
    assert_eq!(promotion.promoted_node_id, standby_node);
    assert_eq!(promotion.promoted_term, promotion_election.term);
    assert_eq!(
        promotion.promoted_authority_store_id,
        standby.store_id().unwrap()
    );
    assert_eq!(
        standby
            .domain_authority_replica(&standby_actor, &domain.id)
            .unwrap()
            .head_status,
        "promoted"
    );
}

#[test]
fn workspace_writer_failover_is_quorum_fenced_and_epoch_cas_guarded() {
    let a = SessionStore::in_memory().unwrap();
    let b = SessionStore::in_memory().unwrap();
    let actor = actor();
    let node_a = bind_fixture_node(&a, 61);
    let node_b = bind_fixture_node(&b, 62);
    let node_b_on_a = trust_fixture_peer(&a, 62, "writer-b");
    assert_eq!(node_b_on_a, node_b);
    let node_a_on_b = trust_fixture_peer(&b, 61, "writer-a");
    assert_eq!(node_a_on_b, node_a);

    let domain = a.default_domain(&actor).unwrap();
    a.register_remote_machine(&actor, Some(&domain.id), &node_b, Some("writer-b"))
        .unwrap();
    let workspace = a
        .create_workspace(&actor, Some(&domain.id), "writer-fixture")
        .unwrap();
    let (session, branch) = a
        .create_in_workspace(
            &actor,
            "writer session",
            binding(),
            Some(&domain.id),
            Some(&workspace.id),
            None,
        )
        .unwrap();
    let initial = a
        .workspace_writer(&actor, Some(&domain.id), &workspace.id)
        .unwrap();
    assert_eq!(initial.writer_node_id, node_a);
    assert_eq!(initial.writer_epoch, 1);

    let set = a
        .configure_domain_authority(
            &actor,
            Some(&domain.id),
            &[node_a.clone(), node_b.clone()],
            2,
        )
        .unwrap();
    assert!(a
        .failover_workspace_writer(
            &actor,
            Some(&domain.id),
            &workspace.id,
            1,
            &node_b,
            "without_quorum",
        )
        .is_err());

    let bundle = a
        .domain_authority_grant_bundle(&actor, Some(&domain.id))
        .unwrap();
    b.accept_domain_authority_peer_grant(&actor, &node_a, &bundle)
        .unwrap();
    let election = a
        .begin_domain_authority_election(&actor, Some(&domain.id), 30)
        .unwrap();
    let vote = b
        .prepare_domain_authority_peer_vote(
            &node_a,
            &domain.id,
            set.set_revision,
            &set.members_sha256,
            set.quorum,
            election.term,
            &election.id,
            &node_a,
            election.requested_ttl_seconds,
        )
        .unwrap();
    let signature = STANDARD.encode(
        fixture_authority_key(62)
            .sign(&aw_store::domain_authority_vote_transcript(&vote))
            .to_bytes(),
    );
    a.record_domain_authority_vote(&actor, &election.id, &node_b, vote.expires_at, &signature)
        .unwrap();
    a.activate_domain_authority_election(&actor, &election.id)
        .unwrap();

    let remote = a
        .failover_workspace_writer(
            &actor,
            Some(&domain.id),
            &workspace.id,
            1,
            &node_b,
            "authority_failover",
        )
        .unwrap();
    assert_eq!(remote.writer_epoch, 2);
    assert_eq!(remote.writer_node_id, node_b);
    assert_eq!(remote.authority_term, election.term);
    assert!(a
        .checkpoint(
            &actor,
            &session.id,
            &branch.id,
            branch.revision,
            "stale writer must not advance",
        )
        .is_err());
    assert!(a
        .create_in_workspace(
            &actor,
            "stale writer session",
            binding(),
            Some(&domain.id),
            Some(&workspace.id),
            None,
        )
        .is_err());
    assert!(a
        .failover_workspace_writer(
            &actor,
            Some(&domain.id),
            &workspace.id,
            1,
            &node_a,
            "stale_expected_epoch",
        )
        .is_err());

    let local = a
        .failover_workspace_writer(
            &actor,
            Some(&domain.id),
            &workspace.id,
            2,
            &node_a,
            "restore_local_writer",
        )
        .unwrap();
    assert_eq!(local.writer_epoch, 3);
    assert_eq!(local.writer_node_id, node_a);
    let checkpoint = a
        .checkpoint(
            &actor,
            &session.id,
            &branch.id,
            branch.revision,
            "writer restored under newer epoch",
        )
        .unwrap();
    assert_eq!(checkpoint.revision, branch.revision + 1);

    let events = a
        .workspace_writer_events(&actor, Some(&domain.id), &workspace.id)
        .unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].new_writer_epoch, 1);
    assert_eq!(events[1].new_writer_epoch, 2);
    assert_eq!(events[2].new_writer_epoch, 3);
}

#[test]
fn workspace_replica_handoff_bootstraps_remote_writer_and_fast_forwards_on_failback() {
    let source = SessionStore::in_memory().unwrap();
    let target = SessionStore::in_memory().unwrap();
    let actor = actor();
    let source_node = bind_fixture_node(&source, 81);
    let target_node = bind_fixture_node(&target, 82);
    assert_eq!(
        trust_fixture_peer(&source, 82, "workspace-target"),
        target_node
    );
    assert_eq!(
        trust_fixture_peer(&target, 81, "workspace-source"),
        source_node
    );

    let domain = source.default_domain(&actor).unwrap();
    source
        .register_remote_machine(
            &actor,
            Some(&domain.id),
            &target_node,
            Some("workspace-target"),
        )
        .unwrap();
    let workspace = source
        .create_workspace(&actor, Some(&domain.id), "replicated-workspace")
        .unwrap();
    let (session, branch) = source
        .create_in_workspace(
            &actor,
            "replicated session",
            binding(),
            Some(&domain.id),
            Some(&workspace.id),
            None,
        )
        .unwrap();
    let prepared = source
        .checkpoint(
            &actor,
            &session.id,
            &branch.id,
            branch.revision,
            "before handoff",
        )
        .unwrap();

    let remote_writer = source
        .failover_workspace_writer(
            &actor,
            Some(&domain.id),
            &workspace.id,
            1,
            &target_node,
            "phase29_test_handoff",
        )
        .unwrap();
    assert_eq!(remote_writer.writer_epoch, 2);
    assert_eq!(remote_writer.writer_node_id, target_node);
    assert!(source
        .checkpoint(
            &actor,
            &session.id,
            &branch.id,
            prepared.revision,
            "stale source write",
        )
        .is_err());

    let snapshot = source
        .export_workspace_replica_snapshot(&actor, &workspace.id, &target_node)
        .unwrap();
    assert_eq!(snapshot.writer.writer_epoch, 2);
    assert_eq!(snapshot.writer.writer_node_id, target_node);
    assert_eq!(snapshot.sessions.len(), 1);
    let source_signature = STANDARD.encode(
        fixture_authority_key(81)
            .sign(&aw_store::workspace_replica_snapshot_transcript(&snapshot))
            .to_bytes(),
    );
    let applied = target
        .import_workspace_replica_snapshot(&actor, &source_node, &snapshot, &source_signature)
        .unwrap();
    let stable_actor =
        aw_store::stable_user_principal(&source.current_user(&actor).unwrap().user_id).unwrap();
    assert!(!applied.reused);
    assert_eq!(applied.writer.writer_epoch, 2);
    assert_eq!(applied.writer.writer_node_id, target_node);
    assert_eq!(applied.sessions_imported, 1);
    assert!(applied.nodes_imported >= 2);

    let target_branches = target
        .replica_writer_branches(
            &source_node,
            &stable_actor,
            &workspace.id,
            2,
            remote_writer.authority_term,
            &session.id,
        )
        .unwrap();
    let target_main = target_branches
        .iter()
        .find(|b| b.id == branch.id)
        .unwrap()
        .clone();
    assert_eq!(target_main.head, prepared.head);
    let target_checkpoint = target
        .replica_writer_checkpoint(
            &source_node,
            &stable_actor,
            &workspace.id,
            2,
            remote_writer.authority_term,
            &session.id,
            &branch.id,
            &target_main.head,
            target_main.revision,
            "written by target writer",
        )
        .unwrap();
    assert_ne!(target_checkpoint.head, prepared.head);
    assert!(target
        .replica_writer_checkpoint(
            &source_node,
            &actor,
            &workspace.id,
            1,
            remote_writer.authority_term,
            &session.id,
            &branch.id,
            &target_checkpoint.head,
            target_checkpoint.revision,
            "stale writer epoch",
        )
        .is_err());

    // Replaying the exact original handoff snapshot is lookup-only and cannot
    // roll back a branch advanced by the new writer.
    let replay = target
        .import_workspace_replica_snapshot(&actor, &source_node, &snapshot, &source_signature)
        .unwrap();
    assert!(replay.reused);
    let after_replay = target
        .replica_writer_branches(
            &source_node,
            &stable_actor,
            &workspace.id,
            2,
            remote_writer.authority_term,
            &session.id,
        )
        .unwrap()
        .into_iter()
        .find(|b| b.id == branch.id)
        .unwrap();
    assert_eq!(after_replay.head, target_checkpoint.head);

    // Domain authority transfers the writer back to the source. The source has
    // not advanced a canonical ref under epoch 3 yet, so it may absorb the
    // immediately previous writer's signed epoch-2 snapshot and fast-forward.
    let source_writer = source
        .failover_workspace_writer(
            &actor,
            Some(&domain.id),
            &workspace.id,
            2,
            &source_node,
            "phase29_test_failback",
        )
        .unwrap();
    assert_eq!(source_writer.writer_epoch, 3);
    assert_eq!(source_writer.writer_node_id, source_node);
    let fence_event = source
        .workspace_writer_events(&actor, Some(&domain.id), &workspace.id)
        .unwrap()
        .last()
        .cloned()
        .unwrap();
    let fenced = target
        .replica_writer_fence(&source_node, &workspace.id, &fence_event)
        .unwrap();
    assert_eq!(fenced.writer_epoch, 3);
    assert_eq!(fenced.writer_node_id, source_node);
    assert!(target
        .replica_writer_checkpoint(
            &source_node,
            &stable_actor,
            &workspace.id,
            2,
            remote_writer.authority_term,
            &session.id,
            &branch.id,
            &target_checkpoint.head,
            target_checkpoint.revision,
            "fenced writer cannot advance",
        )
        .is_err());

    let return_snapshot = target
        .export_workspace_replica_snapshot(&actor, &workspace.id, &source_node)
        .unwrap();
    assert_eq!(return_snapshot.writer.writer_epoch, 3);
    assert_eq!(return_snapshot.ref_writer_epoch, 2);
    let target_signature = STANDARD.encode(
        fixture_authority_key(82)
            .sign(&aw_store::workspace_replica_snapshot_transcript(
                &return_snapshot,
            ))
            .to_bytes(),
    );
    let returned = source
        .import_workspace_replica_snapshot(
            &actor,
            &target_node,
            &return_snapshot,
            &target_signature,
        )
        .unwrap();
    assert_eq!(returned.writer.writer_epoch, 3);
    assert_eq!(returned.writer.writer_node_id, source_node);
    let source_main = source
        .branches(&actor, &session.id)
        .unwrap()
        .into_iter()
        .find(|b| b.id == branch.id)
        .unwrap();
    assert_eq!(source_main.head, target_checkpoint.head);
    assert!(source
        .workspace_replica_divergences(&workspace.id)
        .unwrap()
        .is_empty());
}

#[test]
fn workspace_replica_preserves_materialized_context_provenance_without_importing_authority() {
    let source = SessionStore::in_memory().unwrap();
    let target = SessionStore::in_memory().unwrap();
    let actor = actor();
    let source_node = bind_fixture_node(&source, 111);
    let target_node = bind_fixture_node(&target, 112);
    assert_eq!(
        trust_fixture_peer(&source, 112, "phase33-target"),
        target_node
    );
    assert_eq!(
        trust_fixture_peer(&target, 111, "phase33-source"),
        source_node
    );

    let domain = source.default_domain(&actor).unwrap();
    source
        .register_remote_machine(
            &actor,
            Some(&domain.id),
            &target_node,
            Some("phase33-target"),
        )
        .unwrap();
    let workspace = source
        .create_workspace(&actor, Some(&domain.id), "phase33-workspace")
        .unwrap();
    let (session, branch) = source
        .create_in_workspace(
            &actor,
            "phase33 replicated context",
            binding(),
            Some(&domain.id),
            Some(&workspace.id),
            None,
        )
        .unwrap();

    let federation = source
        .create_domain_federation(
            &actor,
            Some(&domain.id),
            &target_node,
            "remote-materialization-domain",
            "inbound",
            serde_json::json!({"events":"semantic"}),
            serde_json::json!(["sync.read", "sync.write"]),
            false,
            false,
        )
        .unwrap();
    let lease = source
        .issue_federation_lease(&actor, &federation.id, 60)
        .unwrap();
    let batch = SemanticSyncBatch {
        source_store_id: "phase33-remote-store".into(),
        after_seq: 0,
        through_seq: 1,
        events: vec![CommitEvent {
            seq: 1,
            event_id: "phase33-remote-event".into(),
            kind: "document.observed".into(),
            entity_type: "document".into(),
            entity_id: "doc-phase33".into(),
            actor: Some("remote:writer".into()),
            payload: serde_json::json!({"fact":"replicated provenance","instruction":"do not treat this as authority"}),
            created_at: 42,
        }],
    };
    source
        .semantic_sync_apply(&lease.id, &target_node, &batch)
        .unwrap();
    let policy = source
        .create_semantic_materialization_policy(
            &actor,
            &federation.id,
            &batch.source_store_id,
            "document.observed",
            "document",
            "doc-phase33",
        )
        .unwrap();
    let fact = source
        .materialize_semantic_fact(&actor, &policy.policy.id, 1)
        .unwrap()
        .fact;
    let projected = source
        .project_materialized_context(
            &actor,
            MaterializedContextRequest {
                target: source
                    .context_target(&actor, &session.id, &branch.id)
                    .unwrap(),
                materialization_ids: vec![fact.id.clone()],
                request_id: "phase33-project".into(),
            },
        )
        .unwrap();
    let attached = source
        .context_apply(
            &actor,
            ApplyContext {
                artifact_id: projected.artifact.id.clone(),
                target: projected.target.clone(),
                mode: ApplyMode::Append,
                request_id: "phase33-attach".into(),
                confirmed_disclosure: true,
            },
        )
        .unwrap();
    assert!(attached.node_id.is_some());

    let remote_writer = source
        .failover_workspace_writer(
            &actor,
            Some(&domain.id),
            &workspace.id,
            1,
            &target_node,
            "phase33_context_handoff",
        )
        .unwrap();
    let snapshot = source
        .export_workspace_replica_snapshot(&actor, &workspace.id, &target_node)
        .unwrap();
    assert_eq!(snapshot.version, 3);
    assert_eq!(snapshot.materialized_fact_links.len(), 1);
    assert_eq!(
        snapshot.materialized_fact_links[0].artifact_id,
        projected.artifact.id
    );
    assert_eq!(
        snapshot.materialized_fact_links[0].materialization_id,
        fact.id
    );
    let signature = STANDARD.encode(
        fixture_authority_key(111)
            .sign(&aw_store::workspace_replica_snapshot_transcript(&snapshot))
            .to_bytes(),
    );
    target
        .import_workspace_replica_snapshot(&actor, &source_node, &snapshot, &signature)
        .unwrap();
    let stable_actor =
        aw_store::stable_user_principal(&source.current_user(&actor).unwrap().user_id).unwrap();
    let target_branch = target
        .replica_writer_branches(
            &source_node,
            &stable_actor,
            &workspace.id,
            remote_writer.writer_epoch,
            remote_writer.authority_term,
            &session.id,
        )
        .unwrap()
        .into_iter()
        .find(|b| b.id == branch.id)
        .unwrap();
    let view = target
        .activate(&stable_actor, &session.id, &target_branch.id)
        .unwrap()
        .0;
    assert!(view.history.context_blocks.iter().any(|block| {
        block.artifact_id == projected.artifact.id && block.text.contains("replicated provenance")
    }));

    // Snapshot v3 preserved the provenance link, but did not create a local
    // Phase-25 materialized fact or federation policy. A new use fails closed.
    assert!(target
        .context_apply(
            &stable_actor,
            ApplyContext {
                artifact_id: projected.artifact.id,
                target: target
                    .context_target(&stable_actor, &session.id, &target_branch.id)
                    .unwrap(),
                mode: ApplyMode::NextPrompt,
                request_id: "phase33-target-reuse".into(),
                confirmed_disclosure: true,
            },
        )
        .is_err());
}

#[test]
fn workspace_replica_rejoin_preserves_divergent_previous_writer_descendant() {
    let source = SessionStore::in_memory().unwrap();
    let target = SessionStore::in_memory().unwrap();
    let actor = actor();
    let source_node = bind_fixture_node(&source, 91);
    let target_node = bind_fixture_node(&target, 92);
    assert_eq!(
        trust_fixture_peer(&source, 92, "diverge-target"),
        target_node
    );
    assert_eq!(
        trust_fixture_peer(&target, 91, "diverge-source"),
        source_node
    );

    let domain = source.default_domain(&actor).unwrap();
    source
        .register_remote_machine(&actor, Some(&domain.id), &target_node, None)
        .unwrap();
    let workspace = source
        .create_workspace(&actor, Some(&domain.id), "divergence-workspace")
        .unwrap();
    let (session, branch) = source
        .create_in_workspace(
            &actor,
            "divergence session",
            binding(),
            Some(&domain.id),
            Some(&workspace.id),
            None,
        )
        .unwrap();
    let base = source
        .checkpoint(
            &actor,
            &session.id,
            &branch.id,
            branch.revision,
            "shared base",
        )
        .unwrap();

    let remote_writer = source
        .failover_workspace_writer(
            &actor,
            Some(&domain.id),
            &workspace.id,
            1,
            &target_node,
            "divergence_handoff",
        )
        .unwrap();
    let initial = source
        .export_workspace_replica_snapshot(&actor, &workspace.id, &target_node)
        .unwrap();
    let initial_signature = STANDARD.encode(
        fixture_authority_key(91)
            .sign(&aw_store::workspace_replica_snapshot_transcript(&initial))
            .to_bytes(),
    );
    target
        .import_workspace_replica_snapshot(&actor, &source_node, &initial, &initial_signature)
        .unwrap();
    let stable_actor =
        aw_store::stable_user_principal(&source.current_user(&actor).unwrap().user_id).unwrap();

    let remote_branch = target
        .replica_writer_branches(
            &source_node,
            &stable_actor,
            &workspace.id,
            remote_writer.writer_epoch,
            remote_writer.authority_term,
            &session.id,
        )
        .unwrap()
        .into_iter()
        .find(|b| b.id == branch.id)
        .unwrap();
    assert_eq!(remote_branch.head, base.head);
    let remote_descendant = target
        .replica_writer_checkpoint(
            &source_node,
            &stable_actor,
            &workspace.id,
            remote_writer.writer_epoch,
            remote_writer.authority_term,
            &session.id,
            &branch.id,
            &remote_branch.head,
            remote_branch.revision,
            "remote epoch-2 descendant",
        )
        .unwrap();

    let local_writer = source
        .failover_workspace_writer(
            &actor,
            Some(&domain.id),
            &workspace.id,
            2,
            &source_node,
            "partition_failback",
        )
        .unwrap();
    assert_eq!(local_writer.writer_epoch, 3);
    let source_before = source
        .branches(&actor, &session.id)
        .unwrap()
        .into_iter()
        .find(|b| b.id == branch.id)
        .unwrap();
    assert_eq!(source_before.head, base.head);
    let local_descendant = source
        .checkpoint(
            &actor,
            &session.id,
            &branch.id,
            source_before.revision,
            "local epoch-3 descendant",
        )
        .unwrap();
    assert_ne!(local_descendant.head, remote_descendant.head);
    let fence_event = source
        .workspace_writer_events(&actor, Some(&domain.id), &workspace.id)
        .unwrap()
        .last()
        .cloned()
        .unwrap();
    let fenced = target
        .replica_writer_fence(&source_node, &workspace.id, &fence_event)
        .unwrap();
    assert_eq!(fenced.writer_epoch, 3);
    assert_eq!(fenced.writer_node_id, source_node);

    let stale_writer_snapshot = target
        .export_workspace_replica_snapshot(&actor, &workspace.id, &source_node)
        .unwrap();
    assert_eq!(stale_writer_snapshot.writer.writer_epoch, 3);
    assert_eq!(stale_writer_snapshot.ref_writer_epoch, 2);
    let stale_signature = STANDARD.encode(
        fixture_authority_key(92)
            .sign(&aw_store::workspace_replica_snapshot_transcript(
                &stale_writer_snapshot,
            ))
            .to_bytes(),
    );
    let applied = source
        .import_workspace_replica_snapshot(
            &actor,
            &target_node,
            &stale_writer_snapshot,
            &stale_signature,
        )
        .unwrap();
    assert_eq!(applied.writer.writer_epoch, 3);
    assert_eq!(applied.divergences_preserved, 1);

    let branches = source.branches(&actor, &session.id).unwrap();
    let canonical = branches.iter().find(|b| b.id == branch.id).unwrap();
    assert_eq!(canonical.head, local_descendant.head);
    let divergence = source.workspace_replica_divergences(&workspace.id).unwrap();
    assert_eq!(divergence.len(), 1);
    assert_eq!(divergence[0].local_writer_epoch, 3);
    assert_eq!(divergence[0].incoming_writer_epoch, 2);
    assert_eq!(divergence[0].local_head, local_descendant.head);
    assert_eq!(divergence[0].incoming_head, remote_descendant.head);
    let preserved = branches
        .iter()
        .find(|b| b.id == divergence[0].preserved_branch_id)
        .unwrap();
    assert_eq!(preserved.head, remote_descendant.head);
}

#[test]
fn identity_continuity_authenticates_same_stable_user_across_workspace_replica() {
    let source = SessionStore::in_memory().unwrap();
    let target = SessionStore::in_memory().unwrap();
    let actor = actor();
    let source_node = bind_fixture_node(&source, 101);
    let target_node = bind_fixture_node(&target, 102);
    assert_eq!(
        trust_fixture_peer(&source, 102, "identity-target"),
        target_node
    );
    assert_eq!(
        trust_fixture_peer(&target, 101, "identity-source"),
        source_node
    );

    let source_user = source.current_user(&actor).unwrap();
    let stable_actor = aw_store::stable_user_principal(&source_user.user_id).unwrap();
    let device_key = fixture_authority_key(103);
    let device_public = device_key
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let device = source
        .register_identity_device_key(&actor, &device_public)
        .unwrap();
    assert_eq!(device.user_id, source_user.user_id);
    let after_device = source.current_user(&actor).unwrap();
    assert!(after_device.security_epoch > source_user.security_epoch);

    let oidc = source
        .link_oidc_identity(
            &actor,
            Some("fixture-oidc"),
            "https://issuer.example.invalid",
            "subject-123",
        )
        .unwrap();
    assert_eq!(oidc.user_id, source_user.user_id);
    let other = PrincipalId::new("internal:fixture:other").unwrap();
    source.current_user(&other).unwrap();
    assert!(source
        .link_oidc_identity(
            &other,
            Some("another-label-does-not-matter"),
            "https://issuer.example.invalid",
            "subject-123",
        )
        .is_err());

    let domain = source.default_domain(&actor).unwrap();
    source
        .register_remote_machine(
            &actor,
            Some(&domain.id),
            &target_node,
            Some("identity-target"),
        )
        .unwrap();
    let workspace = source
        .create_workspace(&actor, Some(&domain.id), "identity-workspace")
        .unwrap();
    let (session, branch) = source
        .create_in_workspace(
            &actor,
            "identity replicated session",
            binding(),
            Some(&domain.id),
            Some(&workspace.id),
            None,
        )
        .unwrap();
    source
        .checkpoint(
            &actor,
            &session.id,
            &branch.id,
            branch.revision,
            "identity continuity fixture",
        )
        .unwrap();
    // Stable user principals are already authorization identities locally; the
    // local Unix/internal label is only one binding to the same user.
    assert_eq!(
        source.session(&stable_actor, &session.id).unwrap().id,
        session.id
    );

    let remote_writer = source
        .failover_workspace_writer(
            &actor,
            Some(&domain.id),
            &workspace.id,
            1,
            &target_node,
            "identity_continuity_handoff",
        )
        .unwrap();
    let workspace_snapshot = source
        .export_workspace_replica_snapshot(&actor, &workspace.id, &target_node)
        .unwrap();
    let workspace_signature = STANDARD.encode(
        fixture_authority_key(101)
            .sign(&aw_store::workspace_replica_snapshot_transcript(
                &workspace_snapshot,
            ))
            .to_bytes(),
    );
    let workspace_applied = target
        .import_workspace_replica_snapshot(
            &actor,
            &source_node,
            &workspace_snapshot,
            &workspace_signature,
        )
        .unwrap();
    assert_eq!(
        workspace_applied.writer.writer_epoch,
        remote_writer.writer_epoch
    );
    assert_eq!(workspace_applied.writer.writer_node_id, target_node);

    let grant = target
        .identity_continuity_accept(&actor, &domain.id, &source_node)
        .unwrap();
    assert_eq!(grant.source_node_id, source_node);
    let snapshot = source
        .export_identity_continuity_snapshot(&actor, &domain.id, &target_node)
        .unwrap();
    assert!(snapshot.users.iter().any(|u| {
        u.user.user_id == source_user.user_id
            && u.bindings.iter().any(|b| b.kind == "device_key")
            && u.bindings.iter().any(|b| b.kind == "oidc")
    }));
    let source_signature = STANDARD.encode(
        fixture_authority_key(101)
            .sign(&aw_store::identity_continuity_snapshot_transcript(
                &snapshot,
            ))
            .to_bytes(),
    );
    let applied = target
        .import_identity_continuity_snapshot(&actor, &source_node, &snapshot, &source_signature)
        .unwrap();
    assert!(!applied.reused);
    assert!(applied.active_bindings >= 2);

    let transcript = b"phase30-user-device-auth-proof";
    let proof = STANDARD.encode(device_key.sign(transcript).to_bytes());
    let authenticated = target
        .authenticate_portable_device_signature(&domain.id, &device_public, transcript, &proof)
        .unwrap();
    assert_eq!(authenticated.user_id, source_user.user_id);
    assert_eq!(authenticated.stable_principal, stable_actor.as_str());
    let authenticated_actor = PrincipalId::new(authenticated.stable_principal.clone()).unwrap();
    assert_eq!(
        target
            .session(&authenticated_actor, &session.id)
            .unwrap()
            .id,
        session.id
    );
    assert_eq!(
        target
            .branches(&authenticated_actor, &session.id)
            .unwrap()
            .len(),
        1
    );
    let trust_bound_session = target
        .issue_auth_session_from_portable_auth(
            &authenticated,
            600,
            serde_json::json!({"client":"phase31-trust-bound"}),
        )
        .unwrap();
    assert_eq!(
        target
            .authenticate_auth_session(&trust_bound_session.refresh_token)
            .unwrap()
            .session
            .user_id,
        source_user.user_id
    );

    // Source-trust revocation is immediate: imported metadata remains durable
    // evidence, but it cannot authenticate while the continuity grant is revoked.
    let revoked_grant = target
        .revoke_identity_continuity_grant(&actor, &grant.id)
        .unwrap();
    assert_eq!(revoked_grant.status, "revoked");
    assert!(target
        .authenticate_portable_device_signature(&domain.id, &device_public, transcript, &proof)
        .is_err());
    assert!(target
        .authenticate_auth_session(&trust_bound_session.refresh_token)
        .is_err());
    let reaccepted = target
        .identity_continuity_accept(&actor, &domain.id, &source_node)
        .unwrap();
    assert_eq!(reaccepted.id, grant.id);
    assert_eq!(reaccepted.status, "active");
    assert!(reaccepted.revision > grant.revision);
    // Re-accepting the source restores trust, not credentials. A fresh signed
    // sync is required so credentials revoked while trust was off cannot revive.
    assert!(target
        .authenticate_portable_device_signature(&domain.id, &device_public, transcript, &proof)
        .is_err());
    let reapplied = target
        .import_identity_continuity_snapshot(&actor, &source_node, &snapshot, &source_signature)
        .unwrap();
    assert!(reapplied.reused);
    assert!(reapplied.active_bindings >= 2);
    let reauthenticated = target
        .authenticate_portable_device_signature(&domain.id, &device_public, transcript, &proof)
        .unwrap();
    assert_eq!(reauthenticated.user_id, source_user.user_id);
    assert!(target
        .authenticate_auth_session(&trust_bound_session.refresh_token)
        .is_err());
    let epoch_bound_session = target
        .issue_auth_session_from_portable_auth(
            &reauthenticated,
            600,
            serde_json::json!({"client":"phase31-epoch-bound"}),
        )
        .unwrap();

    // OIDC revocation is explicit and portable. It advances the stable user's
    // security epoch; after resync the OIDC binding disappears while the active
    // device key remains usable under the new epoch.
    let revoked_oidc = source
        .revoke_oidc_identity(&actor, &oidc.identity_id)
        .unwrap();
    assert_eq!(revoked_oidc.status, "revoked");
    let oidc_revoked_snapshot = source
        .export_identity_continuity_snapshot(&actor, &domain.id, &target_node)
        .unwrap();
    let oidc_revoked_signature = STANDARD.encode(
        fixture_authority_key(101)
            .sign(&aw_store::identity_continuity_snapshot_transcript(
                &oidc_revoked_snapshot,
            ))
            .to_bytes(),
    );
    target
        .import_identity_continuity_snapshot(
            &actor,
            &source_node,
            &oidc_revoked_snapshot,
            &oidc_revoked_signature,
        )
        .unwrap();
    let bindings = target
        .portable_identity_bindings(&actor, &domain.id)
        .unwrap();
    assert!(!bindings.iter().any(|binding| {
        binding.user_id == source_user.user_id
            && binding.kind == "oidc"
            && binding.status == "active"
    }));
    let post_oidc_auth = target
        .authenticate_portable_device_signature(&domain.id, &device_public, transcript, &proof)
        .unwrap();
    assert_eq!(post_oidc_auth.user_id, source_user.user_id);
    assert!(target
        .authenticate_auth_session(&epoch_bound_session.refresh_token)
        .is_err());
    let binding_bound_session = target
        .issue_auth_session_from_portable_auth(
            &post_oidc_auth,
            600,
            serde_json::json!({"client":"phase31-binding-bound"}),
        )
        .unwrap();

    source
        .revoke_identity_device_key(&actor, &device.device_id)
        .unwrap();
    let revoked_snapshot = source
        .export_identity_continuity_snapshot(&actor, &domain.id, &target_node)
        .unwrap();
    let revoked_signature = STANDARD.encode(
        fixture_authority_key(101)
            .sign(&aw_store::identity_continuity_snapshot_transcript(
                &revoked_snapshot,
            ))
            .to_bytes(),
    );
    target
        .import_identity_continuity_snapshot(
            &actor,
            &source_node,
            &revoked_snapshot,
            &revoked_signature,
        )
        .unwrap();
    assert!(target
        .authenticate_portable_device_signature(&domain.id, &device_public, transcript, &proof,)
        .is_err());
    assert!(target
        .authenticate_auth_session(&binding_bound_session.refresh_token)
        .is_err());
}
