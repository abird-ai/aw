//! Versioned application commands, not arbitrary HTTP or provider requests.
//! Transport sequence IDs are NOT semantic idempotency keys.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const VERSION: u16 = 1;
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PROMPT_BYTES: usize = 128 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u16,
    pub id: u64,
    pub expected_instance: Option<String>,
    pub command: Command,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Hello,
    Snapshot,
    Context {
        expected_revision: u64,
        command: Value,
    },
    ContextCancel {
        request_id: String,
    },
    Accounts,
    Sessions,
    Branches,
    Tree,
    Usage,
    /// Inspect another authorized ancestry without moving the active owner.
    Inspect {
        session_id: String,
        branch: String,
    },
    Prompt(PromptInput),
    /// Lookup only. A missing receipt never starts a call.
    PromptStatus {
        original: PromptInput,
    },
    Cancel {
        run_id: u64,
    },
    Reset {
        expected_revision: u64,
    },
    Open {
        expected_revision: u64,
        session_id: String,
        branch: String,
    },
    Fork {
        expected_revision: u64,
        name: String,
        at_node: Option<String>,
    },
    Checkpoint {
        expected_revision: u64,
        summary: String,
    },
    Acknowledge {
        expected_revision: u64,
        confirmed_inspected: bool,
    },
    /// Borrow this connection for one run's observation; End restores commands.
    Follow {
        run_id: u64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PromptInput {
    pub request_id: String,
    pub expected_revision: u64,
    pub prompt: String,
    /// Both are explicit: recovery must not resolve changed owner defaults.
    pub account_id: String,
    pub model: String,
    pub session_id: Option<String>,
    pub branch_id: Option<String>,
    #[serde(default)]
    pub retry_operation_id: Option<String>,
}

impl PromptInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_id.is_empty()
            || self.request_id.len() > 64
            || !self
                .request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err("invalid semantic request ID");
        }
        if self.prompt.trim().is_empty() || self.prompt.len() > MAX_PROMPT_BYTES {
            return Err("prompt outside byte limit");
        }
        label(&self.account_id, 128)?;
        label(&self.model, 256)?;
        if self.session_id.is_some() != self.branch_id.is_some() {
            return Err("saved session and branch must be selected together");
        }
        for text in [&self.session_id, &self.branch_id, &self.retry_operation_id]
            .into_iter()
            .flatten()
        {
            label(text, 256)?;
        }
        Ok(())
    }
}

fn label(text: &str, max: usize) -> Result<(), &'static str> {
    if text.is_empty() || text.len() > max || text.chars().any(char::is_control) {
        Err("invalid bounded label")
    } else {
        Ok(())
    }
}

impl Command {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Context { command, .. } => {
                if !command.is_object()
                    || serde_json::to_vec(command).map_or(true, |v| v.len() > 256 * 1024)
                {
                    Err("context envelope outside bounds")
                } else {
                    Ok(())
                }
            }
            Self::ContextCancel { request_id } => label(request_id, 128),
            Self::Prompt(p) | Self::PromptStatus { original: p } => p.validate(),
            Self::Inspect { session_id, branch }
            | Self::Open {
                session_id, branch, ..
            } => {
                label(session_id, 256)?;
                label(branch, 256)
            }
            Self::Fork { name, at_node, .. } => {
                label(name, 256)?;
                if let Some(node) = at_node {
                    label(node, 256)?;
                }
                Ok(())
            }
            Self::Checkpoint { summary, .. } => {
                if summary.trim().is_empty() || summary.len() > MAX_PROMPT_BYTES {
                    Err("checkpoint outside byte limit")
                } else {
                    Ok(())
                }
            }
            Self::Cancel { run_id } | Self::Follow { run_id } if *run_id == 0 => {
                Err("run ID is zero")
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u16,
    pub id: u64,
    pub packet: Packet,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Packet {
    Result {
        value: Value,
    },
    Error {
        code: u16,
        message: String,
    },
    Event {
        sequence: u64,
        event: String,
        data: Value,
    },
    End {
        reason: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u16,
    pub instance: String,
    pub principal: String,
    pub owner_uid: u32,
    pub package_version: String,
    pub capabilities: Vec<String>,
    pub shared_active_view: bool,
    pub inference_location: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub run_id: u64,
    pub revision: u64,
    pub state: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedSelection {
    pub session_id: String,
    pub branch_id: String,
    pub branch_name: String,
    pub head: String,
    pub branch_revision: u64,
    pub title: String,
    pub imported: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountChoice {
    pub id: String,
    pub allowed_models: Vec<String>,
    pub account_kind: String,
    pub payer: Value,
    pub credential_custody: Value,
    pub inference_executor: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pending {
    pub run_id: u64,
    pub prompt: String,
    pub answer: String,
    #[serde(default)]
    pub tool_records: Vec<Value>,
    pub identity: Option<Value>,
}
/// History records are preserved as version-matched JSON, without requiring an
/// embedding reader to link the execution engine, tool I/O or SQLite.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub instance: String,
    pub sequence: u64,
    pub revision: u64,
    pub model: String,
    pub profile: String,
    pub turns: Vec<Value>,
    pub pending: Option<Pending>,
    pub last_error: Option<String>,
    pub identity: Option<Value>,
    #[serde(default)]
    pub accounts: Vec<AccountChoice>,
    #[serde(default)]
    pub tools: Vec<String>,
    pub tool_host: String,
    pub needs_reconciliation: bool,
    pub saved: Option<SavedSelection>,
    pub context_checkpoint: bool,
    #[serde(default)]
    pub context_blocks: Vec<Value>,
    #[serde(default)]
    pub recovery_hints: Vec<Value>,
}
impl Snapshot {
    pub fn prepare(
        &self,
        request_id: String,
        prompt: String,
        account: Option<(&str, &str)>,
    ) -> Result<PromptInput, &'static str> {
        let (account_id, model) = match account {
            Some((id, model)) => (id.to_owned(), model.to_owned()),
            None => {
                let identity = self
                    .identity
                    .as_ref()
                    .ok_or("default account unavailable")?;
                let id = identity
                    .get("account_id")
                    .and_then(Value::as_str)
                    .ok_or("account identity unavailable")?;
                let model = identity
                    .get("model")
                    .and_then(Value::as_str)
                    .ok_or("model identity unavailable")?;
                (id.to_owned(), model.to_owned())
            }
        };
        if self.pending.is_some() || self.needs_reconciliation {
            return Err("owner busy or awaiting inspection");
        }
        let command = PromptInput {
            request_id,
            expected_revision: self.revision,
            prompt,
            account_id,
            model,
            session_id: self.saved.as_ref().map(|s| s.session_id.clone()),
            branch_id: self.saved.as_ref().map(|s| s.branch_id.clone()),
            retry_operation_id: None,
        };
        command.validate()?;
        Ok(command)
    }
}
