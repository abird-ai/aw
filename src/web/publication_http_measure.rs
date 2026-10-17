// Opt-in real-loopback HTTP/SSE measurement of the shipping Session owner.
// Uses a synthetic in-memory pending display; there is no durable input,
// provider dispatch, tool grant, real account or service outside the test.
use std::{sync::Arc, time::Instant as Clock};

fn measurement_percentiles(mut samples: Vec<u128>) -> [u128; 2] {
    if samples.is_empty() {
        return [0, 0];
    }
    samples.sort_unstable();
    [50, 95].map(|p| samples[(samples.len() - 1) * p / 100])
}

fn measurement_cpu_ticks() -> Option<u64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    let fields = fields.split_whitespace().collect::<Vec<_>>();
    Some(fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?)
}

async fn measure_session_request(
    client: &reqwest::Client,
    url: &str,
    cookie: &str,
) -> (u128, usize) {
    let start = Clock::now();
    let response = client.get(url).header(reqwest::header::COOKIE, cookie)
        .send().await.expect("authorized loopback GET");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers()[reqwest::header::CACHE_CONTROL], "no-store");
    let content = response.bytes().await.expect("complete bounded snapshot");
    let latency = start.elapsed().as_nanos();
    assert!(content.windows(b"\"pending\"".len()).any(|w| w == b"\"pending\""));
    (latency, content.len())
}

async fn measure_initial_sse(
    client: &reqwest::Client,
    base: &str,
    cookie: &str,
    expected_sequence: u64,
) -> (u128, usize) {
    let start = Clock::now();
    let mut response = client
        .get(format!("{base}/api/events"))
        .header(reqwest::header::COOKIE, cookie)
        .send().await.expect("authenticated loopback event stream");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert!(response.headers()[reqwest::header::CONTENT_TYPE]
        .to_str().unwrap().starts_with("text/event-stream"));
    let mut total = Vec::new();
    // Read precisely the first complete SSE frame; never hold a stream client
    // across a benchmark run or block the eight permitted SSE slots.
    loop {
        let next = tokio::time::timeout(Duration::from_secs(12), response.chunk())
            .await.expect("SSE frame deadline")
            .expect("SSE socket read")
            .expect("SSE stream must not close before first snapshot");
        total.extend_from_slice(&next);
        assert!(total.len() <= 5 * 1024 * 1024, "SSE remains display bounded");
        if total.windows(b"\n\n".len()).any(|w| w == b"\n\n") {
            break;
        }
    }
    let frame = std::str::from_utf8(&total).expect("SSE text");
    assert!(frame.contains("event: snapshot"), "initial SSE must contain a snapshot");
    assert!(frame.contains(&format!("id: {expected_sequence}")));
    drop(response);
    (start.elapsed().as_nanos(), total.len())
}

