use aw_git::Repository;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(root) = std::env::args_os().nth(1) else {
        println!("Usage: aw-review-example CHECKOUT (explicit local read only)");
        return Ok(());
    };
    let git = std::env::var_os("AW_GIT")
        .map(std::path::PathBuf::from)
        .or_else(|| option_env!("AW_DEFAULT_GIT").map(std::path::PathBuf::from))
        .unwrap_or_else(|| "/usr/bin/git".into());
    let repo = Repository::discover(git, root)?;
    let status = repo.status()?;
    for change in &status.changes {
        println!("{:?} {}", change.group, change.path.label());
    }
    println!(
        "{} worktrees. No model, terminal, checkout or staging operation performed.",
        repo.worktrees()?.len()
    );
    Ok(())
}
