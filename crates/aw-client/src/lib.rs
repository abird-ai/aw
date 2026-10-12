//! Local owner-client SDK. No provider, agent engine, SQLite or HTTP dependency.
//! This is same-UID Linux IPC, not Internet auth or device inference delegation.
pub mod framing;
pub mod local;
pub mod protocol;
pub use protocol::{AccountChoice, Command, Hello, Packet, PromptInput, Receipt, Snapshot};
use protocol::{Request, Response, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, VERSION};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{path::Path, time::Duration};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("local transport I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("owner protocol: {0}")]
    Protocol(&'static str),
    #[error("local endpoint: {0}")]
    Local(&'static str),
    #[error("owner rejected request ({code}): {message}")]
    Rejected { code: u16, message: String },
    #[error("owner connection timed out; outcome may be unknown, do not automatically replay")]
    Timeout,
    #[error("owner connection closed; outcome may be unknown, do not automatically replay")]
    Closed,
    #[error("local owner attachment is supported on Linux only")]
    Unsupported,
}
pub type Result<T> = std::result::Result<T, Error>;
pub fn request_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| Error::Local("OS randomness unavailable"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// One ordered connection. Mutating-call cancellation/error poisons the socket,
/// never retries it. Follow reads retain decoder state across cancelled polls.
#[cfg(target_os = "linux")]
pub struct Client {
    stream: Option<tokio::net::UnixStream>,
    reader: framing::Reader,
    next_id: u64,
    hello: Hello,
    following: Option<u64>,
    timeout: Duration,
}
#[cfg(target_os = "linux")]
impl Client {
    pub async fn connect(path: &Path) -> Result<Self> {
        let uid = local::effective_uid()?;
        let before = local::socket_identity(path, uid)?;
        let mut stream = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::net::UnixStream::connect(path),
        )
        .await
        .map_err(|_| Error::Timeout)??;
        if stream.peer_cred()?.uid() != uid || local::socket_identity(path, uid)? != before {
            return Err(Error::Local(
                "owner peer or socket changed during connection",
            ));
        }
        let request = Request {
            version: VERSION,
            id: 1,
            expected_instance: None,
            command: Command::Hello,
        };
        let mut reader = framing::Reader::new();
        let reply = tokio::time::timeout(Duration::from_secs(10), async {
            framing::write(&mut stream, &request, MAX_REQUEST_BYTES).await?;
            reader
                .next::<_, Response>(&mut stream, MAX_RESPONSE_BYTES)
                .await?
                .ok_or(Error::Closed)
        })
        .await
        .map_err(|_| Error::Timeout)??;
        validate_response(&reply, 1)?;
        let hello: Hello = decode_result(reply.packet)?;
        if hello.protocol != VERSION
            || hello.owner_uid != uid
            || hello.principal != format!("unix:{uid}")
            || hello.instance.len() != 64
            || !hello.instance.bytes().all(|b| b.is_ascii_hexdigit())
            || hello.package_version.is_empty()
            || hello.package_version.len() > 128
            || hello.package_version.chars().any(char::is_control)
            || hello.inference_location.len() > 512
            || hello.inference_location.chars().any(char::is_control)
            || hello.capabilities.len() > 32
            || hello
                .capabilities
                .iter()
                .any(|c| c.is_empty() || c.len() > 128 || c.chars().any(char::is_control))
        {
            return Err(Error::Protocol("invalid or oversized owner handshake"));
        }
        Ok(Self {
            stream: Some(stream),
            reader,
            next_id: 2,
            hello,
            following: None,
            timeout: Duration::from_secs(30),
        })
    }
    pub fn hello(&self) -> &Hello {
        &self.hello
    }
    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }
    pub async fn snapshot(&mut self) -> Result<Snapshot> {
        self.call(Command::Snapshot).await
    }
    pub async fn call<T: DeserializeOwned>(&mut self, command: Command) -> Result<T> {
        if matches!(&command, Command::Follow { .. }) {
            return Err(Error::Protocol(
                "use start_follow to preserve the connection state",
            ));
        }
        self.exchange(command).await
    }
    async fn exchange<T: DeserializeOwned>(&mut self, command: Command) -> Result<T> {
        if self.following.is_some() {
            return Err(Error::Protocol(
                "connection is following a run; use another client for control",
            ));
        }
        command.validate().map_err(Error::Protocol)?;
        if matches!(command, Command::Hello) {
            return Err(Error::Protocol("handshake already completed"));
        }
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or(Error::Protocol("request sequence exhausted"))?;
        let request = Request {
            version: VERSION,
            id,
            expected_instance: Some(self.hello.instance.clone()),
            command,
        };
        // Taking ownership is intentional. Dropping this future drops the socket.
        let mut socket = self.stream.take().ok_or(Error::Closed)?;
        let mut reader = std::mem::take(&mut self.reader);
        let response = tokio::time::timeout(self.timeout, async {
            framing::write(&mut socket, &request, MAX_REQUEST_BYTES).await?;
            let response = reader
                .next::<_, Response>(&mut socket, MAX_RESPONSE_BYTES)
                .await?
                .ok_or(Error::Closed)?;
            validate_response(&response, id)?;
            Ok::<_, Error>(response)
        })
        .await
        .map_err(|_| Error::Timeout)??;
        self.stream = Some(socket);
        self.reader = reader;
        let result = decode_result(response.packet);
        // An application denial is a complete reply. An invalid reply schema
        // poisons the transport: unread/unsolicited data must never become the
        // answer to a later command, especially after an ambiguous mutation.
        if matches!(&result, Err(Error::Protocol(_))) {
            self.stream = None;
        }
        result
    }
    pub async fn start_follow(&mut self, run_id: u64) -> Result<()> {
        let id = self.next_id;
        let _: Value = self.exchange(Command::Follow { run_id }).await?;
        self.following = Some(id);
        Ok(())
    }
    /// Cancellation-safe while waiting for a frame. A control request must use
    /// another authenticated connection; dropping this client merely detaches.
    pub async fn next_event(&mut self) -> Result<Option<Packet>> {
        let id = self
            .following
            .ok_or(Error::Protocol("not following a run"))?;
        let socket = self.stream.as_mut().ok_or(Error::Closed)?;
        let response = match self
            .reader
            .next::<_, Response>(socket, MAX_RESPONSE_BYTES)
            .await
        {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                self.stream = None;
                self.following = None;
                return Err(Error::Closed);
            }
            Err(error) => {
                self.stream = None;
                self.following = None;
                return Err(error);
            }
        };
        if let Err(e) = validate_response(&response, id) {
            self.stream = None;
            self.following = None;
            return Err(e);
        }
        match response.packet {
            Packet::End { reason } => {
                self.following = None;
                Ok(Some(Packet::End { reason }))
            }
            packet @ Packet::Event { .. } => Ok(Some(packet)),
            Packet::Error { code, message } => {
                self.stream = None;
                self.following = None;
                Err(Error::Rejected { code, message })
            }
            _ => {
                self.stream = None;
                self.following = None;
                Err(Error::Protocol("unexpected following reply"))
            }
        }
    }
    /// Convenience observer. Returning an error detaches locally without
    /// claiming the owner stopped. Do not use observation as an approval hook.
    pub async fn follow(
        &mut self,
        run_id: u64,
        mut observe: impl FnMut(Packet) -> Result<()>,
    ) -> Result<()> {
        self.start_follow(run_id).await?;
        loop {
            let event = self.next_event().await?.ok_or(Error::Closed)?;
            let end = matches!(&event, Packet::End { .. });
            if let Err(error) = observe(event) {
                self.stream = None;
                self.following = None;
                return Err(error);
            }
            if end {
                return Ok(());
            }
        }
    }
}

fn validate_response(frame: &Response, id: u64) -> Result<()> {
    if frame.version != VERSION || frame.id != id {
        return Err(Error::Protocol("unexpected protocol version or reply ID"));
    }
    Ok(())
}
fn decode_result<T: DeserializeOwned>(packet: Packet) -> Result<T> {
    match packet {
        Packet::Result { value } => {
            serde_json::from_value(value).map_err(|_| Error::Protocol("unexpected result schema"))
        }
        Packet::Error { code, message } => Err(Error::Rejected { code, message }),
        _ => Err(Error::Protocol(
            "unexpected streaming packet in command reply",
        )),
    }
}
