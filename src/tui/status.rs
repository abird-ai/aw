/// Bounded presentation snapshot exposed to built-in UI extensions.
/// It contains labels/status only; no credentials, tool handles or execution authority.
#[derive(Clone, Debug, Default)]
pub struct StatusContext {
    pub model: String,
    pub profile: String,
    pub provider: String,
    pub effort: String,
    pub directory: String,
    pub agent: String,
    pub branch: String,
    pub tools: String,
    pub session: String,
    pub state: String,
    pub session_tps: Option<f64>,
}
