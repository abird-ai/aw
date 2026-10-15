//! Same-machine, same-UID attachment to the EXISTING AppState/Agent/store.
//! Authentication never comes from a request's actor field or a browser cookie.
use super::*;
use aw_client::{
    framing,
    local::BoundSocket,
    protocol::{self, Command, Packet, PromptInput, Request, Response},
};
use tokio::net::UnixStream;

const CONNECTIONS: usize = 16;
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub(super) async fn serve(app: AppState, bound: BoundSocket) -> Result<()> {
    let uid = app
        .0
        .local_owner_uid
        .ok_or(Error::Denied("local owner capability absent"))?;
    let slots = Arc::new(Semaphore::new(CONNECTIONS));
    let mut children = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = app.0.shutdown.cancelled() => break,
            Some(_) = children.join_next(), if !children.is_empty() => {},
            accepted = bound.listener.accept() => {
                let (socket, _) = accepted?;
                // Kernel identity is checked before allocating a request buffer.
                if socket.peer_cred().map_or(true, |cred| cred.uid() != uid) { continue; }
                let Ok(permit) = slots.clone().try_acquire_owned() else { continue };
                let app = AppState(app.0.clone(), Authority::Local(uid));
                children.spawn(async move {
                    let _permit = permit;
                    // Protocol errors close only this connection, not the owner.
                    let _ = connection(app, socket).await;
                });
            }
        }
    }
    children.abort_all();
    while children.join_next().await.is_some() {}
    drop(bound); // removes only our original socket inode
    Ok(())
}

