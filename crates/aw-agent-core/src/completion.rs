use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionMeta {
    pub finish_reason: String,
    pub usage: Usage,
}
impl CompletionMeta {
    pub fn complete(&self) -> bool {
        self.finish_reason == "stop"
    }
}
