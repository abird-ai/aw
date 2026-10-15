//! An independent consumer of human-view services. No model, HTTP or renderer.
//! With no explicit path this example performs no I/O or process launch.
use aw_surfaces::{Command, Options, SurfaceHost};
use std::path::PathBuf;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(root) = std::env::args_os().nth(1) else {
        println!("Supply an explicit directory to read its bounded human file inventory. No service started.");
        return Ok(());
    };
    let root = PathBuf::from(root).canonicalize()?;
    let host = SurfaceHost::new(Options {
        root,
        git: std::env::var_os("AW_GIT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/bin/git")),
        bash: std::env::var_os("AW_BASH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/bin/bash")),
        actor: "trusted-host-example".into(),
        allow_terminals: false,
        sites: vec![],
    })?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &host.execute("trusted-host-example", Command::Capabilities)?
        )?
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&host.execute(
            "trusted-host-example",
            Command::Files {
                path: String::new()
            }
        )?)?
    );
    Ok(())
}
