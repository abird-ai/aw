//! Versioned bounded inference envelopes and role-separated authenticated frames.
//! Request data deliberately has no endpoint, key, env, file root or command field.
use crate::{Error, Result};
use aw_agent_core::{
    CompletionMeta, InvocationContext, Message, ModelReply, Role, ToolCall, ToolSpec,
};
use hmac::{Hmac, Mac};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite};

pub const VERSION: u32 = 2;
pub const PAYLOAD_LIMIT: usize = 2 * 1024 * 1024;
pub const FRAME_LIMIT: usize = 8 * 1024 * 1024;
pub const DELTA_LIMIT: usize = 16 * 1024;
pub const RESPONSE_LIMIT: usize = 512 * 1024;
pub const RECEIPT_LIMIT: usize = 64;
pub const HANDSHAKE_LIMIT: usize = 4096;

pub fn label(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b":._-@".contains(&b))
    {
        return Err(Error::Protocol("invalid bounded bridge identifier"));
    }
    Ok(())
}
pub fn model_label(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(Error::Protocol("invalid bridge model"));
    }
    Ok(())
}
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn unhex(value: &str) -> Result<Vec<u8>> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Protocol("expected canonical 32-byte hex"));
    }
    (0..64)
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&value[i..i + 2], 16).map_err(|_| Error::Protocol("invalid hex"))
        })
        .collect()
}
pub fn random_id() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::Transport("OS randomness unavailable"))?;
    Ok(hex(&bytes))
}
pub fn digest<T: Serialize>(value: &T) -> Result<String> {
    Ok(hex(&Sha256::digest(serde_json::to_vec(value).map_err(
        |_| Error::Protocol("cannot encode inference data"),
    )?)))
}
fn ipc(_: aw_client::Error) -> Error {
    Error::Transport("device channel closed, invalid or unavailable; outcome may be unknown")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub actor: String,
    pub account_id: String,
    pub executor_id: String,
    pub model: String,
    pub grant_id: String,
}
impl Binding {
    pub fn validate(&self) -> Result<()> {
        for value in [
            &self.actor,
            &self.account_id,
            &self.executor_id,
            &self.grant_id,
        ] {
            label(value)?;
        }
        model_label(&self.model)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub version: u32,
    pub epoch: String,
    pub challenge: String,
    pub binding: Binding,
}
impl Hello {
    pub fn new(binding: Binding) -> Result<Self> {
        binding.validate()?;
        Ok(Self {
            version: VERSION,
            epoch: random_id()?,
            challenge: random_id()?,
            binding,
        })
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != VERSION {
            return Err(Error::Protocol("unsupported device protocol"));
        }
        unhex(&self.epoch)?;
        unhex(&self.challenge)?;
        self.binding.validate()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proof {
    pub version: u32,
    pub nonce: String,
    pub value: String,
}
/// A shared pairing capability, NOT a provider secret. Intentionally no Debug,
/// Serialize, accessor, or secret-export endpoint. OS process-memory trust remains.
pub struct PairingKey([u8; 32]);
impl PairingKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    fn mac(&self, fields: &[&[u8]]) -> Result<Hmac<Sha256>> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0)
            .map_err(|_| Error::Protocol("invalid authentication key"))?;
        // Explicit length framing avoids concatenation ambiguity.
        for field in fields {
            mac.update(&(field.len() as u64).to_be_bytes());
            mac.update(field);
        }
        Ok(mac)
    }
    pub fn proof(&self, hello: &Hello, nonce: &str, role: &str) -> Result<Proof> {
        hello.validate()?;
        unhex(nonce)?;
        if !matches!(role, "worker" | "owner") {
            return Err(Error::Protocol("invalid authentication role"));
        }
        let context = serde_json::to_vec(hello).map_err(|_| Error::Protocol("invalid hello"))?;
        let mac = self.mac(&[
            b"aw-inference/auth/v2",
            role.as_bytes(),
            &context,
            nonce.as_bytes(),
        ])?;
        Ok(Proof {
            version: VERSION,
            nonce: nonce.into(),
            value: hex(&mac.finalize().into_bytes()),
        })
    }
    pub fn verify(&self, hello: &Hello, nonce: &str, role: &str, proof: &Proof) -> Result<()> {
        hello.validate()?;
        unhex(nonce)?;
        if proof.version != VERSION || proof.nonce != nonce || !matches!(role, "worker" | "owner") {
            return Err(Error::Denied("device authentication failed"));
        }
        let context = serde_json::to_vec(hello).map_err(|_| Error::Protocol("invalid hello"))?;
        self.mac(&[
            b"aw-inference/auth/v2",
            role.as_bytes(),
            &context,
            nonce.as_bytes(),
        ])?
        .verify_slice(&unhex(&proof.value)?)
        .map_err(|_| Error::Denied("device authentication failed"))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    sequence: u64,
    payload: String,
    mac: String,
}
/// Stateful frame authenticator. Connection errors poison the containing stream:
/// callers close it, not reuse a half-written frame or automatically replay a call.
pub struct Authenticator {
    key: std::sync::Arc<PairingKey>,
    session: String,
    send_role: &'static str,
    recv_role: &'static str,
    sent: u64,
    received: u64,
}
impl Authenticator {
    pub fn new(
        key: std::sync::Arc<PairingKey>,
        hello: &Hello,
        nonce: &str,
        owner: bool,
    ) -> Result<Self> {
        hello.validate()?;
        unhex(nonce)?;
        Ok(Self {
            key,
            session: digest(&(hello, nonce))?,
            send_role: if owner { "owner" } else { "worker" },
            recv_role: if owner { "worker" } else { "owner" },
            sent: 0,
            received: 0,
        })
    }
    fn tag(&self, role: &str, sequence: u64, payload: &str) -> Result<Hmac<Sha256>> {
        self.key.mac(&[
            b"aw-inference/frame/v2",
            self.session.as_bytes(),
            role.as_bytes(),
            &sequence.to_be_bytes(),
            payload.as_bytes(),
        ])
    }
    fn encode<T: Serialize>(&mut self, value: &T) -> Result<Envelope> {
        let payload =
            serde_json::to_string(value).map_err(|_| Error::Protocol("invalid device payload"))?;
        if payload.len() > PAYLOAD_LIMIT {
            return Err(Error::Limit("device payload limit"));
        }
        let sequence = self
            .sent
            .checked_add(1)
            .ok_or(Error::Limit("device sequence exhausted"))?;
        let mac = hex(&self
            .tag(self.send_role, sequence, &payload)?
            .finalize()
            .into_bytes());
        self.sent = sequence;
        Ok(Envelope {
            sequence,
            payload,
            mac,
        })
    }
    fn decode<T: DeserializeOwned>(&mut self, envelope: Envelope) -> Result<T> {
        let expected = self
            .received
            .checked_add(1)
            .ok_or(Error::Limit("device sequence exhausted"))?;
        if envelope.sequence != expected || envelope.payload.len() > PAYLOAD_LIMIT {
            return Err(Error::Protocol("device sequence/payload rejected"));
        }
        self.tag(self.recv_role, envelope.sequence, &envelope.payload)?
            .verify_slice(&unhex(&envelope.mac)?)
            .map_err(|_| Error::Denied("device frame authentication failed"))?;
        let value = serde_json::from_str(&envelope.payload)
            .map_err(|_| Error::Protocol("invalid device message fields"))?;
        self.received = expected;
        Ok(value)
    }
}
pub struct Sender<W> {
    output: W,
    auth: Authenticator,
}
pub struct Receiver<R> {
    input: R,
    reader: aw_client::framing::Reader,
    auth: Authenticator,
}
impl<W: AsyncWrite + Unpin> Sender<W> {
    pub fn new(output: W, auth: Authenticator) -> Self {
        Self { output, auth }
    }
    pub async fn send<T: Serialize>(&mut self, value: &T) -> Result<()> {
        let frame = self.auth.encode(value)?;
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            aw_client::framing::write(&mut self.output, &frame, FRAME_LIMIT),
        )
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(ipc)
    }
}
impl<R: AsyncRead + Unpin> Receiver<R> {
    pub fn new(input: R, auth: Authenticator) -> Self {
        Self {
            input,
            auth,
            reader: aw_client::framing::Reader::new(),
        }
    }
    pub async fn next<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        match self
            .reader
            .next::<_, Envelope>(&mut self.input, FRAME_LIMIT)
            .await
            .map_err(ipc)?
        {
            Some(envelope) => self.auth.decode(envelope).map(Some),
            None => Ok(None),
        }
    }
}
pub async fn owner_handshake<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    key: &PairingKey,
    binding: Binding,
) -> Result<(Hello, String)> {
    let hello = Hello::new(binding)?;
    aw_client::framing::write(stream, &hello, HANDSHAKE_LIMIT)
        .await
        .map_err(ipc)?;
    let proof: Proof = aw_client::framing::Reader::new()
        .next(stream, HANDSHAKE_LIMIT)
        .await
        .map_err(ipc)?
        .ok_or(Error::Transport("device disconnected at handshake"))?;
    key.verify(&hello, &proof.nonce, "worker", &proof)?;
    let response = key.proof(&hello, &proof.nonce, "owner")?;
    aw_client::framing::write(stream, &response, HANDSHAKE_LIMIT)
        .await
        .map_err(ipc)?;
    Ok((hello, proof.nonce))
}
pub async fn worker_handshake<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    key: &PairingKey,
    binding: &Binding,
) -> Result<(Hello, String)> {
    let mut reader = aw_client::framing::Reader::new();
    let hello: Hello = reader
        .next(stream, HANDSHAKE_LIMIT)
        .await
        .map_err(ipc)?
        .ok_or(Error::Transport("owner disconnected at handshake"))?;
    hello.validate()?;
    if &hello.binding != binding {
        return Err(Error::Denied(
            "owner identity/account/model differs from the local grant",
        ));
    }
    // Fresh worker nonce prevents replay of an old authenticated owner challenge.
    let nonce = random_id()?;
    aw_client::framing::write(
        stream,
        &key.proof(&hello, &nonce, "worker")?,
        HANDSHAKE_LIMIT,
    )
    .await
    .map_err(ipc)?;
    let reply: Proof = reader
        .next(stream, HANDSHAKE_LIMIT)
        .await
        .map_err(ipc)?
        .ok_or(Error::Transport("owner disappeared"))?;
    key.verify(&hello, &nonce, "owner", &reply)?;
    Ok((hello, nonce))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireMessage {
    pub id: u64,
    pub role: Role,
    pub content: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
    /// Opaque handle issued by this device for this turn, never raw private continuation.
    pub continuation: Option<String>,
}
impl WireMessage {
    pub fn from_message(m: &Message) -> Self {
        Self {
            id: m.id,
            role: m.role,
            content: m.content.clone(),
            tool_calls: m.tool_calls.clone(),
            tool_call_id: m.tool_call_id.clone(),
            name: m.name.clone(),
            continuation: m.provider_context.clone(),
        }
    }
    pub fn public_message(&self) -> Message {
        Message {
            id: self.id,
            role: self.role,
            content: self.content.clone(),
            tool_calls: self.tool_calls.clone(),
            tool_call_id: self.tool_call_id.clone(),
            name: self.name.clone(),
            provider_context: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub effect: aw_agent_core::ToolEffect,
}
impl From<&ToolSpec> for WireTool {
    fn from(t: &ToolSpec) -> Self {
        Self {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: t.parameters.clone(),
            effect: t.effect.clone(),
        }
    }
}
impl WireTool {
    pub fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: self.parameters.clone(),
            effect: self.effect.clone(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub id: String,
    pub epoch: String,
    pub context: InvocationContext,
    pub messages: Vec<WireMessage>,
    pub tools: Vec<WireTool>,
}
impl Request {
    pub fn identity(context: &InvocationContext) -> Result<String> {
        let scope = context.scope.as_ref().ok_or(Error::Denied(
            "device inference requires durable session and branch IDs",
        ))?;
        label(&scope.session_id)?;
        label(&scope.branch_id)?;
        if context.turn_id == 0
            || context.turn_id >= (1u64 << 42)
            || context.ordinal == 0
            || context.ordinal > 32
        {
            return Err(Error::Protocol("invalid invocation identity"));
        }
        // Turn IDs are global within a session. Branch changes cannot evade replay checks.
        Ok(format!(
            "{}:{}:{}",
            scope.session_id, context.turn_id, context.ordinal
        ))
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != VERSION || self.id != Self::identity(&self.context)? {
            return Err(Error::Protocol("invalid inference request identity"));
        }
        unhex(&self.epoch)?;
        let public: Vec<_> = self
            .messages
            .iter()
            .map(WireMessage::public_message)
            .collect();
        aw_agent_core::validate_messages(&public)?;
        if let aw_agent_core::InvocationPurpose::Summary {
            operation_id,
            input_sha256,
            sources,
        } = &self.context.purpose
        {
            label(operation_id)?;
            if sources.is_empty()
                || sources.len() > 512
                || !self.tools.is_empty()
                || self.messages.iter().any(|m| m.continuation.is_some())
            {
                return Err(Error::Denied(
                    "summary must be tool-free with a bounded source manifest",
                ));
            }
            for source in sources {
                aw_agent_core::validate_evidence_ref(source)
                    .map_err(|_| Error::Denied("invalid summary evidence"))?;
            }
            let digest = aw_agent_core::summary_message_digest(&public)
                .map_err(|_| Error::Denied("invalid summary input envelope"))?;
            if &digest != input_sha256 {
                return Err(Error::Denied(
                    "summary consent does not match exact model input",
                ));
            }
            // The exact input digest covers source dependencies in the quoted
            // evidence. Validate it equals the separately typed manifest too.
            let envelope: serde_json::Value = serde_json::from_str(&public[1].content)
                .map_err(|_| Error::Denied("invalid summary JSON"))?;
            let embedded: Vec<aw_agent_core::EvidenceRef> =
                serde_json::from_value(envelope["evidence"]["dependencies"].clone())
                    .map_err(|_| Error::Denied("summary dependency manifest missing"))?;
            if &embedded != sources {
                return Err(Error::Denied("summary manifest and consent differ"));
            }
        }
        for m in &self.messages {
            if m.name
                .as_ref()
                .is_some_and(|n| n.len() > 128 || n.chars().any(char::is_control))
            {
                return Err(Error::Protocol("invalid tool message name"));
            }
            if let Some(token) = &m.continuation {
                if m.role != Role::Assistant {
                    return Err(Error::Protocol("continuation requires assistant role"));
                }
                unhex(token)?;
            }
        }
        if self.tools.len() > 32
            || serde_json::to_vec(&self.tools)
                .map_err(|_| Error::Protocol("invalid schemas"))?
                .len()
                > 128 * 1024
        {
            return Err(Error::Limit("device schema budget"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for tool in &self.tools {
            label(&tool.name)?;
            if !seen.insert(&tool.name)
                || tool.description.len() > 8192
                || !tool.parameters.is_object()
            {
                return Err(Error::Protocol("invalid or duplicate tool schema"));
            }
        }
        if serde_json::to_vec(self)
            .map_err(|_| Error::Protocol("invalid request"))?
            .len()
            > PAYLOAD_LIMIT
        {
            return Err(Error::Limit("device request payload budget"));
        }
        Ok(())
    }
    pub fn context_bytes(&self) -> usize {
        self.messages
            .iter()
            .map(|m| m.public_message().context_bytes())
            .sum()
    }
    pub fn request_digest(&self) -> Result<String> {
        let mut request = self.clone();
        request.epoch.clear();
        request.context.identity.grant_revision = 0;
        digest(&request)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Delta {
        id: String,
        sequence: u64,
        text: String,
    },
    Finished {
        id: String,
        sequence: u64,
        meta: CompletionMeta,
        tool_calls: Vec<ToolCall>,
        continuation: Option<String>,
    },
    Failed {
        id: String,
        sequence: u64,
        code: Failure,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    Denied,
    Limit,
    AlreadyAttempted,
    Unknown,
    Provider,
    Expired,
    Cancelled,
    Protocol,
}
impl Failure {
    pub fn error(&self) -> Error {
        match self {
            Self::Denied => Error::Denied("local device grant rejected inference"),
            Self::Limit => Error::Limit("local device budget exhausted"),
            Self::AlreadyAttempted => Error::Conflict(
                "device already attempted this invocation; inspect receipts, do not resend",
            ),
            Self::Expired => Error::Denied("local device grant expired or changed"),
            Self::Cancelled => Error::Cancelled,
            Self::Protocol => Error::Protocol("device rejected protocol or model response"),
            _ => {
                Error::Transport("device inference failed or outcome unknown; no fallback or retry")
            }
        }
    }
}
pub fn validate_reply(reply: &ModelReply, tools: &[WireTool]) -> Result<()> {
    if reply.meta.finish_reason.len() > 64
        || reply.meta.finish_reason.chars().any(char::is_control)
        || reply.tool_calls.len() > 16
    {
        return Err(Error::Protocol("invalid device completion"));
    }
    if reply
        .provider_context
        .as_ref()
        .is_some_and(|s| s.len() > RESPONSE_LIMIT)
    {
        return Err(Error::Limit("private continuation exceeds local budget"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for call in &reply.tool_calls {
        call.validate_envelope()?;
        if !seen.insert(&call.id) || !tools.iter().any(|t| t.name == call.name) {
            return Err(Error::Denied(
                "unadvertised or duplicate device tool proposal",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding() -> Binding {
        Binding {
            actor: "unix:1000".into(),
            account_id: "laptop".into(),
            executor_id: "device-a".into(),
            model: "test-model".into(),
            grant_id: "g1".into(),
        }
    }
    #[test]
    fn labels_are_bounded() {
        for s in ["", "x/y", "a\n", "@/?", &"x".repeat(129)] {
            assert!(label(s).is_err());
        }
        assert!(label("unix:1000").is_ok());
    }
    #[test]
    fn canonical_hex_only() {
        assert!(unhex(&"aa".repeat(32)).is_ok());
        for s in ["x".into(), "AA".repeat(32), "z0".repeat(32)] {
            assert!(unhex(&s).is_err());
        }
    }
    #[test]
    fn auth_roles_cannot_reflect() {
        let h = Hello::new(binding()).unwrap();
        let k = PairingKey::from_bytes([1; 32]);
        let n = random_id().unwrap();
        let p = k.proof(&h, &n, "worker").unwrap();
        assert!(k.verify(&h, &n, "worker", &p).is_ok());
        assert!(k.verify(&h, &n, "owner", &p).is_err());
    }
    #[test]
    fn auth_nonce_is_required() {
        let h = Hello::new(binding()).unwrap();
        let k = PairingKey::from_bytes([1; 32]);
        let n = random_id().unwrap();
        let p = k.proof(&h, &n, "worker").unwrap();
        assert!(k.verify(&h, &random_id().unwrap(), "worker", &p).is_err());
    }
    #[test]
    fn auth_binds_model() {
        let mut h = Hello::new(binding()).unwrap();
        let k = PairingKey::from_bytes([1; 32]);
        let n = random_id().unwrap();
        let p = k.proof(&h, &n, "worker").unwrap();
        h.binding.model = "other".into();
        assert!(k.verify(&h, &n, "worker", &p).is_err());
    }
    #[test]
    fn wrong_key_denied() {
        let h = Hello::new(binding()).unwrap();
        let n = random_id().unwrap();
        let p = PairingKey::from_bytes([1; 32])
            .proof(&h, &n, "worker")
            .unwrap();
        assert!(PairingKey::from_bytes([2; 32])
            .verify(&h, &n, "worker", &p)
            .is_err());
    }
    #[test]
    fn frames_are_sequenced() {
        let h = Hello::new(binding()).unwrap();
        let n = random_id().unwrap();
        let k = std::sync::Arc::new(PairingKey::from_bytes([3; 32]));
        let mut a = Authenticator::new(k.clone(), &h, &n, true).unwrap();
        let mut b = Authenticator::new(k, &h, &n, false).unwrap();
        let e = a.encode(&"hello").unwrap();
        let copy = serde_json::to_vec(&e).unwrap();
        assert_eq!(b.decode::<String>(e).unwrap(), "hello");
        assert!(b
            .decode::<String>(serde_json::from_slice(&copy).unwrap())
            .is_err());
    }
    #[test]
    fn frame_tamper_denied() {
        let h = Hello::new(binding()).unwrap();
        let n = random_id().unwrap();
        let k = std::sync::Arc::new(PairingKey::from_bytes([3; 32]));
        let mut a = Authenticator::new(k.clone(), &h, &n, true).unwrap();
        let mut b = Authenticator::new(k, &h, &n, false).unwrap();
        let mut e = a.encode(&"hello").unwrap();
        e.payload = "\"changed\"".into();
        assert!(b.decode::<String>(e).is_err());
    }
    #[test]
    fn payload_extra_fields_denied() {
        assert!(serde_json::from_str::<Binding>(r#"{"actor":"x","account_id":"y","executor_id":"z","model":"m","grant_id":"g","api_key":"secret"}"#).is_err());
    }
    #[tokio::test]
    async fn reciprocal_handshake() {
        let (mut a, mut b) = tokio::io::duplex(16384);
        let key = PairingKey::from_bytes([7; 32]);
        let bind = binding();
        let (x, y) = tokio::join!(
            owner_handshake(&mut a, &key, bind.clone()),
            worker_handshake(&mut b, &key, &bind)
        );
        let (h, n) = x.unwrap();
        let (h2, n2) = y.unwrap();
        assert_eq!(h.epoch, h2.epoch);
        assert_eq!(n, n2);
    }
}

#[cfg(test)]
mod vectors {
    use super::*;
    #[test]
    fn independent_python_handshake_vector_matches_rust_serialization() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/handshake-v2.json")).unwrap();
        let hello: Hello = serde_json::from_value(v["hello"].clone()).unwrap();
        let key = PairingKey::from_bytes(
            unhex(v["test_key_hex"].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap(),
        );
        let nonce = v["worker_nonce"].as_str().unwrap();
        assert_eq!(
            key.proof(&hello, nonce, "worker").unwrap().value,
            v["worker_proof"].as_str().unwrap()
        );
        assert_eq!(
            key.proof(&hello, nonce, "owner").unwrap().value,
            v["owner_proof"].as_str().unwrap()
        );
    }
}
