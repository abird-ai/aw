use super::background::Executor;
use aw::Result;
use aw_tools::browse::{Entry, Listing, WorkspaceFiles};
use std::{path::PathBuf, sync::{atomic::{AtomicBool, Ordering}, Arc}};
use tokio::sync::mpsc as async_channel;

#[derive(Debug)]
pub enum Request {
    List(String),
    Read(String),
    Search {
        query: String,
        limit: usize,
        cancel: Arc<AtomicBool>,
    },
}
#[derive(Debug)]
pub enum Content {
    Directory(Listing),
    File { path: String, text: String },
    Search { query: String, entries: Vec<Entry> },
}
#[derive(Debug)]
pub struct Response {
    pub ticket: u64,
    pub result: std::result::Result<Content, String>,
}

pub struct Browser {
    background: Executor,
    root: PathBuf,
    reply_tx: async_channel::Sender<Response>,
    pub replies: async_channel::Receiver<Response>,
}
impl Browser {
    pub fn new(root: PathBuf, background: Executor) -> Result<Self> {
        let (reply_tx, replies) = async_channel::channel(32);
        Ok(Self {
            background,
            root,
            reply_tx,
            replies,
        })
    }
    pub fn request(&self, ticket: u64, request: Request) -> Result<()> {
        let root = self.root.clone();
        let cancel = match &request {
            Request::Search { cancel, .. } => Some(cancel.clone()),
            _ => None,
        };
        let out = self.reply_tx.clone();
        let panic_out = out.clone();
        self.background.try_submit_with_panic("files", move || {
            let value = (|| -> Result<Content> {
                let files = WorkspaceFiles::open(&root)?;
                match request {
                    Request::List(path) => Ok(Content::Directory(files.list(&path)?)),
                    Request::Read(path) => Ok(Content::File {
                        text: files.read(&path)?,
                        path,
                    }),
                    Request::Search {
                        query,
                        limit,
                        cancel,
                    } => Ok(Content::Search {
                        entries: files.search_with_cancel(&query, limit, || {
                            cancel.load(Ordering::Acquire)
                        })?,
                        query,
                    }),
                }
            })()
            .map_err(|e| e.to_string());
            if cancel
                .as_ref()
                .is_some_and(|cancel| cancel.load(Ordering::Acquire))
            {
                return;
            }
            let _ = out.blocking_send(Response { ticket, result: value });
        }, move || {
            let _ = panic_out.blocking_send(Response {
                ticket,
                result: Err("file background job panicked; retry explicitly".into()),
            });
        })
    }
}
