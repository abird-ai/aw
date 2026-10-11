//! Frontend-independent workspace view model. No agent, credential, network,
//! filesystem, terminal, database or process implementation belongs here.
//! Layout mutations only rearrange view references. They never dispatch work.
pub mod editor {
    pub use aw_editor::*;
}
pub mod layout;
pub use aw_editor::Editor;
pub use layout::*;
