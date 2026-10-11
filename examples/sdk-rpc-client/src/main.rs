//! An independent external program, no aw-agent-core/Session dependencies.
//! Run with --binary /abs/path/aw --base-url http://127.0.0.1:PORT/v1
//! and --full for the default build, or omit --full for JSONL-minimal.
use aw_rpc_client_example::Client;
use serde_json::json;
use std::{
    env,
    io::{self, BufReader},
    process::{Command, Stdio},
};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut path = None;
    let mut base = None;
    let mut full = false;
    let mut iter = env::args().skip(1);
    while let Some(key) = iter.next() {
        match key.as_str() {
            "--binary" => path = iter.next(),
            "--base-url" => base = iter.next(),
            "--full" => full = true,
            _ => return Err("use --binary PATH --base-url URL [--full]".into()),
        }
    }
    let path = path.ok_or("missing --binary")?;
    let base = base.ok_or("missing --base-url")?;
    let mut cmd = Command::new(path);
    if full {
        cmd.arg("rpc").arg("--agent-stdio");
    }
    cmd.args([
        "--state-memory-only",
        "--provider-profile",
        "llama",
        "--provider-base-url",
        &base,
        "--model-id",
        "mock",
        "--tools-mode",
        "off",
    ]);
    if !full {
        cmd.arg("rpc").arg("--agent-stdio");
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let input = child.stdin.take().ok_or("child stdin missing")?;
    let output = child.stdout.take().ok_or("child stdout missing")?;
    let mut client = Client::connect(BufReader::new(output), input)?;
    let before = client.state()?;
    if before["next_turn"] != 1 {
        return Err("unexpected initial Session state".into());
    }
    let result = client.prompt("independent-rust-client", |f| {
        if f.kind == "delta" {
            eprint!("{}", f.data["text"].as_str().unwrap_or(""));
        }
    })?;
    if !result.accepted || result.terminal.data["result"]["outcome"] != "completed" {
        return Err("model prompt was not settled successfully".into());
    }
    let after = client.state()?;
    if after["next_turn"] != 2 {
        return Err("Session did not advance exactly once".into());
    }
    println!(
        "{}",
        json!({"client":"rust-independent","status":"settled",
                          "next_turn":2,"model":after["model"]})
    );
    drop(client);
    let exit = child.wait()?;
    if !exit.success() {
        return Err("child RPC exited with failure".into());
    }
    io::Write::flush(&mut io::stderr())?;
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("RPC external client: {error}");
        std::process::exit(2);
    }
}
