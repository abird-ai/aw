// Explicit real TCP/HTTP benchmark of authenticated presence with an actual
// SQLite Store and the shipping handler. Synthetic cache rows are inserted
// by a cfg(test)-only helper; no user/provider/Tool effects are generated.
use std::{sync::Arc as Shared, time::Instant as Clock};

fn measurement_percentiles(mut samples: Vec<u128>) -> [u128; 2] {
    if samples.is_empty() {
        return [0, 0];
    }
    samples.sort_unstable();
    [50, 95].map(|p| samples[(samples.len() - 1) * p / 100])
}

fn cpu_ticks() -> Option<u64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let (_, value) = stat.rsplit_once(") ")?;
    let fields = value.split_whitespace().collect::<Vec<_>>();
    Some(fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?)
}

async fn post_json(
    client: &reqwest::Client,
    base: &str,
    route: &str,
    cookie: Option<&str>,
    value: &serde_json::Value,
) -> reqwest::Response {
    let mut request = client.post(format!("{base}{route}"))
        .header(reqwest::header::ORIGIN, base)
        .json(value);
    if let Some(cookie) = cookie {
        request = request.header(reqwest::header::COOKIE, cookie);
    }
    request.send().await.unwrap()
}

async fn authorized_presence_read(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    body: &serde_json::Value,
    expected_rows: usize,
) -> u128 {
    let start = Clock::now();
    let response = post_json(client, base, "/api/team/draft/presence/list", Some(cookie), body)
        .await;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.unwrap();
    let rows = body["presence"].as_array().unwrap();
    assert_eq!(rows.len(), expected_rows);
    assert!(rows.iter().all(|row| row["display_name"].as_str().is_some()));
    start.elapsed().as_nanos()
}

async fn authorized_presence_write(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    body: &serde_json::Value,
) -> u128 {
    let start = Clock::now();
    let response = post_json(client, base, "/api/team/draft/presence", Some(cookie), body)
        .await;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.json::<serde_json::Value>().await.unwrap()["accepted"], true);
    start.elapsed().as_nanos()
}

