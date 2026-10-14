//! Device-side execution of a locally fixed Provider. Does not execute tools.
//! A request only supplies model input; account, endpoint and max-token settings
//! are resolved by the local application before this worker is constructed.
use crate::{
    grant::{self, Grant},
    protocol::{self, Event, Failure, PairingKey, Request},
    receipts::{ReceiptBook, State},
    Error, Result,
};
use aw_agent_core::{CancellationToken, Message, ModelReply, Provider};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::Notify,
};

struct Continuation {
    family: (String, u64),
    public_digest: String,
    private_value: String,
}
#[derive(Default)]
struct TextBuffer {
    full: String,
    pending: String,
}
impl TextBuffer {
    fn take(&mut self) -> Option<String> {
        if self.pending.is_empty() {
            return None;
        }
        let mut end = self.pending.len().min(protocol::DELTA_LIMIT);
        while !self.pending.is_char_boundary(end) {
            end -= 1;
        }
        Some(self.pending.drain(..end).collect())
    }
}
pub struct DeviceWorker {
    provider: Arc<dyn Provider>,
    grant: Grant,
    grant_path: PathBuf,
    grant_digest: String,
    receipts: ReceiptBook,
    continuations: BTreeMap<String, Continuation>,
    ready_observer: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl DeviceWorker {
    /// Local trusted host supplies the provider/profile fingerprint; it must not
    /// reconstruct a provider from request metadata. No secret enters that hash.
    pub fn open(
        provider: Arc<dyn Provider>,
        grant_path: &Path,
        receipts_path: &Path,
        profile_digest: &str,
    ) -> Result<Self> {
        if provider.delegated_binding().is_some() {
            return Err(Error::Denied(
                "device cannot implicitly chain to another delegated provider",
            ));
        }
        let (grant, grant_digest) = Grant::load(grant_path)?;
        let receipts = ReceiptBook::open(receipts_path, &grant, profile_digest)?;
        Ok(Self {
            provider,
            grant,
            grant_path: grant_path.into(),
            grant_digest,
            receipts,
            continuations: BTreeMap::new(),
            ready_observer: None,
        })
    }
    /// Optional trusted host notification after reciprocal authentication. It
    /// carries no request, secret or permission and does not trigger inference.
    pub fn set_ready_observer(&mut self, observer: Arc<dyn Fn() + Send + Sync>) {
        self.ready_observer = Some(observer);
    }
    pub fn grant(&self) -> &Grant {
        &self.grant
    }
    pub fn receipts(&self) -> &ReceiptBook {
        &self.receipts
    }
    fn recheck(&self) -> Result<()> {
        let (current, digest) = Grant::load(&self.grant_path)?;
        if digest != self.grant_digest || current.binding != self.grant.binding {
            return Err(Error::Denied("local device grant changed or was revoked"));
        }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    pub async fn connect(
        &mut self,
        socket: &Path,
        key: Arc<PairingKey>,
        stop: CancellationToken,
    ) -> Result<()> {
        let uid = aw_client::local::effective_uid()
            .map_err(|_| Error::Denied("Linux device identity unavailable"))?;
        let before = aw_client::local::socket_identity(socket, uid)
            .map_err(|_| Error::Denied("device must connect to an owned private Unix socket, directly or explicitly tunneled"))?;
        let stream = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::UnixStream::connect(socket),
        )
        .await
        .map_err(|_| Error::Timeout)??;
        let after = aw_client::local::socket_identity(socket, uid)
            .map_err(|_| Error::Denied("device socket changed"))?;
        if before != after || stream.peer_cred().map_or(true, |p| p.uid() != uid) {
            return Err(Error::Denied("device socket peer/path identity changed"));
        }
        self.run_stream(stream, key, stop).await
    }
    /// Low-level embedding/test seam. The caller must supply a confidential,
    /// authenticated transport (the shipped path is private Unix/explicit SSH).
    /// HMAC frames authenticate bytes; they do not encrypt a generic TCP stream.
    pub async fn run_stream<S: AsyncRead + AsyncWrite + Unpin + Send>(
        &mut self,
        mut stream: S,
        key: Arc<PairingKey>,
        stop: CancellationToken,
    ) -> Result<()> {
        self.recheck()?;
        let (hello, nonce) = tokio::select! {
            _ = stop.cancelled() => return Err(Error::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(5), protocol::worker_handshake(&mut stream, &key, &self.grant.binding)) => result.map_err(|_| Error::Timeout)??,
        };
        let (read, write) = tokio::io::split(stream);
        let mut tx = protocol::Sender::new(
            write,
            protocol::Authenticator::new(key.clone(), &hello, &nonce, false)?,
        );
        let mut rx = protocol::Receiver::new(
            read,
            protocol::Authenticator::new(key, &hello, &nonce, false)?,
        );
        if let Some(observer) = &self.ready_observer {
            observer();
        }
        let mut requests = 0u32;
        loop {
            self.recheck()?;
            let request = loop {
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => return Err(Error::Cancelled),
                    incoming = rx.next::<Request>() => break incoming?.ok_or(Error::Transport("owner disconnected; no automatic reconnect"))?,
                    _ = tokio::time::sleep(Duration::from_secs(1)) => self.recheck()?,
                }
            };
            if request.epoch != hello.epoch {
                return Err(Error::Protocol("device request epoch changed"));
            }
            match self.execute(&request, &mut tx, &mut rx, &stop).await {
                Ok(()) => {}
                Err(error) => {
                    // Never leak local endpoint/config/provider error strings.
                    // Connection/cancellation faults terminate, not recycle a
                    // partially written authenticated frame.
                    return Err(error);
                }
            }
            requests += 1;
            if requests >= 128 {
                return Err(Error::Limit(
                    "device connection request limit; reconnect explicitly with the same receipts",
                ));
            }
        }
    }
    fn messages(&mut self, request: &Request) -> Result<Vec<Message>> {
        let scope = request
            .context
            .scope
            .as_ref()
            .ok_or(Error::Denied("session scope required"))?;
        let family = (scope.session_id.clone(), request.context.turn_id);
        self.continuations.retain(|_, c| c.family == family);
        let mut messages = Vec::with_capacity(request.messages.len());
        for wire in &request.messages {
            let mut message = wire.public_message();
            if let Some(token) = &wire.continuation {
                let value = self.continuations.get(token).ok_or(Error::Denied(
                    "private model continuation no longer available; no automatic reconstruction",
                ))?;
                if value.family != family
                    || value.public_digest != protocol::digest(&(&wire.content, &wire.tool_calls))?
                {
                    return Err(Error::Denied(
                        "private continuation does not match this turn and assistant block",
                    ));
                }
                message.provider_context = Some(value.private_value.clone());
            }
            messages.push(message);
        }
        aw_agent_core::validate_messages(&messages)?;
        if messages.iter().map(Message::context_bytes).sum::<usize>() > self.grant.max_context_bytes
        {
            return Err(Error::Limit(
                "context plus local private continuation exceeds grant",
            ));
        }
        Ok(messages)
    }
    async fn fail<W: AsyncWrite + Unpin>(
        tx: &mut protocol::Sender<W>,
        id: &str,
        sequence: u64,
        code: Failure,
    ) -> Result<()> {
        tx.send(&Event::Failed {
            id: id.into(),
            sequence,
            code,
        })
        .await
    }
    async fn flush<W: AsyncWrite + Unpin>(
        &self,
        buffer: &Arc<Mutex<TextBuffer>>,
        tx: &mut protocol::Sender<W>,
        id: &str,
        sequence: &mut u64,
    ) -> Result<()> {
        loop {
            let text = buffer
                .lock()
                .map_err(|_| Error::Transport("device text buffer unavailable"))?
                .take();
            let Some(text) = text else { return Ok(()) };
            self.recheck()?;
            tx.send(&Event::Delta {
                id: id.into(),
                sequence: *sequence,
                text,
            })
            .await?;
            *sequence = sequence
                .checked_add(1)
                .ok_or(Error::Limit("device event sequence exhausted"))?;
        }
    }
    async fn execute<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
        &mut self,
        request: &Request,
        tx: &mut protocol::Sender<W>,
        rx: &mut protocol::Receiver<R>,
        stop: &CancellationToken,
    ) -> Result<()> {
        let mut sequence = 1u64;
        // Admission and private-context reconstruction happen before recording a
        // paid attempt. Denied schemas/identities never poll the Provider future.
        self.recheck()?;
        if let Err(error) = self.grant.admit(request, grant::now()?) {
            return Self::fail(tx, &request.id, sequence, classify(&error)).await;
        }
        let messages = match self.messages(request) {
            Ok(messages) => messages,
            Err(error) => return Self::fail(tx, &request.id, sequence, classify(&error)).await,
        };
        let tools: Vec<_> = request.tools.iter().map(|t| t.spec()).collect();
        if !tools.is_empty() && !self.provider.supports_tools() {
            return Self::fail(tx, &request.id, sequence, Failure::Denied).await;
        }
        if let Err(error) = self.receipts.begin(request, &self.grant, grant::now()?) {
            if matches!(error, Error::Storage(_) | Error::Io(_)) {
                return Err(error);
            }
            return Self::fail(tx, &request.id, sequence, classify(&error)).await;
        }
        let buffer = Arc::new(Mutex::new(TextBuffer::default()));
        let notify = Arc::new(Notify::new());
        let view = buffer.clone();
        let signal = notify.clone();
        let max_output = self.grant.max_output_bytes;
        let mut sink = move |text: &str| -> Result<()> {
            if text.is_empty() {
                return Ok(());
            }
            let mut state = view
                .lock()
                .map_err(|_| Error::Transport("device text buffer unavailable"))?;
            if state.full.len().saturating_add(text.len()) > max_output {
                return Err(Error::Limit("local response budget exhausted"));
            }
            state.full.push_str(text);
            state.pending.push_str(text);
            signal.notify_one();
            Ok(())
        };
        let provider = self.provider.clone();
        let mut inference =
            provider.stream_tools(&messages, &self.grant.binding.model, &tools, &mut sink);
        let deadline = tokio::time::Instant::now()
            + Duration::from_secs(self.grant.remaining_seconds(grant::now()?));
        let result: Result<ModelReply> = loop {
            tokio::select! {
                biased;
                _ = stop.cancelled() => break Err(Error::Cancelled),
                _ = tokio::time::sleep_until(deadline) => break Err(Error::Timeout),
                other = rx.next::<Request>() => {
                    let _ = other;
                    break Err(Error::Transport("owner disconnected or pipelined another request while busy"));
                }
                reply = inference.as_mut() => break reply,
                _ = notify.notified() => {
                    if let Err(error) = self.flush(&buffer, tx, &request.id, &mut sequence).await { break Err(error); }
                }
                _ = tokio::time::sleep(Duration::from_millis(500)) => {
                    if let Err(error) = self.recheck() { break Err(error); }
                }
            }
        };
        drop(inference); // cooperative provider cancellation before settling local receipt
        drop(sink);
        let text = buffer
            .lock()
            .map_err(|_| Error::Transport("device text buffer unavailable"))?
            .full
            .clone();
        let result = result.and_then(|reply| {
            self.recheck()?;
            protocol::validate_reply(&reply, &request.tools)?;
            let bytes = text.len().saturating_add(if reply.tool_calls.is_empty() {
                0
            } else {
                serde_json::to_vec(&reply.tool_calls)
                    .map_err(|_| Error::Protocol("invalid tool response"))?
                    .len()
            });
            if bytes > self.grant.max_output_bytes {
                return Err(Error::Limit(
                    "text and completed tool proposals exceed local output budget",
                ));
            }
            Ok((reply, bytes))
        });
        match result {
            Ok((reply, bytes)) => {
                // Private reasoning/continuation remains in this process. Only
                // a random, turn/block-bound lookup handle crosses to the owner.
                let continuation = if let Some(value) = &reply.provider_context {
                    if self.continuations.len() >= 32 {
                        return Err(Error::Limit("private continuation cache full"));
                    }
                    let token = protocol::random_id()?;
                    let scope = request
                        .context
                        .scope
                        .as_ref()
                        .ok_or(Error::Denied("scope lost"))?;
                    self.continuations.insert(
                        token.clone(),
                        Continuation {
                            family: (scope.session_id.clone(), request.context.turn_id),
                            public_digest: protocol::digest(&(&text, &reply.tool_calls))?,
                            private_value: value.clone(),
                        },
                    );
                    Some(token)
                } else {
                    None
                };
                self.receipts.finish(
                    &request.id,
                    State::Finished,
                    bytes,
                    Some(reply.meta.clone()),
                    grant::now()?,
                )?;
                self.flush(&buffer, tx, &request.id, &mut sequence).await?;
                self.recheck()?;
                tx.send(&Event::Finished {
                    id: request.id.clone(),
                    sequence,
                    meta: reply.meta,
                    tool_calls: reply.tool_calls,
                    continuation,
                })
                .await
            }
            Err(error) => {
                let state = if matches!(
                    error,
                    Error::Cancelled | Error::Timeout | Error::Transport(_)
                ) {
                    State::Interrupted
                } else {
                    State::Failed
                };
                self.receipts
                    .finish(&request.id, state, text.len(), None, grant::now()?)?;
                // A partially sent frame, lost connection, cancellation or
                // revoked grant must close the stream; do not reuse it for errors.
                if matches!(
                    error,
                    Error::Cancelled
                        | Error::Timeout
                        | Error::Transport(_)
                        | Error::Io(_)
                        | Error::Storage(_)
                ) {
                    return Err(error);
                }
                self.flush(&buffer, tx, &request.id, &mut sequence).await?;
                Self::fail(tx, &request.id, sequence, classify(&error)).await
            }
        }
    }
}
fn classify(error: &Error) -> Failure {
    match error {
        Error::Denied(_) | Error::Config(_) => Failure::Denied,
        Error::Limit(_) => Failure::Limit,
        Error::Conflict(_) => Failure::AlreadyAttempted,
        Error::Cancelled => Failure::Cancelled,
        Error::Protocol(_) => Failure::Protocol,
        Error::Http { .. } => Failure::Provider,
        _ => Failure::Unknown,
    }
}
