//! One explicitly admitted child request channel, serviced only by the
//! original finite Process wait. Bounded synchronous host callback; no
//! detached worker, listener, replay or unbounded response queue.
use std::io;
use tokio::{net::UnixStream, time::Instant};

const MAX_REQUESTS: usize = 64;
const MAX_REPLY: usize = 4096;

pub type Reply<'a> = dyn FnMut(&[u8]) -> io::Result<()> + 'a;
type Handler = dyn FnMut(u8, &mut Reply<'_>) -> io::Result<()> + Send;

pub(super) struct RequestSocket {
    socket: UnixStream,
    handler: Box<Handler>,
    cancelled: Option<Box<dyn Fn() -> bool + Send>>,
}

impl RequestSocket {
    pub(super) fn new(
        socket: std::os::unix::net::UnixStream,
        handler: impl FnMut(u8, &mut Reply<'_>) -> io::Result<()> + Send + 'static,
    ) -> io::Result<Self> {
        Ok(Self {
            socket: UnixStream::from_std(socket)?,
            handler: Box::new(handler),
            cancelled: None,
        })
    }

    pub(super) fn set_cancellation(&mut self, cancelled: Option<Box<dyn Fn() -> bool + Send>>) {
        self.cancelled = cancelled;
    }

    pub(super) async fn serve(
        self,
        deadline: Instant,
        leader_ready: impl Fn() -> io::Result<bool>,
    ) {
        let Self {
            socket,
            mut handler,
            cancelled,
        } = self;
        for _ in 0..MAX_REQUESTS {
            let mut request = [0];
            loop {
                if socket.readable().await.is_err() {
                    return;
                }
                match socket.try_read(&mut request) {
                    Ok(1) => break,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                    _ => return,
                }
            }
            // Arm write readiness before entering any authority guard.
            if socket.writable().await.is_err() {
                return;
            }
            {
                let live = || {
                    Instant::now() < deadline
                        && !cancelled.as_ref().is_some_and(|probe| probe())
                        && matches!(leader_ready(), Ok(false))
                };
                if !live() {
                    return;
                }
                // The provider must hold authority through this actual write.
                // No await, retry, queued proof or partial-response continuation.
                let mut attempted = false;
                let mut complete = false;
                let mut reply = |bytes: &[u8]| {
                    if attempted || bytes.is_empty() || bytes.len() > MAX_REPLY || !live() {
                        return Err(io::Error::other("Process request channel unavailable"));
                    }
                    attempted = true;
                    let written = socket.try_write(bytes)?;
                    if written != bytes.len() {
                        return Err(io::Error::other("incomplete Process reply"));
                    }
                    complete = true;
                    Ok(())
                };
                if handler(request[0], &mut reply).is_err() || !complete {
                    return;
                }
            }
            // Fairness for a peer with buffered requests, especially against
            // cancellation and the original leader-exit observation.
            tokio::task::yield_now().await;
        }
    }
}
