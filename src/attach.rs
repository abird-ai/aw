//! Native same-user client. Dispatch occurs before loading local config or a
//! SessionStore: connecting never exports a key or starts a competing owner.
use aw::{terminal, Error, Result};
use aw_client::{Client, Command, Packet, PromptInput, Receipt, Snapshot};
use clap::Args as ClapArgs;
use rustyline::{history::MemHistory, Cmd, Editor, KeyCode, KeyEvent, Modifiers};
use serde_json::Value;
use std::{
    io::{self, IsTerminal, Read, Write},
    path::PathBuf,
};

#[derive(ClapArgs, Debug)]
pub(crate) struct Args {
    /// Explicit context command JSON file. Provider credentials remain at their executor.
    #[arg(long)]
    context_action: Option<PathBuf>,
    #[arg(long)]
    socket: PathBuf,
    /// Send one prompt. stdin is read only with --output-print and no --attach-send.
    #[arg(long, conflicts_with_all=["snapshot", "sessions", "tree", "inspect"])]
    send: Option<String>,
    #[arg(short='p', long, conflicts_with_all=["snapshot", "sessions", "tree", "inspect"])]
    print: bool,
    #[arg(long, conflicts_with_all=["sessions", "tree", "inspect"])]
    snapshot: bool,
    #[arg(long, conflicts_with_all=["tree", "inspect"])]
    sessions: bool,
    #[arg(long, conflicts_with = "inspect")]
    tree: bool,
    /// Read this accessible session/branch WITHOUT switching the owner.
    #[arg(long)]
    inspect: Option<String>,
    #[arg(long, requires = "inspect")]
    branch: Option<String>,
    /// Opaque permitted account ID; never a local config/profile or credential.
    #[arg(long, requires = "model")]
    account: Option<String>,
    #[arg(long, requires = "account")]
    model: Option<String>,
    /// Semantic request identity; only valid with an explicit one-shot send.
    #[arg(long, requires = "send")]
    request_id: Option<String>,
}
fn convert(error: aw_client::Error) -> Error {
    match error {
        aw_client::Error::Io(e) => Error::Io(e),
        other => Error::Config(other.to_string()),
    }
}
fn json(value: &impl serde::Serialize) -> Result<()> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|_| Error::Protocol("cannot render owner reply"))?;
    let shown = if io::stdout().is_terminal() {
        terminal::safe_text(&text)
    } else {
        text
    };
    writeln!(io::stdout(), "{shown}")?;
    Ok(())
}
fn client_io(error: io::Error) -> aw_client::Error {
    aw_client::Error::Io(error)
}
fn append_output(emitted: &mut String, complete: &str) -> aw_client::Result<()> {
    if complete == emitted {
        return Ok(());
    }
    if let Some(delta) = complete.strip_prefix(emitted.as_str()) {
        write!(io::stdout(), "{}", terminal::safe_text(delta)).map_err(client_io)?;
    } else {
        eprintln!(
            "\n[owner snapshot replaced an incomplete local stream; displaying current text]"
        );
        write!(io::stdout(), "{}", terminal::safe_text(complete)).map_err(client_io)?;
    }
    io::stdout().flush().map_err(client_io)?;
    *emitted = complete.to_owned();
    Ok(())
}
fn target(snapshot: &Snapshot) -> Option<(&str, &str)> {
    snapshot
        .saved
        .as_ref()
        .map(|s| (s.session_id.as_str(), s.branch_id.as_str()))
}
fn input_target(p: &PromptInput) -> Option<(&str, &str)> {
    p.session_id.as_deref().zip(p.branch_id.as_deref())
}
struct Attached {
    socket: PathBuf,
    client: Client,
    view: Snapshot,
    selected: Option<(String, String)>,
    /// Retain exact input after an ambiguous call; /recover is lookup-only.
    unresolved: Option<PromptInput>,
}
impl Attached {
    async fn connect(
        socket: PathBuf,
        selected: Option<(String, String)>,
    ) -> aw_client::Result<Self> {
        let mut client = Client::connect(&socket).await?;
        let view = client.snapshot().await?;
        Ok(Self {
            socket,
            client,
            view,
            selected,
            unresolved: None,
        })
    }
    fn describe(&self) {
        eprintln!(
            "aw attached · {} · owner {}",
            terminal::safe_text(&self.client.hello().principal),
            terminal::safe_text(&self.socket.display().to_string())
        );
        if let Some(s) = &self.view.saved {
            eprintln!(
                "session {} / {} · revision {}",
                terminal::safe_text(&s.session_id),
                terminal::safe_text(&s.branch_name),
                self.view.revision
            );
        } else {
            eprintln!("memory-only owner · revision {}", self.view.revision);
        }
        eprintln!("Inference and tool effects use this OWNER's current grants. Client provider config and secrets are not loaded.");
        if let Some(p) = &self.view.pending {
            eprintln!(
                "Run {} is active; /follow observes, /cancel explicitly requests cancellation.",
                p.run_id
            );
        }
        for hint in &self.view.recovery_hints {
            let kind = hint
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("recovery");
            let message = hint
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("durable recovery requires attention");
            eprintln!(
                "recovery · {} · {}",
                terminal::safe_text(kind),
                terminal::safe_text(message)
            );
        }
    }
    async fn refresh(&mut self) -> aw_client::Result<()> {
        let next = self.client.snapshot().await?;
        if target(&next) != target(&self.view) {
            eprintln!("Owner selected a different session/branch. Review the target before sending any preserved draft.");
        }
        self.view = next;
        self.describe();
        Ok(())
    }
    async fn reconnect(&mut self) -> aw_client::Result<()> {
        // No transport retry or command replay. Replacing a client just detaches.
        let mut client = Client::connect(&self.socket).await?;
        let next = client.snapshot().await?;
        if client.hello().instance != self.client.hello().instance {
            eprintln!(
                "Owner instance changed. A missing receipt does not prove old work never ran."
            );
        }
        self.client = client;
        self.view = next;
        self.describe();
        Ok(())
    }
    async fn send(&mut self, text: String, id: Option<String>) -> aw_client::Result<u8> {
        self.send_with_retry(text, id, None).await
    }
    async fn send_with_retry(
        &mut self,
        text: String,
        id: Option<String>,
        retry_operation_id: Option<String>,
    ) -> aw_client::Result<u8> {
        if self.unresolved.is_some() {
            return Err(aw_client::Error::Protocol(
                "an earlier request is unresolved; /recover or explicitly /discard-request first",
            ));
        }
        let mut input = self
            .view
            .prepare(
                id.map(Ok).unwrap_or_else(aw_client::request_id)?,
                text,
                self.selected
                    .as_ref()
                    .map(|(a, m)| (a.as_str(), m.as_str())),
            )
            .map_err(aw_client::Error::Protocol)?;
        input.retry_operation_id = retry_operation_id;
        eprintln!(
            "request {} · account {} · model {}",
            input.request_id,
            terminal::safe_text(&input.account_id),
            terminal::safe_text(&input.model)
        );
        self.unresolved = Some(input.clone());
        let receipt: Receipt = self.client.call(Command::Prompt(input.clone())).await?;
        eprintln!(
            "accepted receipt {} ({})",
            receipt.run_id,
            terminal::safe_text(&receipt.state)
        );
        let mut emitted = String::new();
        if receipt.state == "accepted" {
            self.observe(receipt.run_id, &mut emitted).await?;
        }
        self.finish(input, &mut emitted).await
    }
    async fn observe(&mut self, run_id: u64, emitted: &mut String) -> aw_client::Result<()> {
        self.client.start_follow(run_id).await?;
        let mut interrupts = 0;
        loop {
            let next = tokio::select! {
                event=self.client.next_event()=>Some(event),
                signal=tokio::signal::ctrl_c()=>{
                    if signal.is_err(){return Err(aw_client::Error::Protocol("cannot install local interrupt handler"));}
                    interrupts+=1;
                    if interrupts>1{return Err(aw_client::Error::Protocol("detached while cancellation/settlement may still be pending; use /reconnect then /recover"));}
                    let mut control=Client::connect(&self.socket).await?;
                    if control.hello().instance!=self.client.hello().instance{return Err(aw_client::Error::Protocol("owner changed; cancellation target not forwarded to a different owner"));}
                    let _:Value=control.call(Command::Cancel{run_id}).await?;
                    eprintln!("\nCancellation requested for run {run_id}; this cannot undo remote billing or tool effects.");
                    None
                }
            };
            let Some(next) = next else { continue };
            match next?.ok_or(aw_client::Error::Closed)? {
                Packet::Event { event, data, .. } if event == "snapshot" => {
                    let snapshot: Snapshot = serde_json::from_value(data)
                        .map_err(|_| aw_client::Error::Protocol("invalid owner snapshot"))?;
                    if let Some(p) = snapshot.pending.as_ref().filter(|p| p.run_id == run_id) {
                        append_output(emitted, &p.answer)?;
                    }
                    self.view = snapshot;
                }
                Packet::Event { event, data, .. } if event == "delta" => {
                    if data["run_id"].as_u64() != Some(run_id) {
                        return Err(aw_client::Error::Protocol("unexpected stream run"));
                    }
                    let text = data["text"]
                        .as_str()
                        .ok_or(aw_client::Error::Protocol("invalid delta"))?;
                    if emitted.len().saturating_add(text.len()) > 4 * 1024 * 1024 {
                        return Err(aw_client::Error::Protocol(
                            "local displayed stream exceeds limit",
                        ));
                    }
                    write!(io::stdout(), "{}", terminal::safe_text(text)).map_err(client_io)?;
                    io::stdout().flush().map_err(client_io)?;
                    emitted.push_str(text);
                }
                Packet::Event { .. } => {}
                Packet::End { reason } => {
                    eprintln!("\n[{}]", terminal::safe_text(&reason));
                    return Ok(());
                }
                _ => return Err(aw_client::Error::Protocol("unexpected observation packet")),
            }
        }
    }
    async fn finish(&mut self, input: PromptInput, emitted: &mut String) -> aw_client::Result<u8> {
        let status: Value = self
            .client
            .call(Command::PromptStatus {
                original: input.clone(),
            })
            .await?;
        if status["found"].as_bool() != Some(true) {
            eprintln!("No matching receipt at this owner. This is not proof that an earlier owner never executed it; inspect before a deliberate new request.");
            return Ok(4);
        }
        if let Some(answer) = status["turn"]["answer"].as_str() {
            append_output(emitted, answer)?;
        }
        let receipt_state = status["receipt"]["state"].as_str().unwrap_or("unknown");
        if receipt_state == "accepted" {
            eprintln!("The exact request remains accepted/active. Use /recover; it has not been resubmitted.");
            return Ok(4);
        }
        let completion = &status["completion"];
        // Memory owners retain small outcome receipts across reset. Only use the
        // current answer when its view revision still identifies this settlement.
        if status["durable"].as_bool() == Some(false)
            && target(&self.view) == input_target(&input)
            && self.view.revision
                == status["receipt"]["revision"]
                    .as_u64()
                    .unwrap_or(u64::MAX)
                    .saturating_add(1)
        {
            if let Some(id) = completion["turn_id"].as_u64() {
                if let Some(answer) = self
                    .view
                    .turns
                    .iter()
                    .find(|t| t["id"].as_u64() == Some(id))
                    .and_then(|t| t["answer"].as_str())
                {
                    append_output(emitted, answer)?;
                }
            }
        }
        let outcome = completion["outcome"].as_str().unwrap_or("unknown");
        eprintln!(
            "\nrequest {}: receipt {receipt_state}, outcome {outcome}",
            input.request_id
        );
        if let Some(error) = completion["error"].as_str() {
            eprintln!("{}", terminal::safe_text(error));
        }
        self.unresolved = None;
        // No local preference, grant or execution directory is restored from history.
        self.refresh().await?;
        Ok(match outcome {
            "completed" => 0,
            "cancelled" => 130,
            "incomplete" => 3,
            "failed" => 1,
            _ => 4,
        })
    }
    async fn retry_hint(&mut self, operation: &str) -> aw_client::Result<u8> {
        let hint = self
            .view
            .recovery_hints
            .iter()
            .find(|h| {
                h.get("operation_id").and_then(Value::as_str) == Some(operation)
                    && h.get("kind").and_then(Value::as_str) == Some("retry_available")
            })
            .cloned()
            .ok_or(aw_client::Error::Protocol(
                "retry operation is not offered by the current durable snapshot; /refresh",
            ))?;
        let actions = hint.get("actions").ok_or(aw_client::Error::Protocol(
            "recovery hint is missing action metadata",
        ))?;
        let prompt = actions
            .get("prompt")
            .and_then(Value::as_str)
            .ok_or(aw_client::Error::Protocol(
                "recovery hint is missing the original prompt",
            ))?
            .to_owned();
        let origin_head = actions
            .get("origin_head")
            .and_then(Value::as_str)
            .ok_or(aw_client::Error::Protocol(
                "recovery hint is missing the original head",
            ))?
            .to_owned();
        let current = self.view.saved.as_ref().ok_or(aw_client::Error::Protocol(
            "durable retry requires a saved session",
        ))?;
        if current.head != origin_head {
            let suffix = operation
                .chars()
                .rev()
                .take(8)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>();
            self.manage(Command::Fork {
                expected_revision: self.view.revision,
                name: format!("retry-{suffix}"),
                at_node: Some(origin_head),
            })
            .await?;
        }
        self.send_with_retry(prompt, None, Some(operation.to_owned()))
            .await
    }
    async fn recover(&mut self) -> aw_client::Result<()> {
        let input = self
            .unresolved
            .clone()
            .ok_or(aw_client::Error::Protocol("no unresolved local request"))?;
        if !self.client.is_connected() {
            self.reconnect().await?;
        }
        let status: Value = self
            .client
            .call(Command::PromptStatus {
                original: input.clone(),
            })
            .await?;
        if status["found"].as_bool() != Some(true) {
            eprintln!("No receipt found. Nothing was resubmitted. Inspect the original owner/state before discarding recovery.");
            return Ok(());
        }
        let mut emitted = String::new();
        if status["receipt"]["state"].as_str() == Some("accepted") {
            let id = status["receipt"]["run_id"]
                .as_u64()
                .ok_or(aw_client::Error::Protocol("invalid recovered receipt"))?;
            self.observe(id, &mut emitted).await?;
        }
        self.finish(input, &mut emitted).await?;
        Ok(())
    }
    async fn manage(&mut self, command: Command) -> aw_client::Result<()> {
        if self.unresolved.is_some() {
            return Err(aw_client::Error::Protocol("resolve or explicitly discard prior request recovery before changing the shared owner view"));
        }
        self.view = self.client.call(command).await?;
        self.describe();
        Ok(())
    }
}
const HELP:&str="/context-flow {JSON}: explicit prepare/generate/apply; /summary-cancel REQUEST_ID\n/help /quit /status /refresh /reconnect /history /accounts\n/use ACCOUNT MODEL | /use default (next prompt only, owner-registered targets)\n/sessions /branches /tree /usage /inspect SESSION [BRANCH] (read only)\n/open SESSION [BRANCH] /new /fork NAME [NODE] /checkpoint TEXT (change shared active view)\n/follow /cancel /acknowledge inspected\n/retry OPERATION retries an offered durable attempt from its exact original head.\n/recover looks up the exact unresolved request; never submits it again.\n/discard-request abandons LOCAL recovery metadata only; not effect acknowledgement.\nEnter sends; Alt+Enter newline; Ctrl-C requests cancellation during a stream; Ctrl-D detaches.\nNo local credentials/config/store are loaded by attach. No remote fullscreen client yet.";