#[cfg(feature = "multiplayer")]
#[tokio::test]
#[ignore = "explicit real-HTTP shared-draft presence publication screening"]
async fn shipping_presence_http_publication_measurement() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (router, team, store, calls) = setup_at(address);
    let old_alice = saved(&team, 0);
    let original_bob_session = saved(&team, 1).session_id;
    let stop = CancellationToken::new();
    let closing = stop.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { closing.cancelled().await })
            .await.unwrap();
    });
    let base = format!("http://{address}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(18))
        .pool_max_idle_per_host(16)
        .build().unwrap();

    let mut cookies = Vec::new();
    for code in [ALICE, BOB] {
        let response = post_json(&client, &base, "/api/pair", None,
            &serde_json::json!({"code":code})).await;
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        cookies.push(response.headers()[reqwest::header::SET_COOKIE]
            .to_str().unwrap().split(';').next().unwrap().to_owned());
    }
    let alice_cookie = &cookies[0];
    let bob_cookie = &cookies[1];
    let unauth = post_json(&client, &base, "/api/team/draft/presence/list", None,
        &serde_json::json!({"draft_id":"invalid","generation":1})).await;
    assert_eq!(unauth.status(), reqwest::StatusCode::UNAUTHORIZED);

    let grant = post_json(&client, &base, "/api/team/session/share", Some(alice_cookie),
        &serde_json::json!({"participant_id":"bob","enabled":true,"mode":"on"})).await;
    assert_eq!(grant.status(), reqwest::StatusCode::OK);
    let revision = lock(&team.0.entries[1].app.0.session.state).snapshot.revision;
    let opened = post_json(&client, &base, "/api/team/session/open-shared", Some(bob_cookie),
        &serde_json::json!({
            "expected_revision":revision,"session_id":old_alice.session_id,
            "branch":old_alice.branch_id
        })).await;
    assert_eq!(opened.status(), reqwest::StatusCode::OK);
    let shared_bob_session = saved(&team, 1).session_id;
    assert_eq!(shared_bob_session, old_alice.session_id);
    assert_ne!(shared_bob_session, original_bob_session);
    let draft = post_json(&client, &base, "/api/team/draft/open", Some(bob_cookie),
        &serde_json::json!({"activate_auto":false})).await;
    assert_eq!(draft.status(), reqwest::StatusCode::OK);
    let draft = draft.json::<serde_json::Value>().await.unwrap();
    let draft_id = draft["draft_id"].as_str().unwrap().to_owned();
    let generation = draft["generation"].as_u64().unwrap();
    let read_body = serde_json::json!({"draft_id":draft_id,"generation":generation});
    let write_body = serde_json::json!({
        "draft_id":draft_id,"generation":generation,
        "client_id":"bob-tab","cursor_anchor":0,"cursor_head":0,"active":true
    });

    for rows in [8usize, 128] {
        {
            let mut presence = lock(&team.0.multiplayer.presence);
            multiplayer::seed_bounded_presence_for_http_measurement(
                &mut presence, &draft_id, generation, &old_alice.session_id, rows,
            );
        }
        let _warm = authorized_presence_read(
            &client, &base, alice_cookie, &read_body, rows,
        ).await;
        for readers in [1usize, 4, 8] {
            for (write_part, read_part) in [(1usize, 16usize), (1, 4), (4, 1)] {
                let reads_each = 12;
                let writes = (readers * reads_each * write_part / read_part).max(1);
                let barrier = Shared::new(tokio::sync::Barrier::new(readers + 2));
                let mut tasks = Vec::new();
                let cpu_before = cpu_ticks();
                let started = Clock::now();
                for _ in 0..readers {
                    let barrier = barrier.clone();
                    let client = client.clone();
                    let base = base.clone();
                    let cookie = alice_cookie.clone();
                    let body = read_body.clone();
                    tasks.push(tokio::spawn(async move {
                        barrier.wait().await;
                        let mut latencies = Vec::with_capacity(reads_each);
                        for _ in 0..reads_each {
                            latencies.push(authorized_presence_read(
                                &client, &base, &cookie, &body, rows,
                            ).await);
                        }
                        (false, latencies)
                    }));
                }
                let barrier_writer = barrier.clone();
                let writer_client = client.clone();
                let writer_base = base.clone();
                let writer_cookie = bob_cookie.clone();
                let writer_body = write_body.clone();
                tasks.push(tokio::spawn(async move {
                    barrier_writer.wait().await;
                    let mut latencies = Vec::with_capacity(writes);
                    for _ in 0..writes {
                        latencies.push(authorized_presence_write(
                            &writer_client, &writer_base, &writer_cookie, &writer_body,
                        ).await);
                        tokio::task::yield_now().await;
                    }
                    (true, latencies)
                }));
                barrier.wait().await;
                let mut read_latency = Vec::new();
                let mut write_latency = Vec::new();
                for task in tasks {
                    let (writer, samples) = task.await.unwrap();
                    if writer { write_latency.extend(samples); }
                    else { read_latency.extend(samples); }
                }
                let cpu_delta = cpu_ticks().zip(cpu_before)
                    .map(|(end, start)| end.saturating_sub(start));
                eprintln!("{}", serde_json::json!({
                    "measurement":"shipping_presence_http",
                    "rows":rows,"readers":readers,
                    "reads":read_latency.len(),"writes":writes,
                    "write_read":format!("{write_part}:{read_part}"),
                    "wall_ns":started.elapsed().as_nanos(),"cpu_ticks":cpu_delta,
                    "read_p50_p95_ns":measurement_percentiles(read_latency),
                    "write_p50_p95_ns":measurement_percentiles(write_latency),
                    "limits":"real authenticated team TCP/HTTP + SQLite draft-grant recheck + TTL + JSON; synthetic rows, no real collaboration activity"
                }));
            }
        }
    }
    // Expired data never becomes a visible cached grant. This is independent
    // of the published-vs-mutable representation used for raw presence bytes.
    {
        let mut presence = lock(&team.0.multiplayer.presence);
        for row in presence.values_mut() {
            row.updated_at = Instant::now() - Duration::from_secs(31);
        }
    }
    let expired = post_json(&client, &base, "/api/team/draft/presence/list",
        Some(alice_cookie), &read_body).await;
    assert_eq!(expired.status(), reqwest::StatusCode::OK);
    assert!(expired.json::<serde_json::Value>().await.unwrap()["presence"]
        .as_array().unwrap().is_empty());

    let revoked = post_json(&client, &base, "/api/team/session/share", Some(alice_cookie),
        &serde_json::json!({"participant_id":"bob","enabled":false})).await;
    assert_eq!(revoked.status(), reqwest::StatusCode::OK);
    let denied = post_json(&client, &base, "/api/team/draft/presence/list",
        Some(bob_cookie), &read_body).await;
    assert!(matches!(denied.status(),
        reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::NOT_FOUND));
    assert_eq!(calls.load(Ordering::SeqCst), 0, "provider remains unused");
    assert_eq!(saved(&team, 0).session_id, old_alice.session_id);
    assert_eq!(saved(&team, 1).session_id, shared_bob_session);
    assert_eq!(store.list(&team.0.entries[0].app.0.session.actor).unwrap().len(), 1);
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(5), server).await.unwrap().unwrap();
}
