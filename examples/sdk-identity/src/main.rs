//! Directory naming only: no login, I/O service, LLM or credentials.
use aw_identity::{Participant, Roster};
fn main() -> aw_identity::Result<()> {
    let roster = Roster::new(
        "example-realm",
        vec![
            Participant {
                id: "alice".into(),
                display_name: "Alice".into(),
            },
            Participant {
                id: "bob".into(),
                display_name: "Bob".into(),
            },
        ],
    )?;
    for participant in roster.participants() {
        println!(
            "{} -> {} (naming fixture, NOT an authenticated login)",
            participant.display_name,
            roster.principal(&participant.id)?
        );
    }
    Ok(())
}
