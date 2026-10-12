//! Prints a real layout descriptor and computed rectangles. No agent, network,
//! filesystem browsing or terminal renderer is needed by this SDK consumer.
use aw_workspace::{Action, Navigator, Rect, Side, Target, Workspace};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut workspace = Workspace::default();
    workspace.apply(Action::SetNavigator {
        pane: 1,
        kind: Navigator::Files,
    })?;
    workspace.apply(Action::SplitGroup {
        pane: 2,
        side: Side::Right,
    })?;
    let right = workspace.focused;
    workspace.apply(Action::Open {
        group: right,
        target: Target::Activity,
        pinned: true,
    })?;
    workspace.apply(Action::Focus(2))?;
    println!("{}", serde_json::to_string_pretty(&workspace)?);
    println!(
        "Validated layout cells (not rendered UI): {:?}",
        workspace.geometry(
            Rect {
                x: 0,
                y: 0,
                width: 140,
                height: 38
            },
            false
        )?
    );
    Ok(())
}
