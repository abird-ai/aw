//! Tool-free auxiliary inference for side questions and generated metadata.
use crate::{
    validate_messages, CompletionMeta, Error, InvocationContext, InvocationIdentity,
    InvocationPurpose, JournalFuture, Message, Outcome, ProviderSelection, Result,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{future::Future, time::Duration};

pub const MAX_AUXILIARY_OUTPUT: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuxiliaryKind {
    Side,
    Title,
}

impl AuxiliaryKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Side => "side",
            Self::Title => "title",
        }
    }
}

#[derive(Clone, Debug)]
pub struct AuxiliaryInput {
    pub kind: AuxiliaryKind,
    pub messages: Vec<Message>,
    pub max_output_bytes: usize,
    pub timeout: Duration,
}

impl AuxiliaryInput {
    pub fn validate(&self) -> Result<()> {
        validate_messages(&self.messages)?;
        if self.max_output_bytes == 0 || self.max_output_bytes > MAX_AUXILIARY_OUTPUT {
            return Err(Error::Limit(
                "auxiliary output limit must be 1..65536 bytes",
            ));
        }
        if self.timeout.is_zero() || self.timeout > Duration::from_secs(600) {
            return Err(Error::Limit("auxiliary timeout must be 1..600 seconds"));
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String> {
        let public = self
            .messages
            .iter()
            .map(|message| {
                (
                    message.role,
                    message.content.as_str(),
                    &message.tool_calls,
                    message.tool_call_id.as_deref(),
                    message.name.as_deref(),
                )
            })
            .collect::<Vec<_>>();
        let bytes = serde_json::to_vec(&(
            self.kind,
            public,
            self.max_output_bytes,
            self.timeout.as_millis(),
        ))
        .map_err(|_| Error::Protocol("cannot encode auxiliary input"))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

#[derive(Clone, Debug)]
pub struct AuxiliaryResult {
    pub operation_id: String,
    pub kind: AuxiliaryKind,
    pub outcome: Outcome,
    pub text: String,
    pub meta: Option<CompletionMeta>,
    pub error: Option<String>,
}

pub trait AuxiliaryJournal: Send + Sync {
    fn authorize<'a>(
        &'a self,
        input: &'a AuxiliaryInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, ()>;
    fn begin<'a>(
        &'a self,
        input: &'a AuxiliaryInput,
        identity: &'a InvocationIdentity,
    ) -> JournalFuture<'a, InvocationContext>;
    fn finish<'a>(&'a self, result: &'a AuxiliaryResult) -> JournalFuture<'a, ()>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AuxiliaryExecutor;

impl AuxiliaryExecutor {
    pub async fn execute<C>(
        &self,
        selected: &ProviderSelection,
        input: &AuxiliaryInput,
        journal: &dyn AuxiliaryJournal,
        cancel: C,
    ) -> Result<AuxiliaryResult>
    where
        C: Future<Output = ()> + Send,
    {
        input.validate()?;
        let mut selection_reader = selected.reader()?;
        let mut sink_reader = selected.reader()?;
        selection_reader.revalidate()?;
        let deadline = tokio::time::Instant::now() + input.timeout;
        tokio::pin!(cancel);
        let prepare = async {
            journal.authorize(input, selected.identity()).await?;
            selection_reader.revalidate()?;
            journal.authorize(input, selected.identity()).await?;
            Ok::<(), Error>(())
        };
        tokio::select! {
            biased;
            _ = &mut cancel => return Err(Error::Cancelled),
            _ = tokio::time::sleep_until(deadline) => return Err(Error::Timeout),
            result = prepare => result?,
        }
        // Once begin() is entered, never race/destroy that future: it may be
        // committing provider-dispatch intent. Cancellation applies only after
        // begin has returned a durable invocation context.
        let invocation = journal.begin(input, selected.identity()).await?;
        let digest = input.digest()?;
        let mut text = String::new();
        let mut sink_fault = None;
        let reply = {
            let mut sink = |fragment: &str| -> Result<()> {
                if sink_fault.is_some() {
                    return Err(Error::Protocol("auxiliary stream already rejected"));
                }
                let accepted = sink_reader.revalidate().and_then(|()| {
                    if text.len().saturating_add(fragment.len()) > input.max_output_bytes {
                        return Err(Error::Limit(
                            "auxiliary output exceeded its complete-candidate budget",
                        ));
                    }
                    Ok(())
                });
                if let Err(error) = accepted {
                    sink_fault = Some(error);
                    return Err(Error::Protocol("auxiliary stream callback rejected"));
                }
                text.push_str(fragment);
                Ok(())
            };
            tokio::select! {
                biased;
                _ = &mut cancel => Err(Error::Cancelled),
                _ = tokio::time::sleep_until(deadline) => Err(Error::Timeout),
                reply = async {
                    selection_reader.revalidate()?;
                    journal.authorize(input, selected.identity()).await?;
                    selection_reader.revalidate()?;
                    match &invocation.purpose {
                        InvocationPurpose::Auxiliary {
                            input_sha256,
                            auxiliary_kind,
                            ..
                        } if input_sha256 == &digest
                            && auxiliary_kind == input.kind.label()
                            && &invocation.identity == selected.identity()
                            && invocation.ordinal > 0 => {}
                        _ => {
                            return Err(Error::Protocol(
                                "journal supplied a mismatched auxiliary invocation",
                            ))
                        }
                    }
                    selected.provider().invoke(
                        &invocation,
                        &input.messages,
                        &selected.identity().model,
                        &[],
                        &mut sink,
                    ).await
                } => reply,
            }
        };

        let rejected_meta = reply.as_ref().ok().map(|reply| reply.meta.clone());
        let mut result = if let Some(error) = sink_fault {
            AuxiliaryResult {
                operation_id: operation_id(&invocation)?,
                kind: input.kind,
                outcome: Outcome::Failed,
                text: String::new(),
                meta: rejected_meta,
                error: Some(error.to_string()),
            }
        } else {
            match reply {
                Ok(reply)
                    if reply.meta.complete()
                        && reply.tool_calls.is_empty()
                        && !text.trim().is_empty() =>
                {
                    AuxiliaryResult {
                        operation_id: operation_id(&invocation)?,
                        kind: input.kind,
                        outcome: Outcome::Completed,
                        text,
                        meta: Some(reply.meta),
                        error: None,
                    }
                }
                Ok(reply) => AuxiliaryResult {
                    operation_id: operation_id(&invocation)?,
                    kind: input.kind,
                    outcome: Outcome::Incomplete,
                    text: String::new(),
                    meta: Some(reply.meta),
                    error: Some(
                        "auxiliary inference did not finish as a nonempty tool-free answer".into(),
                    ),
                },
                Err(error) => AuxiliaryResult {
                    operation_id: operation_id(&invocation)?,
                    kind: input.kind,
                    outcome: if matches!(error, Error::Cancelled) {
                        Outcome::Cancelled
                    } else {
                        Outcome::Failed
                    },
                    text: String::new(),
                    meta: None,
                    error: Some(error.to_string()),
                },
            }
        };
        if result.outcome == Outcome::Completed {
            if let Err(error) = selection_reader
                .revalidate()
                .and_then(|()| input.validate())
            {
                result.text.clear();
                result.outcome = Outcome::Failed;
                result.error = Some(error.to_string());
            }
        }
        // Settlement is deliberately not raced with UI cancellation.
        journal.finish(&result).await?;
        Ok(result)
    }
}

fn operation_id(invocation: &InvocationContext) -> Result<String> {
    match &invocation.purpose {
        InvocationPurpose::Auxiliary { operation_id, .. } => Ok(operation_id.clone()),
        _ => Err(Error::Protocol("auxiliary invocation has wrong purpose")),
    }
}