pub(crate) fn run_args(args: Args) -> Result<u8> {
    let rt = crate::runtime()?;
    let selected = args.account.zip(args.model);
    let mut host = rt
        .block_on(Attached::connect(args.socket, selected))
        .map_err(convert)?;
    host.describe();
    if let Some(path) = args.context_action {
        let value: Value = serde_json::from_slice(&crate::read_bounded(&path, 256 * 1024)?)
            .map_err(|_| Error::Config("invalid context command JSON".into()))?;
        json(
            &rt.block_on(host.client.call::<Value>(Command::Context {
                expected_revision: host.view.revision,
                command: value,
            }))
            .map_err(convert)?,
        )?;
        return Ok(0);
    }
    if args.snapshot {
        json(&host.view)?;
        return Ok(0);
    }
    if args.sessions {
        json(
            &rt.block_on(host.client.call::<Value>(Command::Sessions))
                .map_err(convert)?,
        )?;
        return Ok(0);
    }
    if args.tree {
        json(
            &rt.block_on(host.client.call::<Value>(Command::Tree))
                .map_err(convert)?,
        )?;
        return Ok(0);
    }
    if let Some(session_id) = args.inspect {
        json(
            &rt.block_on(host.client.call::<Value>(Command::Inspect {
                session_id,
                branch: args.branch.unwrap_or_else(|| "main".into()),
            }))
            .map_err(convert)?,
        )?;
        return Ok(0);
    }
    let mut send = args.send;
    if args.print && send.is_none() {
        if io::stdin().is_terminal() {
            return Err(Error::Config(
                "attach --output-print requires --attach-send or piped stdin".into(),
            ));
        }
        let mut bytes = Vec::new();
        io::stdin()
            .take((aw_client::protocol::MAX_PROMPT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > aw_client::protocol::MAX_PROMPT_BYTES {
            return Err(Error::Limit("prompt exceeds 128 KiB"));
        }
        send = Some(
            String::from_utf8(bytes).map_err(|_| Error::Config("stdin must be UTF-8".into()))?,
        );
    }
    if let Some(text) = send {
        return rt
            .block_on(host.send(text, args.request_id))
            .map_err(convert);
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::Config(
            "attach requires a terminal, --attach-snapshot, or --output-print with input".into(),
        ));
    }
    let config = rustyline::Config::builder()
        .max_history_size(128)
        .map_err(|_| Error::Config("editor configuration failed".into()))?
        .auto_add_history(false)
        .build();
    let mut editor = Editor::<(), MemHistory>::with_config(config)
        .map_err(|_| Error::Config("cannot initialize attachment editor".into()))?;
    editor.bind_sequence(KeyEvent(KeyCode::Enter, Modifiers::ALT), Cmd::Newline);
    eprintln!("{HELP}");
    loop {
        let line = match editor.readline("attached> ") {
            Ok(line) => line,
            Err(rustyline::error::ReadlineError::Interrupted) => continue,
            Err(rustyline::error::ReadlineError::Eof) => break,
            Err(_) => return Err(Error::Config("attachment terminal input failed".into())),
        };
        if line.trim().is_empty() {
            continue;
        }
        if !line.contains('\n') && line.starts_with('/') && !line.starts_with("//") {
            let (verb, rest) = line.split_once(' ').unwrap_or((&line, ""));
            let words = rest.split_whitespace().collect::<Vec<_>>();
            let result: Result<()> = match verb {
                "/quit" | "/exit" => break,
                "/help" => {
                    eprintln!("{HELP}");
                    Ok(())
                }
                "/context-flow" => match serde_json::from_str::<Value>(rest) {
                    Ok(command) => rt
                        .block_on(host.client.call::<Value>(Command::Context {
                            expected_revision: host.view.revision,
                            command,
                        }))
                        .map_err(convert)
                        .and_then(|v| json(&v)),
                    Err(_) => Err(Error::Config("/context-flow {typed JSON}".into())),
                },
                "/summary-cancel" => rt
                    .block_on(host.client.call::<Value>(Command::ContextCancel {
                        request_id: rest.trim().into(),
                    }))
                    .map_err(convert)
                    .and_then(|v| json(&v)),
                "/status" => json(&host.view),
                "/history" => json(&host.view.turns),
                "/refresh" => rt.block_on(host.refresh()).map_err(convert),
                "/reconnect" => rt.block_on(host.reconnect()).map_err(convert),
                "/accounts" => rt
                    .block_on(host.client.call::<Value>(Command::Accounts))
                    .map_err(convert)
                    .and_then(|v| json(&v)),
                "/sessions" | "/branches" | "/tree" | "/usage" => {
                    let command = match verb {
                        "/sessions" => Command::Sessions,
                        "/branches" => Command::Branches,
                        "/tree" => Command::Tree,
                        _ => Command::Usage,
                    };
                    rt.block_on(host.client.call::<Value>(command))
                        .map_err(convert)
                        .and_then(|v| json(&v))
                }
                "/inspect" if (1..=2).contains(&words.len()) => rt
                    .block_on(host.client.call::<Value>(Command::Inspect {
                        session_id: words[0].into(),
                        branch: words.get(1).unwrap_or(&"main").to_string(),
                    }))
                    .map_err(convert)
                    .and_then(|v| json(&v)),
                "/open" if (1..=2).contains(&words.len()) => rt
                    .block_on(host.manage(Command::Open {
                        expected_revision: host.view.revision,
                        session_id: words[0].into(),
                        branch: words.get(1).unwrap_or(&"main").to_string(),
                    }))
                    .map_err(convert),
                "/new" => rt
                    .block_on(host.manage(Command::Reset {
                        expected_revision: host.view.revision,
                    }))
                    .map_err(convert),
                "/fork" if (1..=2).contains(&words.len()) => rt
                    .block_on(host.manage(Command::Fork {
                        expected_revision: host.view.revision,
                        name: words[0].into(),
                        at_node: words.get(1).map(|s| s.to_string()),
                    }))
                    .map_err(convert),
                "/checkpoint" if !rest.trim().is_empty() => rt
                    .block_on(host.manage(Command::Checkpoint {
                        expected_revision: host.view.revision,
                        summary: rest.into(),
                    }))
                    .map_err(convert),
                "/acknowledge" if rest.trim() == "inspected" => rt
                    .block_on(host.manage(Command::Acknowledge {
                        expected_revision: host.view.revision,
                        confirmed_inspected: true,
                    }))
                    .map_err(convert),
                "/use" if rest.trim() == "default" => {
                    host.selected = None;
                    Ok(())
                }
                "/use" if words.len() == 2 => {
                    if host
                        .view
                        .accounts
                        .iter()
                        .any(|a| a.id == words[0] && a.allowed_models.iter().any(|m| m == words[1]))
                    {
                        host.selected = Some((words[0].into(), words[1].into()));
                        Ok(())
                    } else {
                        Err(Error::Denied(
                            "account/model not in the last permitted snapshot; refresh and inspect",
                        ))
                    }
                }
                "/retry" if words.len() == 1 => rt
                    .block_on(host.retry_hint(words[0]))
                    .map(|_| ())
                    .map_err(convert),
                "/recover" => rt.block_on(host.recover()).map_err(convert),
                "/discard-request" => {
                    host.unresolved = None;
                    eprintln!("Local request recovery discarded; no owner effects, hazards or receipts changed.");
                    Ok(())
                }
                "/cancel" => {
                    if let Some(p) = &host.view.pending {
                        rt.block_on(
                            host.client
                                .call::<Value>(Command::Cancel { run_id: p.run_id }),
                        )
                        .map_err(convert)
                        .and_then(|v| json(&v))
                    } else {
                        Err(Error::Config(
                            "no pending run in the viewed snapshot; /refresh".into(),
                        ))
                    }
                }
                "/follow" => {
                    if let Some(p) = &host.view.pending {
                        let id = p.run_id;
                        rt.block_on(host.observe(id, &mut String::new()))
                            .map_err(convert)
                    } else {
                        Err(Error::Config(
                            "no pending run in the viewed snapshot; /refresh".into(),
                        ))
                    }
                }
                _ => Err(Error::Config("unknown/incomplete command; /help".into())),
            };
            if let Err(e) = result {
                eprintln!("aw attach: {}", terminal::safe_text(&e.to_string()));
            }
            continue;
        }
        let prompt = if line.starts_with("//") {
            line[1..].to_owned()
        } else {
            line.clone()
        };
        let _ = editor.add_history_entry(line.as_str());
        if let Err(e) = rt.block_on(host.send(prompt, None)) {
            eprintln!("\naw attach: {}", terminal::safe_text(&e.to_string()));
            if host.unresolved.is_some() {
                eprintln!("Exact input remains in memory. /recover is lookup-only; /reconnect never resubmits. /discard-request only abandons the local recovery record.");
            }
        }
    }
    if host.unresolved.is_some() {
        eprintln!("Unresolved local request metadata is being discarded on client exit. Owner work may still be running; inspect its receipts/history before a new request.");
    }
    eprintln!("Detached. The owner, its agent and existing terminals were not stopped.");
    Ok(0)
}
