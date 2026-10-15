//! Explicit local device commands. Parsed before the normal app/owner config or
//! session store. `run` alone loads local provider configuration; no prompt or
//! bridge request supplies an endpoint, environment variable or API key.
use aw::{config, Error, Result};
use aw_agent_core::{CancellationToken, InvocationScope};
use aw_inference::{
    grant::{now, Grant},
    private,
    protocol::{self, Binding},
    DeviceWorker, ReceiptBook,
};
use clap::Subcommand;
use std::{
    collections::BTreeSet,
    io::{self, Write},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Create only a random pairing capability; not a provider API key.
    Key {
        #[arg(long)]
        out: PathBuf,
    },
    /// Deliberately authorize a finite actor/session/model scope on this device.
    Grant {
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        account: String,
        #[arg(long)]
        executor: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        grant_id: String,
        #[arg(long)]
        session: String,
        /// Exact durable branch ID, NOT a mutable display name.
        #[arg(long)]
        branch: String,
        #[arg(long, default_value_t = 1800)]
        ttl: u64,
        #[arg(long, default_value_t = 8)]
        max_calls: u32,
        #[arg(long, default_value_t = 131072)]
        max_context_bytes: usize,
        #[arg(long, default_value_t = 32768)]
        max_output_bytes: usize,
        #[arg(long, default_value_t = 262144)]
        max_total_output_bytes: usize,
        #[arg(long, default_value_t = 4096)]
        max_output_tokens: u32,
        #[arg(long, default_value_t = 120)]
        timeout: u64,
        /// Allow receiving this tool schema/proposal. Does not execute it locally.
        #[arg(long = "allow-tool")]
        allowed_tools: Vec<String>,
        /// Exact SHA-256 displayed by context preparation. Approves that quoted
        /// evidence/focus only, not arbitrary other-branch inference.
        #[arg(long = "allow-summary")]
        allowed_summary_digests: Vec<String>,
    },
    /// Attach outbound to a private socket (possibly a separately created SSH tunnel).
    Run {
        #[arg(long)]
        socket: PathBuf,
        #[arg(long)]
        key_file: PathBuf,
        #[arg(long)]
        grant: PathBuf,
        #[arg(long)]
        receipts: PathBuf,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Inspect local receipt metadata only; never rerun an uncertain call.
    Receipts {
        #[arg(long)]
        file: PathBuf,
    },
}
pub(crate) fn run_command(command: Command) -> Result<u8> {
    match command {
        Command::Key { out } => {
            let mut bytes = [0; 32];
            getrandom::fill(&mut bytes)
                .map_err(|_| Error::Transport("OS randomness unavailable"))?;
            private::create(&out, &bytes)?;
            eprintln!("Created a private 32-byte bridge pairing capability. Transfer this capability privately, NOT a model key. It is never printed.");
        }
        Command::Grant {
            out,
            actor,
            account,
            executor,
            model,
            grant_id,
            session,
            branch,
            ttl,
            max_calls,
            max_context_bytes,
            max_output_bytes,
            max_total_output_bytes,
            max_output_tokens,
            timeout,
            allowed_tools,
            allowed_summary_digests,
        } => {
            let issued_at = now()?;
            let expires_at = issued_at
                .checked_add(ttl)
                .ok_or(Error::Limit("grant time overflow"))?;
            let grant = Grant {
                version: 2,
                allowed_summary_digests: allowed_summary_digests.into_iter().collect(),
                binding: Binding {
                    actor,
                    account_id: account,
                    executor_id: executor,
                    model,
                    grant_id,
                },
                scopes: vec![InvocationScope {
                    session_id: session,
                    branch_id: branch,
                }],
                allowed_tools: allowed_tools.into_iter().collect::<BTreeSet<_>>(),
                issued_at,
                expires_at,
                max_calls,
                max_context_bytes,
                max_output_bytes,
                max_total_output_bytes,
                max_output_tokens,
                timeout_seconds: timeout,
            };
            grant.validate(issued_at)?;
            let bytes = serde_json::to_vec_pretty(&grant)
                .map_err(|_| Error::Config("cannot encode finite grant".into()))?;
            private::create(&out, &bytes)?;
            eprintln!("Created an explicit finite local consent grant. Review it before starting device run. It does not export credentials or grant local tools.");
        }
        Command::Receipts { file } => {
            let ledger = ReceiptBook::inspect(&file)?;
            let output = serde_json::to_vec_pretty(&ledger)
                .map_err(|_| Error::Storage("cannot display receipt metadata"))?;
            io::stdout().write_all(&output)?;
            writeln!(io::stdout())?;
        }
        Command::Run {
            socket,
            key_file,
            grant,
            receipts,
            profile,
            config: config_file,
        } => {
            let (consent, _) = Grant::load(&grant)?;
            let cfg = config::load(config_file.as_deref())?;
            let mut profiles = config::builtins();
            profiles.extend(cfg.profiles);
            let mut configured = profiles
                .remove(&profile)
                .ok_or_else(|| Error::Config("local profile not configured".into()))?;
            if configured
                .model
                .as_ref()
                .is_some_and(|m| m != &consent.binding.model)
            {
                return Err(Error::Denied(
                    "local profile model differs from the consent model",
                ));
            }
            configured.model = Some(consent.binding.model.clone());
            if configured
                .max_tokens
                .is_some_and(|n| n > consent.max_output_tokens)
            {
                return Err(Error::Denied(
                    "local profile token cap exceeds device grant",
                ));
            }
            configured.max_tokens =
                Some(configured.max_tokens.unwrap_or(consent.max_output_tokens));
            configured.validate()?;
            // Hash configured destination/auth-reference, never credential value.
            // A changed config cannot silently reuse an old receipt namespace.
            let profile_digest = protocol::digest(&configured)?;
            let provider = Arc::new(aw::provider::LazyHttpProvider::new(
                configured,
                Duration::from_secs(consent.timeout_seconds),
            )?);
            let key = Arc::new(private::key(&key_file)?);
            let mut worker = DeviceWorker::open(provider, &grant, &receipts, &profile_digest)?;
            eprintln!("Device executor {}: actor {}, account {}, model {}; {} call reservations used of {}. Provider credentials remain local; no local tools are executed.",
                aw::terminal::safe_text(&consent.binding.executor_id), aw::terminal::safe_text(&consent.binding.actor),
                aw::terminal::safe_text(&consent.binding.account_id), aw::terminal::safe_text(&consent.binding.model),
                worker.receipts().summary().receipts.len(), consent.max_calls);
            worker.set_ready_observer(Arc::new(|| {
                eprintln!("aw device authenticated; awaiting locally granted requests.")
            }));
            let stop = CancellationToken::new();
            let signal = stop.clone();
            super::runtime()?.block_on(async {
                let signal_task = tokio::spawn(async move {
                    super::interrupt().await;
                    signal.cancel();
                });
                let result = worker.connect(&socket, key, stop).await;
                signal_task.abort();
                let _ = signal_task.await;
                result
            })?;
        }
    }
    Ok(0)
}
