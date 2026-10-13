use aw_terminal::{Options, Terminal};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if !std::env::args().any(|a| a == "--run-fixture") {
        println!("Usage: aw-terminal-example --run-fixture (explicit local PTY smoke)");
        return Ok(());
    }
    let bash = std::env::var_os("AW_BASH")
        .map(std::path::PathBuf::from)
        .or_else(|| option_env!("AW_DEFAULT_BASH").map(std::path::PathBuf::from))
        .unwrap_or_else(|| "/bin/bash".into());
    let mut o = Options::bash(std::env::temp_dir(), bash, "example");
    o.args = vec![
        "--noprofile".into(),
        "--norc".into(),
        "-c".into(),
        "printf 'real-pty-fixture'; sleep .1".into(),
    ];
    let t = Terminal::spawn(o, || {})?;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < until {
        let s = t.snapshot(0);
        let text = s.cells.iter().map(|c| c.text.as_str()).collect::<String>();
        if text.contains("real-pty-fixture") {
            println!("{text}");
            t.terminate();
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Err("PTY fixture timed out".into())
}
