//! Owner-side delegated Provider. A single authenticated device connection owns
//! one bounded queue. Disconnect destroys that queue; no request migrates to a
//! replacement connection or falls back to an owner-held account.
use crate::{
    grant::LinkConfig,
    private,
    protocol::{self, Binding, Event, Hello, PairingKey, Request, WireMessage, WireTool},
    Error, Result,
};
use aw_agent_core::{
    AccountDescriptor, AccountOwner, BillingOwner, CancellationToken, CompletionFuture,
    CredentialCustody, DelegatedBinding, DeltaSink, InvocationContext, Message, ModelFuture,
    ModelReply, PrincipalId, Provider, ToolSpec,
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::UnixStream, sync::mpsc};

struct Job {
    request: Request,
    result: mpsc::Sender<Event>,
}
#[derive(Clone)]
struct Online {
    epoch: String,
    jobs: mpsc::Sender<Job>,
}
pub struct Bridge {
    binding: Binding,
    online: Mutex<Option<Online>>,
}
pub struct DeviceListener {
    bridge: Arc<Bridge>,
    bound: aw_client::local::BoundSocket,
    key: Arc<PairingKey>,
}
impl Bridge {
    /// Called explicitly within a Tokio runtime. Registration never sends a
    /// provider secret; it binds a separate private Unix endpoint for this link.
    pub fn bind(link: &LinkConfig, actor: &PrincipalId) -> Result<(Arc<Self>, DeviceListener)> {
        link.validate()?;
        let binding = link.binding(actor.as_str());
        binding.validate()?;
        let key = Arc::new(private::key(&link.key_file)?);
        let bound = aw_client::local::BoundSocket::bind(&link.socket).map_err(|_| {
            Error::Denied(
                "cannot bind private device socket; inspect existing path and permissions",
            )
        })?;
        let bridge = Arc::new(Self {
            binding,
            online: Mutex::new(None),
        });
        Ok((bridge.clone(), DeviceListener { bridge, bound, key }))
    }
    pub fn descriptor(&self) -> Result<AccountDescriptor> {
        Ok(AccountDescriptor::personal(
            self.binding.account_id.clone(),
            PrincipalId::new(self.binding.actor.clone())?,
            self.binding.executor_id.clone(),
            [self.binding.model.clone()],
        ))
    }
    pub fn online(&self) -> bool {
        self.online.lock().map(|s| s.is_some()).unwrap_or(false)
    }
    fn validate_identity(&self, context: &InvocationContext, model: &str) -> Result<()> {
        let i = &context.identity;
        if i.actor.as_str() != self.binding.actor
            || i.account_id != self.binding.account_id
            || model != self.binding.model
            || i.model != model
            || i.inference_executor != self.binding.executor_id
            || i.account_owner != AccountOwner::User(i.actor.clone())
            || i.payer != BillingOwner::User(i.actor.clone())
            || i.credential_custody
                != CredentialCustody::UserDevice(self.binding.executor_id.clone())
        {
            return Err(Error::Denied(
                "personal-device invocation identity does not match its registered link",
            ));
        }
        Request::identity(context)?;
        Ok(())
    }
}
impl Provider for Bridge {
    fn delegated_binding(&self) -> Option<DelegatedBinding> {
        Some(DelegatedBinding {
            account_id: self.binding.account_id.clone(),
            executor_id: self.binding.executor_id.clone(),
        })
    }
    fn supports_tools(&self) -> bool {
        true
    }
    fn stream<'a>(
        &'a self,
        _: &'a [Message],
        _: &'a str,
        _: &'a mut DeltaSink<'_>,
    ) -> CompletionFuture<'a> {
        Box::pin(async {
            Err(Error::Denied(
                "device inference needs an explicit journaled actor/session invocation",
            ))
        })
    }
    fn invoke<'a>(
        &'a self,
        context: &'a InvocationContext,
        messages: &'a [Message],
        model: &'a str,
        tools: &'a [ToolSpec],
        sink: &'a mut DeltaSink<'_>,
    ) -> ModelFuture<'a> {
        Box::pin(async move {
            self.validate_identity(context, model)?;
            let online = self
                .online
                .lock()
                .map_err(|_| Error::Transport("device connection registry unavailable"))?
                .clone()
                .ok_or(Error::Transport(
                    "personal inference device offline; no fallback account selected",
                ))?;
            let request = Request {
                version: protocol::VERSION,
                id: Request::identity(context)?,
                epoch: online.epoch,
                context: context.clone(),
                messages: messages.iter().map(WireMessage::from_message).collect(),
                tools: tools.iter().map(WireTool::from).collect(),
            };
            request.validate()?;
            let id = request.id.clone();
            let specs = request.tools.clone();
            let (result, mut incoming) = mpsc::channel(64);
            online
                .jobs
                .try_send(Job { request, result })
                .map_err(|_| Error::Transport("device disconnected or busy; no queued retry"))?;
            let mut next = 1u64;
            let mut output = 0usize;
            while let Some(event) = incoming.recv().await {
                let (got_id, sequence) = match &event {
                    Event::Delta { id, sequence, .. }
                    | Event::Finished { id, sequence, .. }
                    | Event::Failed { id, sequence, .. } => (id, *sequence),
                };
                if got_id != &id || sequence != next {
                    return Err(Error::Protocol("device result identity/sequence mismatch"));
                }
                next = next
                    .checked_add(1)
                    .ok_or(Error::Limit("device result sequence exhausted"))?;
                match event {
                    Event::Delta { text, .. } => {
                        if text.is_empty() || text.len() > protocol::DELTA_LIMIT {
                            return Err(Error::Protocol("device delta size rejected"));
                        }
                        output = output.saturating_add(text.len());
                        if output > protocol::RESPONSE_LIMIT {
                            return Err(Error::Limit(
                                "device visible response exceeded transport limit",
                            ));
                        }
                        sink(&text)?;
                    }
                    Event::Finished {
                        meta,
                        tool_calls,
                        continuation,
                        ..
                    } => {
                        if let Some(token) = &continuation {
                            protocol::unhex(token)?;
                        }
                        let reply = ModelReply {
                            meta,
                            tool_calls,
                            provider_context: continuation,
                        };
                        protocol::validate_reply(&reply, &specs)?;
                        return Ok(reply);
                    }
                    Event::Failed { code, .. } => return Err(code.error()),
                }
            }
            Err(Error::Transport("personal device connection ended without a completed receipt; inspect, do not resend"))
        })
    }
}
impl DeviceListener {
    pub async fn run(self, stop: CancellationToken) -> Result<()> {
        let uid = aw_client::local::effective_uid()
            .map_err(|_| Error::Denied("Linux identity unavailable"))?;
        loop {
            let (mut stream, _) = tokio::select! {
                biased;
                _ = stop.cancelled() => break,
                result = self.bound.listener.accept() => result?,
            };
            if stream.peer_cred().map_or(true, |p| p.uid() != uid) {
                continue;
            }
            let authenticated = tokio::select! {
                _ = stop.cancelled() => break,
                result = tokio::time::timeout(Duration::from_secs(5),
                    protocol::owner_handshake(&mut stream, &self.key, self.bridge.binding.clone())) => result,
            };
            let Ok(Ok((hello, nonce))) = authenticated else {
                continue;
            };
            let result = self.connected(stream, hello, nonce, &stop).await;
            self.bridge
                .online
                .lock()
                .map_err(|_| Error::Transport("device slot poisoned"))?
                .take();
            // Failures close this authenticated session. A subsequent connection
            // gets a new epoch and a fresh queue, never the former pending call.
            if stop.is_cancelled() {
                break;
            }
            let _ = result; // no sensitive remote errors/prompts in server logs
        }
        self.bridge
            .online
            .lock()
            .map_err(|_| Error::Transport("device slot poisoned"))?
            .take();
        Ok(())
    }
    async fn connected(
        &self,
        stream: UnixStream,
        hello: Hello,
        nonce: String,
        stop: &CancellationToken,
    ) -> Result<()> {
        let (read, write) = stream.into_split();
        let mut sender = protocol::Sender::new(
            write,
            protocol::Authenticator::new(self.key.clone(), &hello, &nonce, true)?,
        );
        let mut receiver = protocol::Receiver::new(
            read,
            protocol::Authenticator::new(self.key.clone(), &hello, &nonce, true)?,
        );
        let (send, mut jobs) = mpsc::channel::<Job>(1);
        *self
            .bridge
            .online
            .lock()
            .map_err(|_| Error::Transport("device slot unavailable"))? = Some(Online {
            epoch: hello.epoch.clone(),
            jobs: send,
        });
        loop {
            let job = tokio::select! {
                biased;
                _ = stop.cancelled() => return Ok(()),
                // A disconnected idle device must disappear before a new job.
                unexpected = receiver.next::<Event>() => {
                    let _ = unexpected;
                    return Err(Error::Transport("device disconnected or sent an unsolicited event"));
                }
                job = jobs.recv() => job.ok_or(Error::Transport("device job queue closed"))?,
            };
            if job.result.is_closed() {
                continue;
            }
            if job.request.epoch != hello.epoch {
                return Err(Error::Protocol("stale device queue epoch"));
            }
            tokio::select! {
                _ = stop.cancelled() => return Ok(()),
                _ = job.result.closed() => return Err(Error::Cancelled),
                result = sender.send(&job.request) => result?,
            }
            let mut expected = 1u64;
            loop {
                let event = tokio::select! {
                    biased;
                    _ = stop.cancelled() => return Ok(()),
                    _ = job.result.closed() => return Err(Error::Cancelled),
                    result = tokio::time::timeout(Duration::from_secs(610), receiver.next::<Event>()) =>
                        result.map_err(|_| Error::Timeout)??.ok_or(Error::Transport("device disconnected during inference"))?,
                };
                let (id, seq, done) = match &event {
                    Event::Delta { id, sequence, .. } => (id, *sequence, false),
                    Event::Finished { id, sequence, .. } | Event::Failed { id, sequence, .. } => {
                        (id, *sequence, true)
                    }
                };
                if id != &job.request.id || seq != expected {
                    return Err(Error::Protocol("wrong invocation event"));
                }
                expected = expected
                    .checked_add(1)
                    .ok_or(Error::Limit("device event sequence exhausted"))?;
                tokio::time::timeout(Duration::from_secs(5), job.result.send(event))
                    .await
                    .map_err(|_| Error::Timeout)?
                    .map_err(|_| Error::Cancelled)?;
                if done {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    #[tokio::test]
    async fn offline_is_not_fallback() {
        let g = grant();
        let b = Bridge {
            binding: g.binding.clone(),
            online: Mutex::new(None),
        };
        let r = request(&g);
        let result = b
            .invoke(
                &r.context,
                &[Message::text(1, aw_agent_core::Role::User, "one")],
                &g.binding.model,
                &[],
                &mut |_| Ok(()),
            )
            .await;
        assert!(result.is_err());
    }
    #[tokio::test]
    async fn no_unscoped_legacy_call() {
        let g = grant();
        let b = Bridge {
            binding: g.binding.clone(),
            online: Mutex::new(None),
        };
        assert!(b.stream(&[], "m", &mut |_| Ok(())).await.is_err());
    }
    #[test]
    fn descriptor_is_personal_not_owner_account() {
        let g = grant();
        let b = Bridge {
            binding: g.binding.clone(),
            online: Mutex::new(None),
        };
        let d = b.descriptor().unwrap();
        assert_eq!(d.inference_executor, "device-a");
        assert!(matches!(d.owner, AccountOwner::User(_)));
    }
    #[test]
    fn wrong_actor_cannot_use_bridge() {
        let g = grant();
        let b = Bridge {
            binding: g.binding.clone(),
            online: Mutex::new(None),
        };
        let mut r = request(&g);
        r.context.identity.actor = PrincipalId::new("someone-else").unwrap();
        assert!(b.validate_identity(&r.context, &g.binding.model).is_err());
    }
}
