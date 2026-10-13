//! Local byte/count ceilings; model-specific limits still apply at the provider.
pub const MAX_PROMPT_BYTES: usize = 128 * 1024;
pub const MAX_CONTEXT_BYTES: usize = 1024 * 1024;
pub const MAX_ATTEMPTS: usize = 256;
pub const MAX_OUTPUT_BYTES: usize = 512 * 1024;
pub const MAX_MESSAGES: usize = 1024;
pub const MAX_EXTENSIONS: usize = 32;