/// Run only on explicit request; this executes the real authenticated Axum
/// middleware, TCP HTTP, JSON encoding and initial SSE, NOT a left-right route
/// implementation. Internal state mutations emulate display deltas only.
#[tokio::test]
#[ignore = "explicit real-HTTP Session/SSE publication screening"]
async fn shipping_session_http_sse_publication_measurement() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let agent = Agent::builder(
        Arc::new(Echo(calls.clone())),
        AgentOptions::new("fixture", "test-model"),
    ).build().unwrap();
    let (router, app) = application(agent, address, CODE).unwrap();
    let server_stop = CancellationToken::new();
    let stop = server_stop.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { stop.cancelled().await })
            .await.unwrap();
    });
    let base = format!("http://{address}");
    let endpoint = format!("{base}/api/session");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .pool_max_idle_per_host(16)
        .build().unwrap();
    let pair = client.post(format!("{base}/api/pair"))
        .header(reqwest::header::ORIGIN, &base)
        .json(&serde_json::json!({"code": CODE}))
        .send().await.unwrap();
    assert_eq!(pair.status(), reqwest::StatusCode::OK);
    let cookie = pair.headers()[reqwest::header::SET_COOKIE].to_str().unwrap()
        .split(';').next().unwrap().to_owned();
    assert!(cookie.contains('='));
    // Confirm actual authentication occurs before any display read.
    assert_eq!(client.get(&endpoint).send().await.unwrap().status(),
        reqwest::StatusCode::UNAUTHORIZED);

    for (payload_bytes, reads_each) in [(32 * 1024, 32usize), (256 * 1024, 16), (4 * 1024 * 1024, 3)] {
        {
            let mut state = lock(&app.0.session.state);
            state.snapshot.pending = Some(Pending {
                run_id: 1,
                prompt: "synthetic unsubmitted display".into(),
                answer: "x".repeat(payload_bytes),
                tool_records: Vec::new(),
                identity: None,
            });
            state.snapshot.sequence = state.snapshot.sequence.checked_add(1).unwrap();
        }
        let sequence = lock(&app.0.session.state).snapshot.sequence;
        let (sse_ns, sse_bytes) = measure_initial_sse(&client, &base, &cookie, sequence).await;
        let (warm_ns, warm_bytes) = measure_session_request(&client, &endpoint, &cookie).await;
        eprintln!("{}", serde_json::json!({
            "measurement":"shipping_session_sse_first","bytes":payload_bytes,
            "sse_ns":sse_ns,"sse_encoded_bytes":sse_bytes,
            "http_warm_ns":warm_ns,"http_encoded_bytes":warm_bytes
        }));
        for readers in [1usize, 4, 8] {
            for (write_part, read_part) in [(1usize, 16usize), (1, 4), (4, 1)] {
                let writes = (readers * reads_each * write_part / read_part).max(1);
                let barrier = Arc::new(tokio::sync::Barrier::new(readers + 2));
                let mut tasks = Vec::new();
                let endpoint = Arc::new(endpoint.clone());
                let cookie = Arc::new(cookie.clone());
                let cpu_before = measurement_cpu_ticks();
                let start = Clock::now();
                for _ in 0..readers {
                    let barrier = barrier.clone();
                    let client = client.clone();
                    let endpoint = endpoint.clone();
                    let cookie = cookie.clone();
                    tasks.push(tokio::spawn(async move {
                        barrier.wait().await;
                        let mut latencies = Vec::with_capacity(reads_each);
                        let mut byte_count = 0usize;
                        for _ in 0..reads_each {
                            let (ns, bytes) = measure_session_request(&client, &endpoint, &cookie).await;
                            latencies.push(ns);
                            byte_count += bytes;
                        }
                        (false, latencies, byte_count)
                    }));
                }
                let publisher = app.0.session.clone();
                let barrier_writer = barrier.clone();
                tasks.push(tokio::spawn(async move {
                    barrier_writer.wait().await;
                    let mut latencies = Vec::with_capacity(writes);
                    for _ in 0..writes {
                        let started = Clock::now();
                        publisher.delta(1, "z");
                        latencies.push(started.elapsed().as_nanos());
                        tokio::task::yield_now().await;
                    }
                    (true, latencies, 0)
                }));
                barrier.wait().await;
                let mut read_latencies = Vec::new();
                let mut write_latencies = Vec::new();
                let mut transferred = 0;
                for task in tasks {
                    let (writer, latencies, bytes) = task.await.unwrap();
                    if writer { write_latencies.extend(latencies); }
                    else { read_latencies.extend(latencies); transferred += bytes; }
                }
                let cpu_ticks = measurement_cpu_ticks().zip(cpu_before)
                    .map(|(end, start)| end.saturating_sub(start));
                eprintln!("{}", serde_json::json!({
                    "measurement":"shipping_session_http","payload_bytes":payload_bytes,
                    "readers":readers,"reads":read_latencies.len(),"writes":writes,
                    "write_read":format!("{write_part}:{read_part}"),
                    "wall_ns":start.elapsed().as_nanos(),
                    "cpu_ticks":cpu_ticks,
                    "read_p50_p95_ns":measurement_percentiles(read_latencies),
                    "write_p50_p95_ns":measurement_percentiles(write_latencies),
                    "http_payload_bytes":transferred,
                    "limits":"real paired TCP HTTP + reauthorized accounts + snapshot JSON; writes are synthetic display deltas, not model effects"
                }));
            }
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0, "no provider ever invoked");
    assert!(lock(&app.0.session.persistence).is_none(), "no new Session");
    lock(&app.0.auth).cookie_hash = None;
    assert_eq!(client.get(&endpoint)
        .header(reqwest::header::COOKIE, &cookie)
        .send().await.unwrap().status(), reqwest::StatusCode::UNAUTHORIZED);
    server_stop.cancel();
    tokio::time::timeout(Duration::from_secs(5), server).await.unwrap().unwrap();
}
