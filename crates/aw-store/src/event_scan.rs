//! One private ordered DL-200 Event scan used by every Event consumer.
//!
//! The scanner owns only ordered traversal, projection/store/placement fencing
//! and the source-first authorization sequence. It never commits cursor progress,
//! EventInputs, Handler acknowledgments or grants.

use crate::{
    db,
    event_projection::{self, EventProjectionSnapshot},
    store_scope::{StorePos, StoreScope},
    Error, Result,
};
use aw_tools::event_subject::EventPattern;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest as _, Sha256};

pub(crate) const MAX_EVENT_SCAN_ROWS: usize = 4096;

pub(crate) fn selector_hash(subject: &str) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"aw-event-selector-v1\0");
    hash.update((subject.len() as u64).to_be_bytes());
    hash.update(subject.as_bytes());
    hash.finalize().into()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EventAuthorization<Evidence> {
    Allow(Evidence),
    Deny,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventPlacement {
    Current,
    Stale,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventSourceKind {
    Domain,
    Principal,
    Project,
    Sandbox,
    Agent,
    Turn,
    Job,
    Node,
}

impl EventSourceKind {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "domain" => Ok(Self::Domain),
            "principal" => Ok(Self::Principal),
            "project" => Ok(Self::Project),
            "sandbox" => Ok(Self::Sandbox),
            "agent" => Ok(Self::Agent),
            "turn" => Ok(Self::Turn),
            "job" => Ok(Self::Job),
            "node" => Ok(Self::Node),
            _ => Err(Error::Storage("invalid Event projection source kind")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EventSource {
    pub(crate) reference: String,
    pub(crate) kind: EventSourceKind,
    pub(crate) domain: String,
    pub(crate) identity: Option<String>,
    pub(crate) project: Option<String>,
    pub(crate) sandbox: Option<String>,
    pub(crate) agent: Option<String>,
    pub(crate) turn: Option<String>,
    pub(crate) job: Option<String>,
    pub(crate) node: Option<u32>,
}

#[derive(Clone, Debug)]
pub(crate) struct EventCandidate {
    pub(crate) position: StorePos,
    pub(crate) event_id: String,
    pub(crate) subject: String,
    pub(crate) source: EventSource,
    pub(crate) event_rev: u64,
    pub(crate) actor: Option<String>,
    pub(crate) payload: serde_json::Value,
}

#[derive(Debug)]
pub(crate) struct EventScanCandidate<SourceEvidence, CandidateEvidence> {
    pub(crate) event: EventCandidate,
    pub(crate) source_evidence: SourceEvidence,
    pub(crate) candidate_evidence: CandidateEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EventScanFence {
    scope: StoreScope,
    projection_generation: u64,
}

impl EventScanFence {
    pub(crate) fn capture(connection: &Connection) -> Result<Self> {
        let projection = event_projection::snapshot(connection)?;
        if !projection.usable {
            return Err(Error::Storage(
                "Event projection is unavailable; owner normalization required",
            ));
        }
        Ok(Self {
            scope: projection.scope,
            projection_generation: projection.generation,
        })
    }
}

#[derive(Debug)]
pub(crate) enum EventScanResult<SourceEvidence, CandidateEvidence> {
    Candidate {
        candidate: Box<EventScanCandidate<SourceEvidence, CandidateEvidence>>,
        examined: usize,
    },
    ProvenSkip {
        through: StorePos,
        examined: usize,
    },
    BlockedUnknown,
    Stale,
    End {
        through: StorePos,
    },
}

struct IndexedEvent {
    position: StorePos,
    event_id: String,
    subject: String,
    source: EventSource,
    event_rev: u64,
}

enum FenceState {
    Current(EventProjectionSnapshot),
    Blocked,
    Stale,
}

/// Scan one bounded ordered Event-index window.
///
/// source_visibility is always evaluated before candidate_authority. A proven
/// nonmatch or denial may advance privately. Any unknown decision holds the
/// caller at its original progress. The placement callback is checked both
/// before scanning and before returning any candidate/progress.
pub(crate) fn scan_events<
    SourceEvidence,
    CandidateEvidence,
    Placement,
    SourceVisibility,
    CandidateAuthority,
>(
    connection: &Connection,
    fence: &EventScanFence,
    after: StorePos,
    pattern: &EventPattern,
    scan_limit: usize,
    mut placement: Placement,
    mut source_visibility: SourceVisibility,
    mut candidate_authority: CandidateAuthority,
) -> Result<EventScanResult<SourceEvidence, CandidateEvidence>>
where
    Placement: FnMut() -> Result<EventPlacement>,
    SourceVisibility: FnMut(&EventSource) -> Result<EventAuthorization<SourceEvidence>>,
    CandidateAuthority:
        FnMut(&EventCandidate, &SourceEvidence) -> Result<EventAuthorization<CandidateEvidence>>,
{
    if scan_limit == 0 || scan_limit > MAX_EVENT_SCAN_ROWS {
        return Err(Error::Limit(
            "Event scan must examine 1..4096 projected rows",
        ));
    }

    let projection = match check_fence(connection, fence, &mut placement)? {
        FenceState::Current(projection) => projection,
        FenceState::Blocked => return Ok(EventScanResult::BlockedUnknown),
        FenceState::Stale => return Ok(EventScanResult::Stale),
    };
    if after > projection.indexed_through {
        return Ok(EventScanResult::Stale);
    }

    let mut query = db(connection.prepare(
        "SELECT
             commit_position,event_id,subject,source_ref,source_kind,
             domain_handle,identity_handle,project_handle,sandbox_handle,agent_handle,
             turn_handle,job_handle,node_coordinate,event_rev
         FROM event_projection_index
         WHERE commit_position>?1 AND commit_position<=?2
         ORDER BY commit_position
         LIMIT ?3",
    ))?;
    let mut rows = db(query.query(params![
        after.get(),
        projection.indexed_through.get(),
        scan_limit as i64
    ]))?;

    let mut examined = 0usize;
    let mut last_proven = after;
    while let Some(row) = db(rows.next())? {
        examined += 1;
        let indexed = IndexedEvent {
            position: StorePos::new(
                row.get(0)
                    .map_err(|_| Error::Storage("invalid Event projection commit position"))?,
            ),
            event_id: row
                .get(1)
                .map_err(|_| Error::Storage("invalid Event projection event id"))?,
            subject: row
                .get(2)
                .map_err(|_| Error::Storage("invalid Event projection subject"))?,
            source: EventSource {
                reference: row
                    .get(3)
                    .map_err(|_| Error::Storage("invalid Event projection source ref"))?,
                kind: EventSourceKind::parse(
                    row.get_ref(4)
                        .map_err(|_| Error::Storage("invalid Event projection source kind"))?
                        .as_str()
                        .map_err(|_| Error::Storage("invalid Event projection source kind"))?,
                )?,
                domain: row
                    .get(5)
                    .map_err(|_| Error::Storage("invalid Event projection Domain"))?,
                identity: row
                    .get(6)
                    .map_err(|_| Error::Storage("invalid Event projection Principal"))?,
                project: row
                    .get(7)
                    .map_err(|_| Error::Storage("invalid Event projection Project"))?,
                sandbox: row
                    .get(8)
                    .map_err(|_| Error::Storage("invalid Event projection Sandbox"))?,
                agent: row
                    .get(9)
                    .map_err(|_| Error::Storage("invalid Event projection Agent"))?,
                turn: row
                    .get(10)
                    .map_err(|_| Error::Storage("invalid Event projection Turn"))?,
                job: row
                    .get(11)
                    .map_err(|_| Error::Storage("invalid Event projection Job"))?,
                node: row
                    .get(12)
                    .map_err(|_| Error::Storage("invalid Event projection Node"))?,
            },
            event_rev: row
                .get(13)
                .map_err(|_| Error::Storage("invalid Event projection event revision"))?,
        };

        if !pattern.matches_canonical(&indexed.subject) {
            last_proven = indexed.position;
            continue;
        }

        let source_evidence = match source_visibility(&indexed.source)? {
            EventAuthorization::Deny => {
                last_proven = indexed.position;
                continue;
            }
            EventAuthorization::Unknown => return Ok(EventScanResult::BlockedUnknown),
            EventAuthorization::Allow(evidence) => evidence,
        };

        let candidate = load_candidate(connection, indexed)?;
        let candidate_evidence = match candidate_authority(&candidate, &source_evidence)? {
            EventAuthorization::Deny => {
                last_proven = candidate.position;
                continue;
            }
            EventAuthorization::Unknown => return Ok(EventScanResult::BlockedUnknown),
            EventAuthorization::Allow(evidence) => evidence,
        };
        return match check_fence(connection, fence, &mut placement)? {
            FenceState::Current(_) => Ok(EventScanResult::Candidate {
                candidate: Box::new(EventScanCandidate {
                    event: candidate,
                    source_evidence,
                    candidate_evidence,
                }),
                examined,
            }),
            FenceState::Blocked => Ok(EventScanResult::BlockedUnknown),
            FenceState::Stale => Ok(EventScanResult::Stale),
        };
    }
    drop(rows);
    drop(query);

    match check_fence(connection, fence, &mut placement)? {
        FenceState::Blocked => Ok(EventScanResult::BlockedUnknown),
        FenceState::Stale => Ok(EventScanResult::Stale),
        FenceState::Current(_) if examined == scan_limit => Ok(EventScanResult::ProvenSkip {
            through: last_proven,
            examined,
        }),
        FenceState::Current(_) => Ok(EventScanResult::End {
            through: projection.indexed_through,
        }),
    }
}

pub(crate) fn candidate_by_source_revision_name(
    connection: &Connection,
    source_ref: &str,
    event_rev: u64,
    event_name: &str,
) -> Result<Option<EventCandidate>> {
    let mut query = db(connection.prepare(
        "SELECT
             commit_position,event_id,subject,source_ref,source_kind,
             domain_handle,identity_handle,project_handle,sandbox_handle,agent_handle,
             turn_handle,job_handle,node_coordinate,event_rev
         FROM event_projection_index
         WHERE source_ref=?1 AND event_rev=?2 AND event_name=?3
         ORDER BY commit_position LIMIT 2",
    ))?;
    let rows = db(
        query.query_map(params![source_ref, event_rev, event_name], |row| {
            Ok((
                row.get::<_, u64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<u32>>(12)?,
                row.get::<_, u64>(13)?,
            ))
        }),
    )?;
    let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    if rows.len() != 1 {
        return Err(Error::Storage(
            "legacy Event bridge found duplicate projected source revision/name",
        ));
    }
    let indexed = IndexedEvent {
        position: StorePos::new(row.0),
        event_id: row.1.clone(),
        subject: row.2.clone(),
        source: EventSource {
            reference: row.3.clone(),
            kind: EventSourceKind::parse(&row.4)?,
            domain: row.5.clone(),
            identity: row.6.clone(),
            project: row.7.clone(),
            sandbox: row.8.clone(),
            agent: row.9.clone(),
            turn: row.10.clone(),
            job: row.11.clone(),
            node: row.12,
        },
        event_rev: row.13,
    };
    load_candidate(connection, indexed).map(Some)
}

fn check_fence<Placement>(
    connection: &Connection,
    fence: &EventScanFence,
    placement: &mut Placement,
) -> Result<FenceState>
where
    Placement: FnMut() -> Result<EventPlacement>,
{
    let projection = event_projection::snapshot(connection)?;
    if !projection.usable
        || projection.scope != fence.scope
        || projection.generation != fence.projection_generation
    {
        return Ok(FenceState::Stale);
    }
    match placement()? {
        EventPlacement::Unknown => Ok(FenceState::Blocked),
        EventPlacement::Stale => Ok(FenceState::Stale),
        EventPlacement::Current => Ok(FenceState::Current(projection)),
    }
}

fn load_candidate(connection: &Connection, indexed: IndexedEvent) -> Result<EventCandidate> {
    let row: Option<(String, Option<String>, String)> = db(connection
        .query_row(
            "SELECT event_id,actor,payload_json
             FROM commit_log WHERE seq=?1",
            [indexed.position.get()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional())?;
    let Some((event_id, actor, payload)) = row else {
        return Err(Error::Storage(
            "Event projection references a missing immutable commit row",
        ));
    };
    if event_id != indexed.event_id {
        return Err(Error::Storage(
            "Event projection identity disagrees with immutable commit row",
        ));
    }
    let payload = serde_json::from_str(&payload)
        .map_err(|_| Error::Storage("cannot decode immutable Event payload"))?;
    Ok(EventCandidate {
        position: indexed.position,
        event_id: indexed.event_id,
        subject: indexed.subject,
        source: indexed.source,
        event_rev: indexed.event_rev,
        actor,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::durability;
    use aw_tools::event_subject::EventSubject;
    use rusqlite::TransactionBehavior;

    fn emit(
        connection: &mut Connection,
        resource: &str,
        event_name: &str,
        event_rev: u64,
        tag: &str,
    ) -> StorePos {
        let subject = EventSubject::from_resource_ref(resource, event_name).unwrap();
        let txid = durability::tx_id().unwrap();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let position = durability::commit_event_position(
            &tx,
            &txid,
            "fixture.event",
            "fixture",
            tag,
            None,
            serde_json::json!({
                "_aw_dl200_event":{
                    "version":1,
                    "subject":subject.as_str(),
                    "event_rev":event_rev
                },
                "tag":tag
            }),
        )
        .unwrap();
        tx.commit().unwrap();
        position
    }

    #[test]
    fn scan_is_source_first_bounded_and_never_advances_unknown_or_stale_work() {
        let store = crate::SessionStore::in_memory().unwrap();
        let mut connection = store.connection().unwrap();
        let floor = event_projection::snapshot(&connection)
            .unwrap()
            .indexed_through;

        let nonmatch = emit(
            &mut connection,
            "/d:acme/i:alice/p:core",
            "changed",
            1,
            "nonmatch",
        );
        let hidden = emit(
            &mut connection,
            "/d:acme/i:alice/p:core/a:2",
            "changed",
            1,
            "hidden",
        );
        let denied = emit(
            &mut connection,
            "/d:acme/i:alice/p:core/a:3",
            "changed",
            1,
            "denied",
        );
        let blocked = emit(
            &mut connection,
            "/d:acme/i:alice/p:core/a:4",
            "changed",
            1,
            "blocked",
        );
        assert!(nonmatch < hidden && hidden < denied && denied < blocked);

        let fence = EventScanFence::capture(&connection).unwrap();
        let pattern = EventPattern::parse_absolute("d:acme.i:alice.p:core.a:*.>").unwrap();

        let mut source_calls = Vec::new();
        let mut deep_calls = Vec::new();
        let first = scan_events(
            &connection,
            &fence,
            floor,
            &pattern,
            3,
            || Ok(EventPlacement::Current),
            |source| {
                source_calls.push(source.reference.clone());
                Ok(if source.reference.ends_with("/a:2") {
                    EventAuthorization::Deny
                } else {
                    EventAuthorization::Allow(source.reference.clone())
                })
            },
            |candidate, source_evidence| {
                assert_eq!(source_evidence, &candidate.source.reference);
                deep_calls.push(candidate.source.reference.clone());
                Ok(EventAuthorization::<()>::Deny)
            },
        )
        .unwrap();
        let EventScanResult::ProvenSkip { through, examined } = first else {
            panic!("bounded proven skips must return private progress");
        };
        assert_eq!(through, denied);
        assert_eq!(examined, 3);
        assert_eq!(
            source_calls,
            vec!["/d:acme/i:alice/p:core/a:2", "/d:acme/i:alice/p:core/a:3"]
        );
        assert_eq!(deep_calls, vec!["/d:acme/i:alice/p:core/a:3"]);

        let second = scan_events(
            &connection,
            &fence,
            through,
            &pattern,
            4,
            || Ok(EventPlacement::Current),
            |_| Ok(EventAuthorization::Allow(())),
            |_, _| Ok(EventAuthorization::<()>::Unknown),
        )
        .unwrap();
        assert!(matches!(second, EventScanResult::BlockedUnknown));

        let third = scan_events(
            &connection,
            &fence,
            through,
            &pattern,
            4,
            || Ok(EventPlacement::Current),
            |source| Ok(EventAuthorization::Allow(source.reference.clone())),
            |candidate, source_evidence| {
                assert_eq!(source_evidence, &candidate.source.reference);
                Ok(EventAuthorization::Allow("manifest-evidence".to_owned()))
            },
        )
        .unwrap();
        let EventScanResult::Candidate {
            candidate,
            examined,
        } = third
        else {
            panic!("same blocked row must remain retryable from unchanged progress");
        };
        assert_eq!(examined, 1);
        assert_eq!(candidate.source_evidence, "/d:acme/i:alice/p:core/a:4");
        assert_eq!(candidate.candidate_evidence, "manifest-evidence");
        let candidate = &candidate.event;
        assert_eq!(candidate.position, blocked);
        assert_eq!(candidate.source.reference, "/d:acme/i:alice/p:core/a:4");
        assert_eq!(candidate.subject, "d:acme.i:alice.p:core.a:4.e:changed");
        assert!(!candidate.event_id.is_empty());
        assert_eq!(candidate.event_rev, 1);
        assert_eq!(candidate.actor, None);
        assert_eq!(candidate.payload["tag"], "blocked");

        let placement_unknown = scan_events(
            &connection,
            &fence,
            candidate.position,
            &pattern,
            4,
            || Ok(EventPlacement::Unknown),
            |_| -> Result<EventAuthorization<()>> {
                panic!("placement must gate source authorization")
            },
            |_, _| -> Result<EventAuthorization<()>> {
                panic!("placement must gate candidate authorization")
            },
        )
        .unwrap();
        assert!(matches!(placement_unknown, EventScanResult::BlockedUnknown));

        let no_match = EventPattern::parse_absolute("d:acme.i:alice.p:other.>").unwrap();
        let no_match_end = scan_events(
            &connection,
            &fence,
            floor,
            &no_match,
            MAX_EVENT_SCAN_ROWS,
            || Ok(EventPlacement::Current),
            |_| -> Result<EventAuthorization<()>> {
                panic!("nonmatch must skip before source authorization")
            },
            |_, _| -> Result<EventAuthorization<()>> {
                panic!("nonmatch must skip before candidate authorization")
            },
        )
        .unwrap();
        let EventScanResult::End { through: end } = no_match_end else {
            panic!("fully scanned nonmatches must end at the safe projection watermark");
        };
        assert_eq!(
            end,
            event_projection::snapshot(&connection)
                .unwrap()
                .indexed_through
        );

        let later = emit(
            &mut connection,
            "/d:acme/i:alice/p:core/a:5",
            "changed",
            1,
            "later",
        );
        let mut placement_checks = 0u8;
        let stale = scan_events(
            &connection,
            &fence,
            candidate.position,
            &pattern,
            4,
            || {
                placement_checks += 1;
                Ok(if placement_checks == 1 {
                    EventPlacement::Current
                } else {
                    EventPlacement::Stale
                })
            },
            |_| Ok(EventAuthorization::Allow(())),
            |_, _| Ok(EventAuthorization::Allow(())),
        )
        .unwrap();
        assert!(matches!(stale, EventScanResult::Stale));
        assert!(later > candidate.position);

        connection
            .execute(
                "DELETE FROM event_projection_index WHERE commit_position=?1",
                [later.get()],
            )
            .unwrap();
        let dirty = scan_events(
            &connection,
            &fence,
            candidate.position,
            &pattern,
            4,
            || Ok(EventPlacement::Current),
            |_| Ok(EventAuthorization::Allow(())),
            |_, _| Ok(EventAuthorization::Allow(())),
        )
        .unwrap();
        assert!(matches!(dirty, EventScanResult::Stale));
    }
}
