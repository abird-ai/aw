// Native W8 risk/owner/concurrency/real-process tests over ORIGINAL P/Q/R/S/T.
// Included inside the existing composite_staging::tests fixture module so
// the original TestRoot/SessionStore/SecretStore fixtures are reused without
// inventing another backup, SQLite schema or execution harness.

#[cfg(target_os = "linux")]
fn aw_p6_10w8_pending_queue_plan(
    archive: &Path,
    cold: &Path,
) -> crate::cold_effect_reconciliation::review::ColdEffectReviewPlan {
    use crate::cold_effect_reconciliation::review::{
        prepare_exact_original_effect_review, ColdEffectSubject,
    };
    let snapshot =
        crate::cold_effect_reconciliation::prepare_consistent_cold_effect_snapshot(archive, cold)
            .unwrap();
    let item = snapshot
        .actor
        .iter()
        .find(|e| e.kind == crate::cold_effect_reconciliation::EvidenceClass::QueuePending)
        .unwrap();
    prepare_exact_original_effect_review(
        archive,
        cold,
        &ColdEffectSubject::Actor {
            kind: item.kind,
            session_id: item.session_id.clone(),
            request_id: item.request_id.clone(),
        },
    )
    .unwrap()
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_10w8_real_owner_effect_review_is_not_settlement_or_activation() {
    use crate::cold_effect_reconciliation::review::{
        record_original_owner_effect_review, ColdEffectReviewIntent,
    };
    let (_root, archive, cold) = aw_p6_10w7_real_cold_fixture();
    let plan = aw_p6_10w8_pending_queue_plan(&archive, &cold);
    let owner = PrincipalId::new("local").unwrap();
    assert!(!plan.runnable && !plan.permits_original_effect_replay);
    let receipt = record_original_owner_effect_review(
        &archive,
        &cold,
        &owner,
        "w8-original-pending-one",
        &plan,
        ColdEffectReviewIntent::KeepQuarantined,
    )
    .unwrap();
    assert!(
        !receipt.runnable && !receipt.original_effect_settled && !receipt.fresh_authority_granted
    );
    assert_eq!(
        receipt.original_evidence_sha256,
        plan.original_evidence_sha256
    );
    let reopened = aw_p6_10w8_pending_queue_plan(&archive, &cold);
    assert_eq!(
        reopened, plan,
        "review must not change original pending Queue or original risk facts"
    );
    let db = Connection::open_with_flags(
        cold.join("sessions.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let events: i64 = db.query_row(
        "SELECT count(*) FROM local_coordination_events WHERE kind='store.cold_effect_original_owner_review_requested'",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(events, 1);
    let json: String = db.query_row(
        "SELECT payload_json FROM local_coordination_events WHERE kind='store.cold_effect_original_owner_review_requested' LIMIT 1",
        [], |r| r.get(0),
    ).unwrap();
    let body: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(body["activation_authorized"], false);
    assert_eq!(body["independent_live_human_attested"], false);
    assert_eq!(body["unknown_provider_cost_preserved"], true);
    assert_eq!(body["automatic_replay_allowed"], false);
    assert!(SessionStore::open(&cold).is_err());
    assert!(
        record_original_owner_effect_review(
            &archive,
            &cold,
            &owner,
            "w8-original-pending-one",
            &plan,
            ColdEffectReviewIntent::KeepQuarantined,
        )
        .is_err(),
        "duplicate original owner request must not re-audit"
    );
    assert!(
        record_original_owner_effect_review(
            &archive,
            &cold,
            &owner,
            "w8-another-request-same-effect",
            &plan,
            ColdEffectReviewIntent::RequestFreshProposalReview,
        )
        .is_err(),
        "new request ID cannot review original same effect twice"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn aw_p6_w8c_original_interrupted_tool_survives_cold_review_without_replay() {
    aw_p6_original_unresolved_tool_review(aw_agent_core::ToolStatus::Interrupted, None).await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn aw_p6_w8g_original_missing_tool_output_survives_cold_review_without_replay() {
    for status in [
        aw_agent_core::ToolStatus::Succeeded,
        aw_agent_core::ToolStatus::Failed,
    ] {
        aw_p6_original_unresolved_tool_review(status, None).await;
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn aw_p6_w8h_succeeded_error_output_survives_original_cold_review() {
    aw_p6_original_unresolved_tool_review(
        aw_agent_core::ToolStatus::Succeeded,
        Some(aw_agent_core::ToolOutput::error("")),
    )
    .await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn aw_p6_w8h_failed_nonerror_output_survives_original_cold_review() {
    aw_p6_original_unresolved_tool_review(
        aw_agent_core::ToolStatus::Failed,
        Some(aw_agent_core::ToolOutput::text("")),
    )
    .await;
}

#[cfg(target_os = "linux")]
async fn aw_p6_original_unresolved_tool_review(
    status: aw_agent_core::ToolStatus,
    output: Option<aw_agent_core::ToolOutput>,
) {
    use crate::cold_effect_reconciliation::{
        prepare_consistent_cold_effect_snapshot,
        review::{
            prepare_exact_original_effect_review, record_original_owner_effect_review,
            ColdEffectReviewIntent, ColdEffectSubject,
        },
        EvidenceClass,
    };
    use aw_agent_core::{
        Agent, AgentOptions, CompletionFuture, DeltaSink, JournalEntry, Message, Outcome, Provider,
        Role, ToolCall, ToolEffect, ToolRecord, ToolSourceEvidence, ToolStatus, Turn, TurnJournal,
        TurnStart,
    };
    struct UnusedProvider;
    impl Provider for UnusedProvider {
        fn stream<'a>(
            &'a self,
            _: &'a [Message],
            _: &'a str,
            _: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async { panic!("cold Tool review must never dispatch a provider") })
        }
    }
    let root = TempRoot::new();
    let source = root.child("w8-outputless-original");
    let bundles = root.child("w8-outputless-bundles");
    let parent = root.child("w8-outputless-cold-parent");
    let store = SessionStore::open(&source).unwrap();
    let agent = Agent::builder(
        std::sync::Arc::new(UnusedProvider),
        AgentOptions::new("fixture", "m"),
    )
    .build()
    .unwrap();
    let owner = agent.actor().clone();
    let identity = agent.default_identity().unwrap();
    let (session, branch) = store
        .create(&owner, "original missing Tool output", binding())
        .unwrap();
    let (_, journal) = store.activate(&owner, &session.id, &branch.id).unwrap();
    journal
        .begin(TurnStart {
            turn_id: 1,
            prompt: "original effect".into(),
            identity: identity.clone(),
        })
        .await
        .unwrap();
    let call = ToolCall {
        id: "w8-outputless-original-effect".into(),
        name: "bash".into(),
        arguments: serde_json::json!({"command":"synthetic; never executed"}),
    };
    let mut proposed = Message::text(1025, Role::Assistant, "");
    proposed.tool_calls.push(call.clone());
    journal
        .append(
            1,
            JournalEntry::Assistant {
                message: proposed.clone(),
            },
        )
        .await
        .unwrap();
    let started = ToolRecord {
        call,
        effect: ToolEffect::Process,
        execution_site: "local".into(),
        status: ToolStatus::Running,
        output: None,
    };
    journal
        .append(
            1,
            JournalEntry::ToolStarted {
                record: started.clone(),
                source: Some(ToolSourceEvidence::trusted_host()),
            },
        )
        .await
        .unwrap();
    // The ORIGINAL journal API keeps legacy terminal facts readable.
    // No OS Tool or provider is dispatched by this synthetic fixture.
    let mut finished = started;
    finished.status = status;
    finished.output = output;
    journal
        .append(
            1,
            JournalEntry::ToolFinished {
                record: finished.clone(),
            },
        )
        .await
        .unwrap();
    journal
        .finish(Turn {
            id: 1,
            prompt: "original effect".into(),
            answer: String::new(),
            model: identity.model,
            profile: identity.account_id,
            outcome: Outcome::Failed,
            meta: None,
            error: Some("external outcome remains unknown".into()),
            actor: Some(owner.clone()),
            invocations: vec![],
            tool_records: vec![finished],
            context_messages: vec![Message::text(1024, Role::User, "original effect"), proposed],
            needs_reconciliation: true,
        })
        .await
        .unwrap();
    let archive = capture_partial(&store, &bundles, |_, _| Ok(()))
        .unwrap()
        .publish_complete(&store, "aw-composite-w8-outputless", |_, _| Ok(()))
        .unwrap();
    drop(journal);
    drop(store);
    let cold = parent.join("aw-restored-w8-outputless");
    restore_complete_cold_quarantined(&archive, &cold, |_, _| Ok(())).unwrap();
    let inspection =
        crate::cold_quarantine_review::inspect_quarantined_cold_restore(&archive, &cold).unwrap();
    assert_eq!(inspection.incomplete_tool_intents, 1);
    assert_eq!(inspection.mismatched_tool_receipts, 0);
    assert!(inspection.outstanding_review && !inspection.activation_authorized);
    let snapshot = prepare_consistent_cold_effect_snapshot(&archive, &cold).unwrap();
    assert_eq!(snapshot.tools.len(), 1);
    let effect = &snapshot.tools[0];
    assert_eq!(effect.kind, EvidenceClass::ToolIntent);
    assert_eq!(effect.call_id, "w8-outputless-original-effect");
    assert_eq!(effect.original_site, "local");
    assert_eq!(
        (effect.starts, effect.finishes),
        (1, 1),
        "retain factual receipt multiplicities"
    );
    assert_eq!(
        effect.original_source,
        Some(ToolSourceEvidence::trusted_host())
    );
    let plan = prepare_exact_original_effect_review(
        &archive,
        &cold,
        &ColdEffectSubject::Tool {
            kind: effect.kind,
            run_id: effect.run_id.clone(),
            call_id: effect.call_id.clone(),
        },
    )
    .unwrap();
    let db = Connection::open_with_flags(
        cold.join("sessions.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let original_rows = || {
        let mut q = db
            .prepare("SELECT payload FROM run_events ORDER BY seq")
            .unwrap();
        q.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    let before = original_rows();
    let receipt = record_original_owner_effect_review(
        &archive,
        &cold,
        &owner,
        "w8-outputless-original-missing-output-review",
        &plan,
        ColdEffectReviewIntent::KeepQuarantined,
    )
    .unwrap();
    assert!(
        !receipt.original_effect_settled && !receipt.runnable && !receipt.fresh_authority_granted
    );
    assert_eq!(original_rows(), before);
    assert_eq!(
        prepare_exact_original_effect_review(&archive, &cold, &plan.subject).unwrap(),
        plan
    );
    assert!(SessionStore::open(&cold).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_w8d_original_t_inspection_rejects_postread_custody_changes() {
    use crate::cold_quarantine_review::inspect_quarantined_cold_restore_with_hook;
    for changed in ["quarantine", "sqlite"] {
        let (root, archive, cold) = aw_p6_10w7_real_cold_fixture();
        let saved = root.child("w8d-preserved-original");
        let result = inspect_quarantined_cold_restore_with_hook(&archive, &cold, || {
            let path = cold.join(if changed == "quarantine" {
                COLD_QUARANTINE_MARKER
            } else {
                "sessions.sqlite3"
            });
            let original = saved.join("original");
            fs::rename(&path, &original)?;
            if changed == "sqlite" {
                fs::copy(&original, &path)?;
            }
            Ok(())
        });
        assert!(
            result.is_err(),
            "original T must reject postread {changed} replacement/removal"
        );
        assert!(
            saved.join("original").is_file(),
            "fault must reach the postread boundary"
        );
        if changed == "sqlite" {
            assert_eq!(
                fs::read(saved.join("original")).unwrap(),
                fs::read(cold.join("sessions.sqlite3")).unwrap()
            );
        }
        assert!(SessionStore::open(&cold).is_err());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_w8f_original_t_precommit_denial_has_no_audit() {
    use crate::cold_quarantine_review::{
        record_quarantine_review_request, record_quarantine_review_request_with_hooks,
        ColdReviewIntent,
    };
    for fault in ["callback", "generation", "quarantine"] {
        let (_root, archive, cold) = aw_p6_10w7_real_cold_fixture();
        let owner = PrincipalId::new("local").unwrap();
        let error = record_quarantine_review_request_with_hooks(
            &archive,
            &cold,
            &owner,
            "w8f-original-t-precommit",
            ColdReviewIntent::KeepQuarantined,
            |state| {
                match fault {
                    "callback" => {
                        return Err(aw_agent_core::Error::Denied("synthetic T precommit denial"))
                    }
                    "generation" => {
                        let db = crate::db(Connection::open(state.join("sessions.sqlite3")))?;
                        crate::db(db.execute(
                            "UPDATE store_meta SET value='999' WHERE key='restore_generation'",
                            [],
                        ))?;
                    }
                    _ => fs::remove_file(state.join(COLD_QUARANTINE_MARKER))?,
                }
                Ok(())
            },
            |_| panic!("rejected original T request must not reach COMMIT"),
        )
        .unwrap_err();
        assert!(
            if fault == "quarantine" {
                matches!(error, aw_agent_core::Error::Io(_))
            } else {
                matches!(error, aw_agent_core::Error::Denied(_))
            },
            "{fault}: original precommit error must not become UNKNOWN: {error:?}"
        );
        let db = Connection::open_with_flags(
            cold.join("sessions.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let count: i64 = db.query_row("SELECT count(*) FROM local_coordination_events WHERE kind='store.cold_quarantine_review_requested'", [], |row| row.get(0)).unwrap();
        assert_eq!(count, 0);
        drop(db);
        if fault == "callback" {
            let report = record_quarantine_review_request(
                &archive,
                &cold,
                &owner,
                "w8f-original-t-precommit",
                ColdReviewIntent::KeepQuarantined,
            )
            .unwrap();
            assert!(!report.activation_authorized);
            assert_eq!(report.queued_pending, 1);
            let db = Connection::open_with_flags(
                cold.join("sessions.sqlite3"),
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let payload: String = db.query_row("SELECT payload_json FROM local_coordination_events WHERE kind='store.cold_quarantine_review_requested'", [], |row| row.get(0)).unwrap();
            let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
            assert_eq!(payload["review_version"], 1);
            assert_eq!(payload["activation_authorized"], false);
            assert_eq!(payload["pending_queued_inputs"], 1);
        }
        assert!(SessionStore::open(&cold).is_err());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_w8f_original_t_postcommit_fault_and_denial_loss_are_unknown() {
    use crate::cold_quarantine_review::{
        record_quarantine_review_request, record_quarantine_review_request_with_hooks,
        ColdReviewIntent,
    };
    for fault in ["callback", "quarantine"] {
        let (_root, archive, cold) = aw_p6_10w7_real_cold_fixture();
        let owner = PrincipalId::new("local").unwrap();
        let error = record_quarantine_review_request_with_hooks(
            &archive,
            &cold,
            &owner,
            "w8f-original-t-postcommit",
            ColdReviewIntent::KeepQuarantined,
            |_| Ok(()),
            |state| {
                if fault == "callback" {
                    return Err(aw_agent_core::Error::Denied(
                        "synthetic T postcommit sync fault",
                    ));
                }
                fs::remove_file(state.join(COLD_QUARANTINE_MARKER))?;
                Ok(())
            },
        )
        .unwrap_err();
        assert!(
            matches!(&error, aw_agent_core::Error::Storage(message)
            if message.contains("outcome unknown") && message.contains("inspect")),
            "{fault}: committed original T audit must not look rolled back: {error:?}"
        );
        let db = Connection::open_with_flags(
            cold.join("sessions.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let count: i64 = db.query_row("SELECT count(*) FROM local_coordination_events WHERE kind='store.cold_quarantine_review_requested'", [], |row| row.get(0)).unwrap();
        assert_eq!(count, 1);
        assert!(record_quarantine_review_request(
            &archive,
            &cold,
            &owner,
            "w8f-original-t-postcommit",
            ColdReviewIntent::KeepQuarantined
        )
        .is_err());
        assert!(SessionStore::open(&cold).is_err());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_w8f_original_t_replaced_audit_is_never_acknowledged() {
    use crate::cold_quarantine_review::{
        record_quarantine_review_request_with_hooks, ColdReviewIntent,
    };
    for mode in ["old-path", "old-in-place", "committed-copy"] {
        let (root, archive, cold) = aw_p6_10w7_real_cold_fixture();
        let original_plan = aw_p6_10w8_pending_queue_plan(&archive, &cold);
        let saved = root.child("w8f-original-t-preserved");
        let before = saved.join("before.sqlite3");
        let committed = saved.join("committed.sqlite3");
        let result = record_quarantine_review_request_with_hooks(
            &archive,
            &cold,
            &PrincipalId::new("local").unwrap(),
            "w8f-original-t-replaced",
            ColdReviewIntent::KeepQuarantined,
            |state| {
                fs::copy(state.join("sessions.sqlite3"), &before)?;
                Ok(())
            },
            |state| {
                let sqlite = state.join("sessions.sqlite3");
                if mode == "old-in-place" {
                    fs::copy(&sqlite, &committed)?;
                    fs::write(&sqlite, fs::read(&before)?)?;
                } else {
                    fs::rename(&sqlite, &committed)?;
                    fs::copy(
                        if mode == "committed-copy" {
                            &committed
                        } else {
                            &before
                        },
                        &sqlite,
                    )?;
                }
                Ok(())
            },
        );
        let error = result.unwrap_err();
        assert!(
            matches!(&error, aw_agent_core::Error::Storage(message)
            if message.contains("outcome unknown") && message.contains("inspect")),
            "{mode}: original T cannot acknowledge replaced/lost audit: {error:?}"
        );
        for (path, expected) in [
            (&committed, 1),
            (
                &cold.join("sessions.sqlite3"),
                i64::from(mode == "committed-copy"),
            ),
        ] {
            let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            let count: i64 = db.query_row("SELECT count(*) FROM local_coordination_events WHERE kind='store.cold_quarantine_review_requested'", [], |row| row.get(0)).unwrap();
            assert_eq!(count, expected, "{mode}: preserve original committed audit");
        }
        assert_eq!(
            aw_p6_10w8_pending_queue_plan(&archive, &cold),
            original_plan
        );
        assert!(SessionStore::open(&cold).is_err());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_w8_precommit_denial_retains_original_error_and_no_audit_record() {
    use crate::cold_effect_reconciliation::review::{
        record_original_owner_effect_review, record_original_owner_effect_review_with_crash_seams,
        ColdEffectReviewIntent,
    };
    let (_root, archive, cold) = aw_p6_10w7_real_cold_fixture();
    let plan = aw_p6_10w8_pending_queue_plan(&archive, &cold);
    let owner = PrincipalId::new("local").unwrap();
    let error = record_original_owner_effect_review_with_crash_seams(
        &archive,
        &cold,
        &owner,
        "w8-precommit-decision",
        &plan,
        ColdEffectReviewIntent::KeepQuarantined,
        |_| Err(aw_agent_core::Error::Denied("synthetic BEFORE transaction")),
        |_| panic!("precommit rejection must not reach after-commit callback"),
    )
    .unwrap_err();
    assert!(
        matches!(error, aw_agent_core::Error::Denied(_)),
        "pretransaction denial must not be mislabeled as postcommit unknown"
    );
    let c = Connection::open_with_flags(
        cold.join("sessions.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let count: i64 = c.query_row(
        "SELECT count(*) FROM local_coordination_events WHERE kind='store.cold_effect_original_owner_review_requested'",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(count, 0);
    drop(c);
    let reviewed = record_original_owner_effect_review(
        &archive,
        &cold,
        &owner,
        "w8-new-owner-request-after-rejection",
        &plan,
        ColdEffectReviewIntent::KeepQuarantined,
    )
    .unwrap();
    assert!(!reviewed.runnable && !reviewed.original_effect_settled);
    assert!(SessionStore::open(&cold).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_w8_postcommit_quarantine_corruption_reports_unknown_after_original_audit() {
    use crate::cold_effect_reconciliation::review::{
        record_original_owner_effect_review_with_crash_seams, ColdEffectReviewIntent,
    };
    let (_root, archive, cold) = aw_p6_10w7_real_cold_fixture();
    let plan = aw_p6_10w8_pending_queue_plan(&archive, &cold);
    let owner = PrincipalId::new("local").unwrap();
    let error = record_original_owner_effect_review_with_crash_seams(
        &archive,
        &cold,
        &owner,
        "w8-postcommit-denial-missing",
        &plan,
        ColdEffectReviewIntent::KeepQuarantined,
        |_| Ok(()),
        |state| {
            fs::remove_file(state.join(COLD_QUARANTINE_MARKER)).unwrap();
            Ok(())
        },
    )
    .unwrap_err();
    match error {
        aw_agent_core::Error::Storage(message) => {
            assert!(message.contains("outcome unknown") && message.contains("inspect"));
        }
        other => panic!("postcommit missing marker must be an uncertain audit outcome: {other:?}"),
    }
    let c = Connection::open_with_flags(
        cold.join("sessions.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let count: i64 = c.query_row(
        "SELECT count(*) FROM local_coordination_events WHERE kind='store.cold_effect_original_owner_review_requested'",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(count, 1);
    assert!(
        SessionStore::open(&cold).is_err(),
        "removed filesystem marker cannot erase original SQLite quarantine"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_w8b_postcommit_database_replacement_never_acknowledges_review() {
    use crate::cold_effect_reconciliation::review::{
        record_original_owner_effect_review_with_crash_seams, ColdEffectReviewIntent,
    };
    for mode in ["old-path", "old-in-place", "committed-copy"] {
        let (root, archive, cold) = aw_p6_10w7_real_cold_fixture();
        let plan = aw_p6_10w8_pending_queue_plan(&archive, &cold);
        // Preserve both versions outside the quarantined State. These are
        // deliberate filesystem faults, never a recovery or replay procedure.
        let saved = root.child("w8b-saved");
        let before = saved.join("before.sqlite3");
        let committed = saved.join("committed.sqlite3");
        let result = record_original_owner_effect_review_with_crash_seams(
            &archive,
            &cold,
            &PrincipalId::new("local").unwrap(),
            "w8b-replaced-database",
            &plan,
            ColdEffectReviewIntent::KeepQuarantined,
            |state| {
                fs::copy(state.join("sessions.sqlite3"), &before)?;
                Ok(())
            },
            |state| {
                let sqlite = state.join("sessions.sqlite3");
                if mode == "old-in-place" {
                    fs::copy(&sqlite, &committed)?;
                    // Same inode: exact audit evidence must be re-read even
                    // when the original object/custody checks still pass.
                    fs::write(&sqlite, fs::read(&before)?)?;
                } else {
                    fs::rename(&sqlite, &committed)?;
                    fs::copy(
                        if mode == "committed-copy" {
                            &committed
                        } else {
                            &before
                        },
                        &sqlite,
                    )?;
                }
                Ok(())
            },
        );
        let error = result.unwrap_err();
        assert!(
            matches!(&error, aw_agent_core::Error::Storage(message)
            if message.contains("outcome unknown") && message.contains("inspect")),
            "{mode} must never acknowledge a replaced or lost audit: {error:?}"
        );
        for (path, expected) in [
            (&committed, 1),
            (
                &cold.join("sessions.sqlite3"),
                i64::from(mode == "committed-copy"),
            ),
        ] {
            let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            let count: i64 = db.query_row(
                "SELECT count(*) FROM local_coordination_events WHERE kind='store.cold_effect_original_owner_review_requested'",
                [], |row| row.get(0),
            ).unwrap();
            assert_eq!(
                count, expected,
                "{mode}: preserve the original committed audit"
            );
        }
        assert_eq!(aw_p6_10w8_pending_queue_plan(&archive, &cold), plan);
        assert!(SessionStore::open(&cold).is_err());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_w8_postcommit_fault_is_explicitly_unknown_not_a_retryable_rollback() {
    use crate::cold_effect_reconciliation::review::{
        record_original_owner_effect_review, record_original_owner_effect_review_with_crash_seams,
        ColdEffectReviewIntent,
    };
    let (_root, archive, cold) = aw_p6_10w7_real_cold_fixture();
    let plan = aw_p6_10w8_pending_queue_plan(&archive, &cold);
    let owner = PrincipalId::new("local").unwrap();
    let error = record_original_owner_effect_review_with_crash_seams(
        &archive,
        &cold,
        &owner,
        "w8-postcommit-unknown",
        &plan,
        ColdEffectReviewIntent::KeepQuarantined,
        |_| Ok(()),
        |_| {
            Err(aw_agent_core::Error::Denied(
                "synthetic postcommit sync failure",
            ))
        },
    )
    .unwrap_err();
    match error {
        aw_agent_core::Error::Storage(message) => {
            assert!(
                message.contains("outcome unknown"),
                "after-commit error must disclose uncertain result, not imply rollback: {message}"
            );
            assert!(message.contains("inspect"));
        }
        other => panic!("incorrect post-commit uncertainty status: {other:?}"),
    }
    let c = Connection::open_with_flags(
        cold.join("sessions.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let count: i64 = c.query_row(
        "SELECT count(*) FROM local_coordination_events WHERE kind='store.cold_effect_original_owner_review_requested'",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(
        count, 1,
        "postcommit error did not roll back already persisted original audit"
    );
    drop(c);
    assert!(
        record_original_owner_effect_review(
            &archive,
            &cold,
            &owner,
            "w8-retry-after-uncertain",
            &plan,
            ColdEffectReviewIntent::RequestFreshProposalReview,
        )
        .is_err(),
        "a different request ID must not reappend uncertain original review"
    );
    assert!(SessionStore::open(&cold).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_10w8_two_offline_clones_cannot_turn_local_review_into_global_authority() {
    use crate::cold_effect_reconciliation::review::{
        record_original_owner_effect_review, ColdEffectReviewIntent,
    };
    let (root, archive, first_cold) = aw_p6_10w7_real_cold_fixture();
    let second_parent = root.child("second-cold-clone-parent");
    let second_cold = second_parent.join("aw-restored-w8-second-independent-copy");
    restore_complete_cold_quarantined(&archive, &second_cold, |_, _| Ok(())).unwrap();
    let first = aw_p6_10w8_pending_queue_plan(&archive, &first_cold);
    let second = aw_p6_10w8_pending_queue_plan(&archive, &second_cold);
    assert_eq!(
        first.original_evidence_sha256, second.original_evidence_sha256,
        "two clones can carry identical ORIGINAL evidence but never current authority"
    );
    assert_eq!(first.store_id, second.store_id);
    let original_owner = PrincipalId::new("local").unwrap();
    for (index, cold, plan) in [(1, &first_cold, &first), (2, &second_cold, &second)] {
        let review_id = format!("w8-clone-{index}-original-review");
        let review = record_original_owner_effect_review(
            &archive,
            cold,
            &original_owner,
            &review_id,
            plan,
            ColdEffectReviewIntent::KeepQuarantined,
        )
        .unwrap();
        assert!(
            !review.original_effect_settled && !review.fresh_authority_granted && !review.runnable
        );
        assert!(SessionStore::open(cold).is_err());
        let inspection =
            crate::cold_quarantine_review::inspect_quarantined_cold_restore(&archive, cold)
                .unwrap();
        assert_eq!(inspection.queued_pending, 1);
        assert!(!inspection.activation_authorized && inspection.outstanding_review);
    }
    // Local SQL receipts are independent in two cloned private Stores;
    // this is why a future runnable transition NEEDS a separate global
    // one-time lease/nonce coordinator and independent live human origin.
    assert!(SessionStore::open(&first_cold).is_err());
    assert!(SessionStore::open(&second_cold).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_10w8_two_concurrent_owner_review_requests_commit_at_most_one() {
    use crate::cold_effect_reconciliation::review::{
        record_original_owner_effect_review, ColdEffectReviewIntent,
    };
    let (_root, archive, cold) = aw_p6_10w7_real_cold_fixture();
    let plan = aw_p6_10w8_pending_queue_plan(&archive, &cold);
    let a = archive.clone();
    let b = archive.clone();
    let x = cold.clone();
    let y = cold.clone();
    let first_plan = plan.clone();
    let second_plan = plan.clone();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let left = std::sync::Arc::clone(&barrier);
    let right = std::sync::Arc::clone(&barrier);
    let left = std::thread::spawn(move || {
        left.wait();
        record_original_owner_effect_review(
            &a,
            &x,
            &PrincipalId::new("local").unwrap(),
            "w8-concurrent-a",
            &first_plan,
            ColdEffectReviewIntent::KeepQuarantined,
        )
        .is_ok()
    });
    let right = std::thread::spawn(move || {
        right.wait();
        record_original_owner_effect_review(
            &b,
            &y,
            &PrincipalId::new("local").unwrap(),
            "w8-concurrent-b",
            &second_plan,
            ColdEffectReviewIntent::RequestFreshProposalReview,
        )
        .is_ok()
    });
    let committed = usize::from(left.join().unwrap()) + usize::from(right.join().unwrap());
    assert_eq!(
        committed, 1,
        "exact original review and its BEGIN IMMEDIATE CAS must serialize two owners"
    );
    let c = Connection::open_with_flags(
        cold.join("sessions.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let row_count: i64 = c.query_row(
        "SELECT count(*) FROM local_coordination_events WHERE kind='store.cold_effect_original_owner_review_requested'",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(row_count, 1);
    assert!(SessionStore::open(&cold).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_10w8_real_process_crash_before_and_after_immutable_review_commit() {
    use crate::cold_effect_reconciliation::review::{
        record_original_owner_effect_review, record_original_owner_effect_review_with_crash_seams,
        ColdEffectReviewIntent,
    };
    const ORIGINAL_ARCHIVE: &str = "AW_P6_10W8_CHILD_ARCHIVE";
    const RESTORED: &str = "AW_P6_10W8_CHILD_RESTORED";
    const MODE: &str = "AW_P6_10W8_CHILD_MODE";
    if let Some(archive) = std::env::var_os(ORIGINAL_ARCHIVE) {
        let restored = PathBuf::from(std::env::var_os(RESTORED).unwrap());
        let mode = std::env::var(MODE).unwrap();
        let plan = aw_p6_10w8_pending_queue_plan(Path::new(&archive), &restored);
        let owner = PrincipalId::new("local").unwrap();
        let _ = record_original_owner_effect_review_with_crash_seams(
            Path::new(&archive),
            &restored,
            &owner,
            "w8-process-crash-review",
            &plan,
            ColdEffectReviewIntent::KeepQuarantined,
            |_| {
                if mode == "before" {
                    std::process::exit(91);
                }
                Ok(())
            },
            |_| {
                if mode == "after" {
                    std::process::exit(92);
                }
                Ok(())
            },
        );
        panic!("W8 crash child did not reach requested physical process exit boundary");
    }
    for (mode, expected_exit, expected_events) in [("before", 91, 0), ("after", 92, 1)] {
        let (_root, archive, cold) = aw_p6_10w7_real_cold_fixture();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("composite_staging::tests::aw_p6_10w8_real_process_crash_before_and_after_immutable_review_commit")
            .arg("--nocapture")
            .env(ORIGINAL_ARCHIVE, &archive).env(RESTORED, &cold).env(MODE, mode)
            .output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(expected_exit),
            "W8 process exit {mode} stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let db = Connection::open_with_flags(
            cold.join("sessions.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let rows: i64 = db.query_row(
            "SELECT count(*) FROM local_coordination_events WHERE kind='store.cold_effect_original_owner_review_requested'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(
            rows, expected_events,
            "physical process exit cannot create half a receipt"
        );
        drop(db);
        let inspected = crate::cold_effect_reconciliation::prepare_consistent_cold_effect_snapshot(
            &archive, &cold,
        )
        .unwrap();
        assert_eq!(inspected.actor.len(), 1);
        assert!(inspected.preparation.obligations.iter().any(|o| o.kind
            == crate::cold_effect_reconciliation::EvidenceClass::QueuePending
            && o.count == 1));
        assert!(!inspected.preparation.runnable);
        let owner = PrincipalId::new("local").unwrap();
        let plan = aw_p6_10w8_pending_queue_plan(&archive, &cold);
        if mode == "after" {
            assert!(
                record_original_owner_effect_review(
                    &archive,
                    &cold,
                    &owner,
                    "w8-process-crash-review",
                    &plan,
                    ColdEffectReviewIntent::KeepQuarantined,
                )
                .is_err(),
                "post-commit crash must never append duplicate receipt"
            );
            assert!(
                record_original_owner_effect_review(
                    &archive,
                    &cold,
                    &owner,
                    "w8-fresh-request-same-original",
                    &plan,
                    ColdEffectReviewIntent::RequestFreshProposalReview,
                )
                .is_err(),
                "new ID cannot silently reopen reviewed original effect"
            );
        } else {
            // An interrupted PRE-COMMIT request never created an audit
            // or a runnable task. A new deliberate request can be made.
            assert_eq!(rows, 0);
        }
        assert!(SessionStore::open(&cold).is_err());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_10w8_wrong_owner_and_forged_or_missing_effect_fail_closed() {
    use crate::cold_effect_reconciliation::review::{
        prepare_exact_original_effect_review, record_original_owner_effect_review,
        ColdEffectReviewIntent, ColdEffectSubject,
    };
    let (_root, archive, cold) = aw_p6_10w7_real_cold_fixture();
    let plan = aw_p6_10w8_pending_queue_plan(&archive, &cold);
    let stranger = PrincipalId::new("unrelated-reviewer").unwrap();
    assert!(record_original_owner_effect_review(
        &archive,
        &cold,
        &stranger,
        "w8-stranger",
        &plan,
        ColdEffectReviewIntent::KeepQuarantined,
    )
    .is_err());
    let owner = PrincipalId::new("local").unwrap();
    let mut forgery = plan.clone();
    forgery.original_evidence_sha256 = "f".repeat(64);
    assert!(record_original_owner_effect_review(
        &archive,
        &cold,
        &owner,
        "w8-forgery",
        &forgery,
        ColdEffectReviewIntent::RequestFreshProposalReview,
    )
    .is_err());
    assert!(prepare_exact_original_effect_review(
        &archive,
        &cold,
        &ColdEffectSubject::Actor {
            kind: crate::cold_effect_reconciliation::EvidenceClass::QueueClaimed,
            session_id: match &plan.subject {
                ColdEffectSubject::Actor { session_id, .. } => session_id.clone(),
                _ => unreachable!(),
            },
            request_id: "missing_original_claimed_request".into(),
        }
    )
    .is_err());
    let db = Connection::open_with_flags(
        cold.join("sessions.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let events: i64 = db.query_row(
        "SELECT count(*) FROM local_coordination_events WHERE kind='store.cold_effect_original_owner_review_requested'",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(events, 0);
    assert!(SessionStore::open(&cold).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_10w8_stale_writer_and_removed_quarantine_fail_before_audit() {
    use crate::cold_effect_reconciliation::review::{
        record_original_owner_effect_review_with_hook, ColdEffectReviewIntent,
    };
    let (_root, archive, cold) = aw_p6_10w7_real_cold_fixture();
    let plan = aw_p6_10w8_pending_queue_plan(&archive, &cold);
    let owner = PrincipalId::new("local").unwrap();
    assert!(
        record_original_owner_effect_review_with_hook(
            &archive,
            &cold,
            &owner,
            "w8-active-original-writer",
            &plan,
            ColdEffectReviewIntent::KeepQuarantined,
            |root| {
                let db = Connection::open(root.join("sessions.sqlite3")).unwrap();
                db.execute("UPDATE workspace_writer_state SET state='active'", [])
                    .unwrap();
                Ok(())
            }
        )
        .is_err(),
        "original stale writer epoch must never be reactivated"
    );
    let db = Connection::open(cold.join("sessions.sqlite3")).unwrap();
    db.execute("UPDATE workspace_writer_state SET state='read_only'", [])
        .unwrap();
    drop(db);
    let original_marker = cold.join(COLD_QUARANTINE_MARKER);
    assert!(record_original_owner_effect_review_with_hook(
        &archive,
        &cold,
        &owner,
        "w8-missing-denial-marker",
        &plan,
        ColdEffectReviewIntent::KeepQuarantined,
        |_| {
            fs::remove_file(&original_marker).unwrap();
            Ok(())
        }
    )
    .is_err());
    let db = Connection::open_with_flags(
        cold.join("sessions.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let events: i64 = db.query_row(
        "SELECT count(*) FROM local_coordination_events WHERE kind='store.cold_effect_original_owner_review_requested'",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(events, 0);
    assert!(SessionStore::open(&cold).is_err());
}

#[cfg(target_os = "linux")]
fn aw_p6_w8i_fixture(operation: &str) -> VFixture {
    let fixture = aw_p6_10v_fixture_with_consent(operation != "consent");
    if operation == "challenge" {
        crate::cold_quarantine_review::record_quarantine_review_request(
            &fixture.archive,
            &fixture.restored,
            &PrincipalId::new("local").unwrap(),
            "w8i-audit",
            crate::cold_quarantine_review::ColdReviewIntent::RequestSeparateActivationReview,
        )
        .unwrap();
    }
    fixture
}

#[cfg(target_os = "linux")]
fn aw_p6_w8i_event(operation: &str) -> (&'static str, &'static str) {
    match operation {
        "challenge" => ("store.cold_activation_device_challenge_issued", "w8i-audit"),
        "consent" => (
            "store.cold_activation_device_consent_verified",
            "v-fully-signed",
        ),
        _ => ("store.cold_activation_dry_run_recorded", "w8i-dry"),
    }
}

#[cfg(target_os = "linux")]
fn aw_p6_w8i_audit<F, G>(
    fixture: &VFixture,
    operation: &str,
    hooks: (F, G),
) -> aw_agent_core::Result<()>
where
    F: FnOnce(&Path) -> aw_agent_core::Result<()>,
    G: FnOnce(&Path) -> aw_agent_core::Result<()>,
{
    let VFixture {
        archive,
        restored,
        challenge,
        public,
        signature,
        ..
    } = fixture;
    match operation {
        "challenge" => {
            let claim: serde_json::Value = serde_json::from_str(&challenge.transcript).unwrap();
            crate::cold_activation_consent::issue_cold_activation_device_challenge_with_hooks(
                archive,
                restored,
                claim["domain_id"].as_str().unwrap(),
                "w8i-audit",
                hooks,
            )
            .map(|_| ())
        }
        "consent" => {
            crate::cold_activation_consent::record_signed_cold_activation_consent_with_hooks(
                archive, restored, challenge, public, signature, hooks,
            )
        }
        _ => crate::cold_activation_dry_run::record_cold_activation_dry_run_with_hooks(
            archive, restored, challenge, public, signature, "w8i-dry", hooks,
        )
        .map(|_| ()),
    }
}

#[cfg(target_os = "linux")]
fn aw_p6_w8i_audit_count(path: &Path, operation: &str) -> i64 {
    let (kind, key) = aw_p6_w8i_event(operation);
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    c.query_row(
        "SELECT count(*) FROM local_coordination_events WHERE kind=?1 AND scope_key=?2",
        rusqlite::params![kind, key],
        |row| row.get(0),
    )
    .unwrap()
}

#[cfg(target_os = "linux")]
fn aw_p6_w8i_precommit_faults(operation: &str) {
    for fault in ["new-run", "generation", "device", "quarantine"] {
        let fixture = aw_p6_w8i_fixture(operation);
        let result = aw_p6_w8i_audit(
            &fixture,
            operation,
            (
                |root: &Path| {
                    if fault == "quarantine" {
                        fs::remove_file(root.join(COLD_QUARANTINE_MARKER))?;
                    } else {
                        let c = crate::db(Connection::open(root.join("sessions.sqlite3")))?;
                        match fault {
                            "new-run" => {
                                crate::db(c.execute("INSERT INTO runs(id,session_id,branch_id,turn_id,prompt_node,identity_json,root_key,status,possible_effect,started_at)
                        SELECT 'w8i-new-run',s.id,b.id,987999,b.head,'{}','w8i-new-root','running',1,0
                        FROM sessions s JOIN branches b ON b.session_id=s.id LIMIT 1", []))?;
                            }
                            "generation" => {
                                crate::db(c.execute("UPDATE store_meta SET value='999' WHERE key='restore_generation'", []))?;
                            }
                            _ => {
                                crate::db(c.execute("UPDATE portable_identity_bindings SET status='revoked' WHERE binding_id='pib_aw_p6_10u'", []))?;
                            }
                        }
                    }
                    Ok(())
                },
                |_| Ok(()),
            ),
        );
        assert!(
            result.is_err(),
            "{operation}/{fault}: stale U/V evidence must not publish an audit"
        );
        assert_eq!(
            aw_p6_w8i_audit_count(&fixture.restored.join("sessions.sqlite3"), operation),
            0
        );
        assert!(SessionStore::open(&fixture.restored).is_err());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_w8i_u_challenge_precommit_faults_publish_no_audit() {
    aw_p6_w8i_precommit_faults("challenge");
}
#[cfg(target_os = "linux")]
#[test]
fn aw_p6_w8i_u_consent_precommit_faults_publish_no_audit() {
    aw_p6_w8i_precommit_faults("consent");
}
#[cfg(target_os = "linux")]
#[test]
fn aw_p6_w8i_v_dry_run_precommit_faults_publish_no_audit() {
    aw_p6_w8i_precommit_faults("dry-run");
}

#[cfg(target_os = "linux")]
fn aw_p6_w8i_postcommit_faults(operation: &str) {
    for fault in [
        "callback",
        "quarantine",
        "old-path",
        "old-in-place",
        "committed-copy",
    ] {
        let fixture = aw_p6_w8i_fixture(operation);
        let saved = fixture._root.child("w8i-preserved-original");
        let before = saved.join("before.sqlite3");
        let committed = saved.join("committed.sqlite3");
        let result = aw_p6_w8i_audit(
            &fixture,
            operation,
            (
                |root: &Path| {
                    fs::copy(root.join("sessions.sqlite3"), &before)?;
                    Ok(())
                },
                |root: &Path| {
                    let sqlite = root.join("sessions.sqlite3");
                    if matches!(fault, "old-path" | "committed-copy") {
                        fs::rename(&sqlite, &committed)?;
                        fs::copy(
                            if fault == "committed-copy" {
                                &committed
                            } else {
                                &before
                            },
                            &sqlite,
                        )?;
                    } else {
                        fs::copy(&sqlite, &committed)?;
                        match fault {
                            "callback" => {
                                return Err(aw_agent_core::Error::Denied(
                                    "synthetic U/V postcommit acknowledgment fault",
                                ))
                            }
                            "quarantine" => fs::remove_file(root.join(COLD_QUARANTINE_MARKER))?,
                            _ => fs::write(&sqlite, fs::read(&before)?)?,
                        }
                    }
                    Ok(())
                },
            ),
        );
        let error = result.unwrap_err();
        assert!(
            matches!(&error, aw_agent_core::Error::Storage(message)
            if message.contains("outcome unknown") && message.contains("inspect")),
            "{operation}/{fault}: durable audit must not look rolled back: {error:?}"
        );
        assert_eq!(aw_p6_w8i_audit_count(&committed, operation), 1);
        let expected = i64::from(!matches!(fault, "old-path" | "old-in-place"));
        assert_eq!(
            aw_p6_w8i_audit_count(&fixture.restored.join("sessions.sqlite3"), operation),
            expected
        );
        assert!(SessionStore::open(&fixture.restored).is_err());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_w8i_u_challenge_postcommit_outcomes_are_unknown() {
    aw_p6_w8i_postcommit_faults("challenge");
}
#[cfg(target_os = "linux")]
#[test]
fn aw_p6_w8i_u_consent_postcommit_outcomes_are_unknown() {
    aw_p6_w8i_postcommit_faults("consent");
}
#[cfg(target_os = "linux")]
#[test]
fn aw_p6_w8i_v_dry_run_postcommit_outcomes_are_unknown() {
    aw_p6_w8i_postcommit_faults("dry-run");
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_w8i_u_v_reads_reject_lost_original_file_identity() {
    for reader in ["consent", "dry-run"] {
        for fault in ["quarantine", "sqlite"] {
            let fixture = aw_p6_10v_fixture();
            let saved = fixture._root.child("w8i-preserved-read");
            let hook = |root: &Path| {
                if fault == "quarantine" {
                    fs::remove_file(root.join(COLD_QUARANTINE_MARKER))?;
                } else {
                    let sqlite = root.join("sessions.sqlite3");
                    let original = saved.join("original.sqlite3");
                    fs::rename(&sqlite, &original)?;
                    fs::copy(&original, &sqlite)?;
                }
                Ok(())
            };
            let denied = if reader == "consent" {
                crate::cold_activation_consent::reverify_signed_cold_consent_evidence_with_hook(
                    &fixture.archive,
                    &fixture.restored,
                    &fixture.challenge,
                    &fixture.public,
                    &fixture.signature,
                    hook,
                )
                .map(|_| ())
            } else {
                crate::cold_activation_dry_run::inspect_cold_activation_dry_run_with_hook(
                    &fixture.archive,
                    &fixture.restored,
                    &fixture.challenge,
                    &fixture.public,
                    &fixture.signature,
                    hook,
                )
                .map(|_| ())
            };
            assert!(
                denied.is_err(),
                "{reader}/{fault}: U/V must retain original verified file identities until return"
            );
            assert!(SessionStore::open(&fixture.restored).is_err());
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn aw_p6_w8i_wrong_envelope_owner_review_cannot_issue_challenge() {
    for fault in ["store", "scope", "lease"] {
        let fixture = aw_p6_10v_fixture();
        let claim: serde_json::Value = serde_json::from_str(&fixture.challenge.transcript).unwrap();
        let sqlite = fixture.restored.join("sessions.sqlite3");
        let c = Connection::open(&sqlite).unwrap();
        c.execute("INSERT INTO local_coordination_events(event_id,store_id,store_generation,process_id,kind,scope_kind,scope_key,lease_generation,payload_json,created_at)
            SELECT ?1,CASE WHEN ?2='store' THEN 'different-original-store' ELSE store_id END,store_generation,NULL,kind,
            CASE WHEN ?2='scope' THEN 'workspace' ELSE scope_kind END,'w8i-invalid-review',
            CASE WHEN ?2='lease' THEN 7 ELSE NULL END,json_set(payload_json,'$.request_id','w8i-invalid-review'),created_at
            FROM local_coordination_events WHERE kind='store.cold_quarantine_review_requested' AND scope_key='v-fully-signed'",
            rusqlite::params![crate::id("coord").unwrap(), fault]).unwrap();
        drop(c);
        assert!(crate::cold_activation_consent::issue_cold_activation_device_challenge(
            &fixture.archive, &fixture.restored, claim["domain_id"].as_str().unwrap(), "w8i-invalid-review",
        ).is_err(), "{fault}: copied payload with a wrong original event envelope is not a review prerequisite");
        assert!(SessionStore::open(&fixture.restored).is_err());
    }
}