async fn send(socket: &mut UnixStream, id: u64, packet: Packet) -> aw_client::Result<()> {
    tokio::time::timeout(
        WRITE_TIMEOUT,
        framing::write(
            socket,
            &Response {
                version: protocol::VERSION,
                id,
                packet,
            },
            protocol::MAX_RESPONSE_BYTES,
        ),
    )
    .await
    .map_err(|_| aw_client::Error::Timeout)?
}
fn failure(error: ApiError) -> Packet {
    Packet::Error {
        code: error.0.as_u16(),
        message: error.1.to_owned(),
    }
}
fn as_value<T: Serialize>(value: T) -> ApiResult<serde_json::Value> {
    serde_json::to_value(value).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "cannot serialize owner result",
        )
    })
}
fn hello(app: &AppState) -> aw_client::protocol::Hello {
    aw_client::protocol::Hello {
        protocol: protocol::VERSION,
        instance: lock(&app.0.session.state).snapshot.instance.clone(),
        principal: app.0.session.actor.as_str().to_owned(),
        owner_uid: app.0.local_owner_uid.unwrap_or(u32::MAX),
        package_version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec!["snapshot".into(), "prompt".into(), "follow".into(), "current-accounts".into(), "history-inspection".into(), "context-flow-v1".into()],
        shared_active_view: true,
        inference_location: "explicit per-prompt registered accounts; personal-device custody requires an enabled authorized device link".into(),
    }
}
async fn connection(app: AppState, mut socket: UnixStream) -> aw_client::Result<()> {
    let mut reader = framing::Reader::new();
    let first: Request = tokio::time::timeout(
        Duration::from_secs(5),
        reader.next(&mut socket, protocol::MAX_REQUEST_BYTES),
    )
    .await
    .map_err(|_| aw_client::Error::Timeout)??
    .ok_or(aw_client::Error::Closed)?;
    if first.version != protocol::VERSION
        || first.id != 1
        || first.expected_instance.is_some()
        || !matches!(first.command, Command::Hello)
    {
        send(
            &mut socket,
            first.id,
            Packet::Error {
                code: 400,
                message: "versioned hello must be first".into(),
            },
        )
        .await?;
        return Ok(());
    }
    send(
        &mut socket,
        first.id,
        Packet::Result {
            value: serde_json::to_value(hello(&app))
                .map_err(|_| aw_client::Error::Protocol("hello serialization failed"))?,
        },
    )
    .await?;
    let mut previous = first.id;
    loop {
        let next = tokio::select! {
            _ = app.0.shutdown.cancelled() => return Ok(()),
            value = tokio::time::timeout(IDLE_TIMEOUT, reader.next::<_, Request>(&mut socket, protocol::MAX_REQUEST_BYTES)) => value,
        }.map_err(|_| aw_client::Error::Timeout)??;
        let Some(request) = next else { return Ok(()) };
        if request.version != protocol::VERSION
            || request.id <= previous
            || request.id == 0
            || matches!(request.command, Command::Hello)
        {
            send(
                &mut socket,
                request.id,
                Packet::Error {
                    code: 400,
                    message: "invalid request sequence/version".into(),
                },
            )
            .await?;
            return Ok(());
        }
        previous = request.id;
        let instance = lock(&app.0.session.state).snapshot.instance.clone();
        if request.expected_instance.as_deref() != Some(instance.as_str()) || !auth_alive(&app) {
            send(
                &mut socket,
                request.id,
                Packet::Error {
                    code: 409,
                    message:
                        "owner changed or stopped; reconnect and inspect, never blindly resend"
                            .into(),
                },
            )
            .await?;
            continue;
        }
        if request.command.validate().is_err() {
            send(
                &mut socket,
                request.id,
                Packet::Error {
                    code: 400,
                    message: "invalid bounded command".into(),
                },
            )
            .await?;
            continue;
        }
        if let Command::Follow { run_id } = &request.command {
            follow(&app, &mut socket, request.id, *run_id).await?;
            continue;
        }
        let result = dispatch(&app, request.command).await;
        let packet = match result {
            Ok(value) => Packet::Result { value },
            Err(e) => failure(e),
        };
        // Large views fail explicitly; no truncated successful result.
        if serde_json::to_vec(&packet)
            .map_or(true, |p| p.len() + 128 > protocol::MAX_RESPONSE_BYTES)
        {
            send(
                &mut socket,
                request.id,
                Packet::Error {
                    code: 413,
                    message: "owner view exceeds native frame limit".into(),
                },
            )
            .await?;
        } else {
            send(&mut socket, request.id, packet).await?;
        }
    }
}
fn prompt_command(p: PromptInput) -> PromptCommand {
    PromptCommand {
        request_id: p.request_id,
        expected_revision: p.expected_revision,
        prompt: p.prompt,
        account_id: Some(p.account_id),
        model: Some(p.model),
        session_id: p.session_id,
        branch_id: p.branch_id,
        retry_operation_id: p.retry_operation_id,
    }
}
async fn dispatch(app: &AppState, command: Command) -> ApiResult<serde_json::Value> {
    if !auth_alive(app) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "local owner stopped"));
    }
    match command {
        Command::Context {
            expected_revision,
            command,
        } => {
            let command = serde_json::from_value(command)
                .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid typed context command"))?;
            let response = context_flow::action(
                State(app.clone()),
                Ok(Json(context_flow::RequestBody {
                    expected_revision,
                    command,
                })),
            )
            .await?;
            let bytes = axum::body::to_bytes(response.into_body(), protocol::MAX_RESPONSE_BYTES)
                .await
                .map_err(|_| ApiError(StatusCode::PAYLOAD_TOO_LARGE, "context result too large"))?;
            serde_json::from_slice(&bytes)
                .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "invalid context result"))
        }
        Command::ContextCancel { request_id } => Ok(context_flow::cancel_job(
            State(app.clone()),
            Ok(Json(context_flow::CancelJob { request_id })),
        )
        .await?
        .0),
        Command::Snapshot => as_value(snapshot(State(app.clone())).await?.0),
        Command::Accounts => as_value(accounts(State(app.clone())).await?.0),
        Command::Sessions => Ok(session_list(State(app.clone())).await?.0),
        Command::Branches => Ok(branch_list(State(app.clone())).await?.0),
        Command::Tree => Ok(tree_view(State(app.clone())).await?.0),
        Command::Usage => as_value(stored_usage(State(app.clone())).await?.0),
        Command::Prompt(p) => {
            let (_, Json(receipt)) =
                prompt(State(app.clone()), Ok(Json(prompt_command(p)))).await?;
            as_value(receipt)
        }
        Command::PromptStatus { original } => receipt_status(app, original).await,
        Command::Cancel { run_id } => Ok(cancel(
            State(app.clone()),
            Ok(Json(CancelCommand { run_id })),
        )
        .await?
        .0),
        Command::Reset { expected_revision } => as_value(
            reset(
                State(app.clone()),
                Ok(Json(ResetCommand { expected_revision })),
            )
            .await?
            .0,
        ),
        Command::Open {
            expected_revision,
            session_id,
            branch,
        } => as_value(
            change_view(app, expected_revision, ViewChange::Open(session_id, branch))
                .await?
                .0,
        ),
        Command::Fork {
            expected_revision,
            name,
            at_node,
        } => as_value(
            change_view(app, expected_revision, ViewChange::Fork(name, at_node))
                .await?
                .0,
        ),
        Command::Checkpoint {
            expected_revision,
            summary,
        } => as_value(
            change_view(app, expected_revision, ViewChange::Checkpoint(summary))
                .await?
                .0,
        ),
        Command::Acknowledge {
            expected_revision,
            confirmed_inspected,
        } => as_value(
            acknowledge(
                State(app.clone()),
                Ok(Json(AcknowledgeCommand {
                    expected_revision,
                    confirmed_inspected,
                })),
            )
            .await?
            .0,
        ),
        Command::Inspect { session_id, branch } => {
            let current = current_persistence(app)?;
            let view = store_work(move || {
                // Reading a permitted branch does NOT install a journal, switch
                // the agent, reconstruct credentials or perform provider work.
                let (view, _) = current.store.activate(&current.actor, &session_id, &branch)?;
                Ok(serde_json::json!({"inspection_only":true,
                    "session":{"id":view.session.id,"title":view.session.title,"directory":view.session.binding.working_directory,"imported":view.session.imported},
                    "branch":view.branch,"history":view.history}))
            }).await?;
            Ok(view)
        }
        Command::Hello | Command::Follow { .. } => Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid dispatcher command",
        )),
    }
}
async fn receipt_status(app: &AppState, p: PromptInput) -> ApiResult<serde_json::Value> {
    let fingerprint = hash(
        serde_json::to_vec(&(
            p.expected_revision,
            &p.prompt,
            &p.account_id,
            &p.model,
            app.0.session.actor.as_str(),
            &p.session_id,
            &p.branch_id,
            &p.retry_operation_id,
        ))
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid receipt request"))?
        .as_slice(),
    );
    let digest = fingerprint
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let persistent = lock(&app.0.session.persistence).clone();
    if let Some(saved) = persistent {
        let found =
            store_work(move || saved.store.web_result(&saved.actor, &p.request_id, &digest))
                .await?;
        return Ok(match found {
            Some((receipt, turn)) => {
                serde_json::json!({"found":true,"receipt":receipt,"completion":turn.as_ref().map(|t|serde_json::json!({"turn_id":t.id,"outcome":t.outcome,"error":t.error,"needs_reconciliation":t.needs_reconciliation})),"turn":turn,"durable":true})
            }
            None => {
                serde_json::json!({"found":false,"receipt":null,"completion":null,"turn":null,"durable":true})
            }
        });
    }
    let key = format!("{}:{}", app.0.session.actor.as_str(), p.request_id);
    let state = lock(&app.0.session.state);
    match state.receipts.get(&key) {
        Some((prior, receipt)) if prior == &fingerprint => Ok(
            serde_json::json!({"found":true,"receipt":receipt,"completion":state.completions.get(&receipt.run_id),"turn":null,"durable":false}),
        ),
        Some(_) => Err(ApiError(
            StatusCode::CONFLICT,
            "request ID belongs to different input",
        )),
        None => Ok(
            serde_json::json!({"found":false,"receipt":null,"completion":null,"turn":null,"durable":false}),
        ),
    }
}
async fn send_snapshot(
    app: &AppState,
    socket: &mut UnixStream,
    id: u64,
) -> aw_client::Result<Snapshot> {
    let snapshot = snapshot(State(app.clone()))
        .await
        .map_err(|_| aw_client::Error::Protocol("current derived context access denied"))?
        .0;
    send(
        socket,
        id,
        Packet::Event {
            sequence: snapshot.sequence,
            event: "snapshot".into(),
            data: serde_json::to_value(&snapshot)
                .map_err(|_| aw_client::Error::Protocol("snapshot serialization failed"))?,
        },
    )
    .await?;
    Ok(snapshot)
}
async fn follow(
    app: &AppState,
    socket: &mut UnixStream,
    id: u64,
    run_id: u64,
) -> aw_client::Result<()> {
    let permit = match app.0.event_slots.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return send(
                socket,
                id,
                Packet::Error {
                    code: 429,
                    message: "too many event observers".into(),
                },
            )
            .await;
        }
    };
    let _permit = permit;
    // Subscribe BEFORE snapshot to establish a high-water mark, as with SSE.
    let mut receiver = app.0.session.events.subscribe();
    send(
        socket,
        id,
        Packet::Result {
            value: serde_json::json!({"following":run_id}),
        },
    )
    .await?;
    let mut last = send_snapshot(app, socket, id).await?;
    loop {
        if last.pending.as_ref().is_none_or(|p| p.run_id != run_id) {
            return send(
                socket,
                id,
                Packet::End {
                    reason: "requested_run_not_active; inspect correlated receipt/history".into(),
                },
            )
            .await;
        }
        let event = tokio::select! {
            _ = app.0.shutdown.cancelled() => return send(socket,id,Packet::End{reason:"owner_stopping; execution outcome may need recovery".into()}).await,
            value = receiver.recv() => Some(value),
            _ = tokio::time::sleep(Duration::from_secs(15)) => None,
        };
        if context_flow::visible(app).await.is_err() {
            return send(
                socket,
                id,
                Packet::End {
                    reason: "context_revoked; discard cached derived content".into(),
                },
            )
            .await;
        }
        match event {
            None => {
                send(
                    socket,
                    id,
                    Packet::Event {
                        sequence: last.sequence,
                        event: "keepalive".into(),
                        data: serde_json::json!({}),
                    },
                )
                .await?
            }
            Some(Ok(event)) if event.kind == "auth_expired" => {} // browser-only authority; local peer remains authenticated
            Some(Ok(event)) if event.sequence <= last.sequence => {}
            Some(Ok(event))
                if event.kind == "delta"
                    && event.data.get("run_id").and_then(serde_json::Value::as_u64)
                        == Some(run_id) =>
            {
                last.sequence = event.sequence;
                send(
                    socket,
                    id,
                    Packet::Event {
                        sequence: event.sequence,
                        event: "delta".into(),
                        data: event.data,
                    },
                )
                .await?;
            }
            Some(Ok(_)) | Some(Err(broadcast::error::RecvError::Lagged(_))) => {
                last = send_snapshot(app, socket, id).await?;
            }
            Some(Err(broadcast::error::RecvError::Closed)) => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{
        AgentOptions, CompletionFuture, CompletionMeta, DeltaSink, Message, Provider,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Fixture {
        calls: Arc<AtomicUsize>,
        delay: Duration,
    }
    impl Provider for Fixture {
        fn stream<'a>(
            &'a self,
            _messages: &'a [Message],
            _model: &'a str,
            sink: &'a mut DeltaSink<'_>,
        ) -> CompletionFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                sink("fixture ")?;
                tokio::time::sleep(self.delay).await;
                sink("answer")?;
                Ok(CompletionMeta {
                    finish_reason: "stop".into(),
                    ..Default::default()
                })
            })
        }
    }
    fn setup(durable: bool, local: bool) -> (AppState, Arc<AtomicUsize>) {
        let uid = aw_client::local::effective_uid().unwrap();
        let actor = PrincipalId::new(format!("unix:{uid}")).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut options = AgentOptions::new("fixture", "tiny");
        options.system_prompt = "test".into();
        let agent = Agent::builder(
            Arc::new(Fixture {
                calls: calls.clone(),
                delay: Duration::from_millis(25),
            }),
            options,
        )
        .account(
            AccountDescriptor::personal("fixture", actor.clone(), "local", ["tiny".to_owned()]),
            actor.clone(),
            "local",
        )
        .build()
        .unwrap();
        let persistence = if durable {
            let store = aw_store::SessionStore::in_memory().unwrap();
            Some(
                PersistentSession::create(
                    store,
                    actor,
                    aw_store::SessionBinding {
                        working_directory: "/tmp".into(),
                        tool_root: "/tmp".into(),
                        system_prompt: "test".into(),
                        profile_hint: "fixture".into(),
                        model_hint: "tiny".into(),
                        destination_fingerprint: "f".repeat(64),
                    },
                    "Native test",
                )
                .unwrap(),
            )
        } else {
            None
        };
        let (_, app) = application_configured(
            agent,
            persistence,
            None,
            SocketAddr::from(([127, 0, 0, 1], 8888)),
            &"b".repeat(64),
            local.then_some(uid),
        )
        .unwrap();
        (AppState(app.0, Authority::Local(uid)), calls)
    }
    async fn input(app: &AppState, id: &str) -> PromptInput {
        let view: protocol::Snapshot =
            serde_json::from_value(dispatch(app, Command::Snapshot).await.unwrap()).unwrap();
        view.prepare(id.into(), "A fixture request".into(), None)
            .unwrap()
    }
    async fn settled(app: &AppState) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if lock(&app.0.session.state).snapshot.pending.is_none() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
    }
    #[test]
    fn browser_owner_has_no_implicit_native_authority() {
        let (app, _) = setup(false, false);
        assert!(!auth_alive(&app));
    }
    #[test]
    fn local_authority_does_not_authenticate_browser() {
        let (app, _) = setup(false, true);
        assert!(auth_alive(&app));
        assert!(!auth_alive(&AppState(app.0.clone(), Authority::Browser)));
        assert!(!auth_alive(&AppState(app.0, Authority::Local(u32::MAX))));
    }
    #[test]
    fn handshake_contains_no_credentials_or_endpoints() {
        let (app, calls) = setup(false, true);
        let value = serde_json::to_value(hello(&app)).unwrap();
        assert_eq!(
            value["principal"],
            format!("unix:{}", aw_client::local::effective_uid().unwrap())
        );
        assert!(value.get("api_key").is_none());
        assert!(value.get("base_url").is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn receipt_lookup_never_dispatches() {
        let (app, calls) = setup(false, true);
        let p = input(&app, "missing").await;
        let result = dispatch(&app, Command::PromptStatus { original: p })
            .await
            .unwrap();
        assert_eq!(result["found"], false);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn two_native_views_use_one_agent_and_exact_receipt() {
        let (app, calls) = setup(false, true);
        let other = app.clone();
        let p = input(&app, "once").await;
        let one = dispatch(&app, Command::Prompt(p.clone())).await.unwrap();
        settled(&app).await;
        let two = dispatch(&other, Command::Prompt(p.clone())).await.unwrap();
        assert_eq!(one["run_id"], two["run_id"]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let result = dispatch(&other, Command::PromptStatus { original: p })
            .await
            .unwrap();
        assert_eq!(result["completion"]["outcome"], "completed");
        assert_eq!(lock(&app.0.session.state).snapshot.turns.len(), 1);
    }
    #[tokio::test]
    async fn stale_revision_is_not_silently_rebased() {
        let (app, calls) = setup(false, true);
        let p = input(&app, "first").await;
        let mut stale = p.clone();
        stale.request_id = "second".into();
        dispatch(&app, Command::Prompt(p)).await.unwrap();
        settled(&app).await;
        let error = dispatch(&app, Command::Prompt(stale)).await.unwrap_err();
        assert_eq!(error.0, StatusCode::CONFLICT);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn account_rejection_does_not_fallback() {
        let (app, calls) = setup(false, true);
        let mut p = input(&app, "denied").await;
        p.account_id = "someone-elses-key".into();
        assert_eq!(
            dispatch(&app, Command::Prompt(p)).await.unwrap_err().0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn memory_receipt_outcome_survives_reset_without_old_text_copy() {
        let (app, calls) = setup(false, true);
        let p = input(&app, "before-reset").await;
        dispatch(&app, Command::Prompt(p.clone())).await.unwrap();
        settled(&app).await;
        let revision = lock(&app.0.session.state).snapshot.revision;
        dispatch(
            &app,
            Command::Reset {
                expected_revision: revision,
            },
        )
        .await
        .unwrap();
        let result = dispatch(&app, Command::PromptStatus { original: p })
            .await
            .unwrap();
        assert_eq!(result["completion"]["outcome"], "completed");
        assert!(result["turn"].is_null());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn inspect_other_branch_does_not_select_or_dispatch() {
        let (app, calls) = setup(true, true);
        let current = current_persistence(&app).unwrap();
        let b = current.handle.branch().unwrap();
        let other = current
            .store
            .fork(
                &current.actor,
                current.handle.session_id(),
                "alternative",
                &b.head,
            )
            .unwrap();
        let before = dispatch(&app, Command::Snapshot).await.unwrap();
        let read = dispatch(
            &app,
            Command::Inspect {
                session_id: current.handle.session_id().into(),
                branch: other.id.clone(),
            },
        )
        .await
        .unwrap();
        let after = dispatch(&app, Command::Snapshot).await.unwrap();
        assert_eq!(read["branch"]["id"], other.id);
        assert_eq!(before["saved"], after["saved"]);
        assert_eq!(before["revision"], after["revision"]);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn durable_result_is_correlated_after_active_view_switch() {
        let (app, calls) = setup(true, true);
        let p = input(&app, "durable").await;
        dispatch(&app, Command::Prompt(p.clone())).await.unwrap();
        settled(&app).await;
        let revision = lock(&app.0.session.state).snapshot.revision;
        dispatch(
            &app,
            Command::Reset {
                expected_revision: revision,
            },
        )
        .await
        .unwrap();
        let status = dispatch(&app, Command::PromptStatus { original: p })
            .await
            .unwrap();
        assert_eq!(status["turn"]["answer"], "fixture answer");
        assert_eq!(status["completion"]["outcome"], "completed");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn shared_browser_logout_does_not_cancel_native_run() {
        let (app, calls) = setup(false, true);
        let p = input(&app, "shared").await;
        dispatch(&app, Command::Prompt(p.clone())).await.unwrap();
        logout(State(AppState(app.0.clone(), Authority::Browser)))
            .await
            .unwrap();
        settled(&app).await;
        assert!(auth_alive(&app));
        assert!(!auth_alive(&AppState(app.0.clone(), Authority::Browser)));
        let result = dispatch(&app, Command::PromptStatus { original: p })
            .await
            .unwrap();
        assert_eq!(result["completion"]["outcome"], "completed");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn transport_rejects_wrong_owner_instance_before_dispatch() {
        let (app, calls) = setup(false, true);
        let (server, mut client) = UnixStream::pair().unwrap();
        let task = tokio::spawn(connection(app, server));
        let mut r = framing::Reader::new();
        framing::write(
            &mut client,
            &Request {
                version: 1,
                id: 1,
                expected_instance: None,
                command: Command::Hello,
            },
            protocol::MAX_REQUEST_BYTES,
        )
        .await
        .unwrap();
        let _: Response = r
            .next(&mut client, protocol::MAX_RESPONSE_BYTES)
            .await
            .unwrap()
            .unwrap();
        framing::write(
            &mut client,
            &Request {
                version: 1,
                id: 2,
                expected_instance: Some("f".repeat(64)),
                command: Command::Snapshot,
            },
            protocol::MAX_REQUEST_BYTES,
        )
        .await
        .unwrap();
        let reply: Response = r
            .next(&mut client, protocol::MAX_RESPONSE_BYTES)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(reply.packet, Packet::Error { code: 409, .. }));
        drop(client);
        task.await.unwrap().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
