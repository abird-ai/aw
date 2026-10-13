//! A separate embedding application: no aw-cli, provider or state-store dependency.
#[cfg(target_os = "linux")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(path) = std::env::args_os().nth(1) else {
        println!("Pass an explicitly trusted private owner socket to inspect it. No argument opens no connection.");
        return Ok(());
    };
    let mut client = aw_client::Client::connect(std::path::Path::new(&path)).await?;
    println!("{}", serde_json::to_string_pretty(client.hello())?);
    let snapshot = client.snapshot().await?;
    // IDs and count only: an inspection need not print the full private transcript.
    println!(
        "{}",
        serde_json::json!({"instance":snapshot.instance,"revision":snapshot.revision,
        "saved":snapshot.saved,"turn_count":snapshot.turns.len(),"pending":snapshot.pending.as_ref().map(|p|p.run_id)})
    );
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The local owner-client transport currently supports Linux only.");
}
