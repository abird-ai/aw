//! Explicitly granted personal-executor inference. The owner gets model output
//! and completed tool proposals, never a provider key/endpoint/env reference.
//!
//! Transport: private Linux Unix sockets, optionally through a separately
//! configured authenticated SSH StreamLocal tunnel. HMAC provides application
//! binding/integrity, not encryption. No TCP listener or browser key vault.
//!
//! This crate depends on the headless core and the bounded IPC primitives, not
//! a concrete provider, tools, database, HTTP server or renderer. A device host
//! supplies its own trusted Provider and finite private grant/receipt paths.
pub mod grant;
#[cfg(target_os = "linux")]
pub mod owner;
pub mod private;
pub mod protocol;
pub mod receipts;
pub mod worker;
pub use aw_agent_core::{Error, Result};
pub use grant::{Grant, LinkConfig};
#[cfg(target_os = "linux")]
pub use owner::Bridge;
pub use receipts::ReceiptBook;
pub use worker::DeviceWorker;

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use aw_agent_core::{
        AccountOwner, BillingOwner, CredentialCustody, InvocationContext, InvocationIdentity,
        InvocationScope, PrincipalId, Role,
    };
    pub fn grant() -> Grant {
        let now = grant::now().unwrap();
        Grant {
            version: 1,
            allowed_summary_digests: Default::default(),
            binding: protocol::Binding {
                actor: "unix:1000".into(),
                account_id: "personal".into(),
                executor_id: "device-a".into(),
                model: "fixture".into(),
                grant_id: "grant-1".into(),
            },
            scopes: vec![InvocationScope {
                session_id: "s_one".into(),
                branch_id: "b_main".into(),
            }],
            allowed_tools: std::collections::BTreeSet::new(),
            issued_at: now,
            expires_at: now + 600,
            max_calls: 8,
            max_context_bytes: 8192,
            max_output_bytes: 4096,
            max_total_output_bytes: 32768,
            max_output_tokens: 128,
            timeout_seconds: 5,
        }
    }
    pub fn request(g: &Grant) -> protocol::Request {
        let actor = PrincipalId::new(g.binding.actor.clone()).unwrap();
        let context = InvocationContext {
            purpose: Default::default(),
            scope: Some(g.scopes[0].clone()),
            turn_id: 1,
            ordinal: 1,
            identity: InvocationIdentity {
                actor: actor.clone(),
                account_id: g.binding.account_id.clone(),
                account_owner: AccountOwner::User(actor.clone()),
                payer: BillingOwner::User(actor),
                credential_custody: CredentialCustody::UserDevice(g.binding.executor_id.clone()),
                credential_version: None,
                inference_executor: g.binding.executor_id.clone(),
                model: g.binding.model.clone(),
                grant_revision: 1,
            },
        };
        protocol::Request {
            version: protocol::VERSION,
            id: protocol::Request::identity(&context).unwrap(),
            epoch: "ab".repeat(32),
            context,
            messages: vec![protocol::WireMessage {
                id: 1,
                role: Role::User,
                content: "fixture question".into(),
                tool_calls: vec![],
                tool_call_id: None,
                name: None,
                continuation: None,
            }],
            tools: vec![],
        }
    }
    pub fn private_dir() -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p =
            std::env::temp_dir().join(format!("aw-device-test-{}", protocol::random_id().unwrap()));
        std::fs::create_dir(&p).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        p
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
