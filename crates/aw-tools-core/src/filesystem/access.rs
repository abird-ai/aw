//! Feature presets are build-time availability; this is an independent
//! runtime permission ceiling, not a model grant or ToolPolicy replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Off,
    ReadOnly,
    ReadWrite,
    Full,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Read,
    Write,
    Edit,
    Copy,
    Move,
    Remove,
    Shell,
}
impl Access {
    pub fn permits(self, operation: Action) -> bool {
        match (self, operation) {
            (Self::Off, _) => false,
            (Self::ReadOnly, Action::Read) => true,
            (Self::ReadOnly, _) => false,
            (Self::ReadWrite, Action::Shell) => false,
            (Self::ReadWrite, _) => true,
            (Self::Full, _) => true,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_can_only_narrow_tool_visibility() {
        let all = [
            Action::Read,
            Action::Write,
            Action::Edit,
            Action::Copy,
            Action::Move,
            Action::Remove,
            Action::Shell,
        ];
        assert!(all.iter().all(|&op| !Access::Off.permits(op)));
        assert!(Access::ReadOnly.permits(Action::Read));
        assert!(all[1..].iter().all(|&op| !Access::ReadOnly.permits(op)));
        assert!(all[..6].iter().all(|&op| Access::ReadWrite.permits(op)));
        assert!(!Access::ReadWrite.permits(Action::Shell));
        assert!(all.iter().all(|&op| Access::Full.permits(op)));
    }
}
