use super::*;
use aw_agent_core::publication;
use std::{collections::BTreeMap, hint::black_box, time::Instant};

fn snapshot(live_bytes: usize, sessions: usize) -> Snapshot {
    Snapshot {
        generation: 0,
        busy: true,
        state_pending: false,
        fatal: false,
        store_id: Some("invented-store".into()),
        session: Some("invented-session".into()),
        branch_id: Some("invented-branch".into()),
        session_title: Some("Fixture".into()),
        branch: "main".into(),
        model: "fixture".into(),
        profile: "fixture".into(),
        provider: "fixture".into(),
        effort: "normal".into(),
        actor: "fixture".into(),
        root: "/invented".into(),
        agent_root: Some("/invented".into()),
        tools: "none".into(),
        needs_inspection: false,
        turns: Arc::new(Vec::new()),
        pending_prompt: "Invented benchmark input".into(),
        live: "x".repeat(live_bytes),
        thinking: false,
        turn_started: None,
        live_tools: Vec::new(),
        rejected_prompt: String::new(),
        rejected_revision: 0,
        activity: (0..32).map(|n| format!("Invented activity {n}")).collect(),
        event_rail: None,
        sessions: (0..sessions)
            .map(|n| SavedSession {
                id: format!("invented-{n}"),
                title: format!("Invented session {n}"),
                directory: "/invented/workspace".into(),
                autonomous: false,
                archived: false,
                updated_at: 0,
                turn_count: 0,
                branches: vec![SavedBranch {
                    id: format!("invented-branch-{n}"),
                    name: "main".into(),
                    parent: None,
                }],
            })
            .collect(),
        subagents: Vec::new(),
        delegation_budget: None,
        tree: String::new(),
        context_output: String::new(),
        context_blocks: Vec::new(),
        loaded_resources: Default::default(),
        status: "Streaming fixture".into(),
        auxiliary_output: None,
        performance: None,
        session_tps: None,
    }
}

fn advance(snapshot: &mut Snapshot, live_bytes: usize) {
    // Bounded streaming windows keep both alternatives on the same dataset.
    if snapshot.live.len() >= live_bytes + 16 * 1024 {
        snapshot.live.truncate(live_bytes);
    }
    snapshot.live.push_str(" streamed delta ");
    snapshot.generation += 1;
}

fn median(mut samples: Vec<u128>) -> u128 {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

/// Run explicitly on the same host/profile; this emits measurements, not
/// timing assertions or a claim about model, terminal, or database latency.
#[test]
#[ignore = "repeatable publication design benchmark; use --ignored --nocapture"]
fn native_projection_publication_workloads() {
    const BATCHES: usize = 1_000;
    for live_bytes in [32 * 1024, 256 * 1024] {
        for sessions in [10, 100] {
            for (writes, reads) in [(1, 4), (4, 1)] {
                let mut locked_samples = Vec::new();
                let mut published_samples = Vec::new();
                for _ in 0..5 {
                    let initial = snapshot(live_bytes, sessions);
                    let locked = Mutex::new(initial.clone());
                    let start = Instant::now();
                    for _ in 0..BATCHES {
                        for _ in 0..writes {
                            advance(&mut locked.lock().unwrap(), live_bytes);
                        }
                        for _ in 0..reads {
                            black_box(locked.lock().unwrap().clone());
                        }
                    }
                    locked_samples.push(start.elapsed().as_nanos());

                    let (publisher, factory) = publication::channel(initial);
                    let reader = factory.reader().unwrap();
                    let start = Instant::now();
                    for _ in 0..BATCHES {
                        for _ in 0..writes {
                            publisher
                                .update(|old| {
                                    let mut next = old.clone();
                                    advance(&mut next, live_bytes);
                                    Ok((next, ()))
                                })
                                .unwrap();
                        }
                        for _ in 0..reads {
                            black_box(reader.snapshot().unwrap());
                        }
                    }
                    published_samples.push(start.elapsed().as_nanos());
                }
                let locked = median(locked_samples);
                let published = median(published_samples);
                println!(
                    "{}",
                    serde_json::json!({
                        "workload":"actual_native_snapshot", "debug_assertions":cfg!(debug_assertions),
                        "live_bytes":live_bytes, "sessions":sessions, "writes":writes, "reads":reads,
                        "batches":BATCHES, "median_locked_ns":locked,
                        "median_published_ns":published, "published_over_locked":published as f64 / locked as f64,
                    })
                );
            }
        }
    }

    // Matching lookup shapes without spawning PTYs/processes: the pointer
    // payload is irrelevant to lookup, and resource/authority I/O is excluded.
    for entries in [4_usize, 16] {
        let initial = (0..entries)
            .map(|n| (format!("handle-{n:02}"), Arc::new(n)))
            .collect::<BTreeMap<_, _>>();
        let locked = Mutex::new(initial.clone());
        let (publisher, factory) = publication::channel(initial);
        let reader = factory.reader().unwrap();
        let tool_reader = Mutex::new(factory.reader().unwrap());
        let mut measurements = BTreeMap::new();
        for shape in ["locked", "owned_reader", "tool_reader_mutex", "cold_reader"] {
            let mut samples = Vec::new();
            for _ in 0..5 {
                let start = Instant::now();
                for _ in 0..100_000 {
                    let found = match shape {
                        "locked" => locked.lock().unwrap().get("handle-02").cloned(),
                        "owned_reader" => {
                            reader.snapshot().unwrap().value.get("handle-02").cloned()
                        }
                        "tool_reader_mutex" => tool_reader
                            .lock()
                            .unwrap()
                            .snapshot()
                            .unwrap()
                            .value
                            .get("handle-02")
                            .cloned(),
                        _ => factory
                            .reader()
                            .unwrap()
                            .snapshot()
                            .unwrap()
                            .value
                            .get("handle-02")
                            .cloned(),
                    };
                    black_box(found);
                }
                samples.push(start.elapsed().as_nanos());
            }
            measurements.insert(shape, median(samples) as f64 / 100_000.0);
        }
        println!(
            "{}",
            serde_json::json!({
                "workload":"bounded_resource_lookup", "entries":entries,
                "debug_assertions":cfg!(debug_assertions), "median_ns_per_lookup":measurements,
                "publication_generation":publisher.current().unwrap().generation,
            })
        );
    }
}
