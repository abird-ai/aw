//! Native UTF-8 tools. The agent handles grants/journal/scheduling; the
//! platform root handles scoped IO, staged publication and mutation locking.
use crate::{platform_fs, tool_failure};
use aw_agent_core::*;
use aw_platform::scoped_fs::Root;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

const READ_BYTES: usize = 96 * 1024;
const MAX_WRITE_BYTES: usize = 48 * 1024;
fn one() -> usize {
    1
}
fn four_k() -> usize {
    4096
}
fn yes() -> bool {
    true
}
fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|_| Error::Denied("invalid, missing or unknown file arguments"))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    path: String,
    #[serde(default = "one")]
    offset: usize,
    #[serde(default = "four_k")]
    limit: usize,
    #[serde(default = "yes")]
    numbered: bool,
    /// Only the TurnMachine interprets this scheduling suggestion.
    #[serde(default)]
    parallel: bool,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteArgs {
    path: String,
    content: String,
    #[serde(default)]
    create_only: bool,
    expected_sha256: Option<String>,
}

#[derive(Clone, Copy)]
pub(crate) enum Operation {
    Read,
    Write,
}
impl Operation {
    fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}
pub(crate) struct Text {
    root: Arc<Root>,
    op: Operation,
}
impl Text {
    pub(crate) fn new(root: Arc<Root>, op: Operation) -> Self {
        Self { root, op }
    }
}
fn bounded_window(
    mut window: String,
    offset: usize,
    cap: usize,
    truncated: bool,
) -> (String, bool) {
    if !truncated {
        return (window, false);
    }
    loop {
        let rows = window.bytes().filter(|b| *b == b'\n').count()
            + usize::from(!window.is_empty() && !window.ends_with('\n'));
        let detailed = format!(
            "… truncated after line {}\n",
            offset.saturating_add(rows).saturating_sub(1)
        );
        let marker = if detailed.len() <= cap {
            detailed
        } else if "… truncated\n".len() <= cap {
            "… truncated\n".to_owned()
        } else {
            // Even a fixed marker does not fit; never spin trying to make
            // an impossible suffix fit the caller's byte budget.
            return (String::new(), true);
        };
        let join = usize::from(!window.is_empty() && !window.ends_with('\n'));
        if window
            .len()
            .saturating_add(join)
            .saturating_add(marker.len())
            <= cap
        {
            if join == 1 {
                window.push('\n');
            }
            window.push_str(&marker);
            return (window, true);
        }
        if let Some(pos) = window.trim_end_matches('\n').rfind('\n') {
            window.truncate(pos + 1);
        } else {
            window.clear();
        }
    }
}
impl Tool for Text {
    fn spec(&self) -> ToolSpec {
        let (description, parameters, effect) = match self.op {
            Operation::Read => (
                "Stream a UTF-8 line window from a granted regular file. offset is 1-based (default 1), limit defaults to 4096, numbered=true prefixes N and a tab; numbered=false preserves exact bytes including CRLF and terminal newline. A bounded reply reports truncation. Set parallel=true only for independent reads.",
                json!({"type":"object","properties":{
                    "path":{"type":"string"},
                    "offset":{"type":"integer","minimum":1},
                    "limit":{"type":"integer","minimum":1,"maximum":65536},
                    "numbered":{"type":"boolean"},
                    "parallel":{"type":"boolean"}},"required":["path"],"additionalProperties":false}),
                ToolEffect::Read,
            ),
            Operation::Write => (
                "Create or atomically replace a UTF-8 file, creating missing parent directories. Existing regular-file permissions are preserved. Model must inspect the file first when replacement is not intended. The write is not a multi-file transaction.",
                json!({"type":"object","properties":{
                    "path":{"type":"string"},
                    "content":{"type":"string","maxLength":MAX_WRITE_BYTES},
                    "create_only":{"type":"boolean"},
                    "expected_sha256":{"type":"string"}},"required":["path","content"],"additionalProperties":false}),
                ToolEffect::Write,
            ),
        };
        ToolSpec {
            name: self.op.name().into(),
            description: description.into(),
            parameters,
            effect,
        }
    }
    fn validate(&self, value: &Value) -> Result<()> {
        match self.op {
            Operation::Read => {
                let a: ReadArgs = parse(value)?;
                platform_fs(aw_platform::scoped_fs::validate_path(&a.path))?;
                if a.offset == 0 || a.limit == 0 || a.limit > 65_536 {
                    return Err(Error::Denied("invalid read window"));
                }
                let _ = a.parallel;
            }
            Operation::Write => {
                let a: WriteArgs = parse(value)?;
                platform_fs(aw_platform::scoped_fs::validate_path(&a.path))?;
                if a.content.len() > MAX_WRITE_BYTES {
                    return Err(Error::Limit("write payload exceeds 48 KiB"));
                }
                crate::text_class::require_text(a.content.as_bytes())?;
                crate::mutate::validate_digest(a.expected_sha256.as_deref())?;
            }
        }
        Ok(())
    }
    fn execute<'a>(&'a self, ctx: ToolContext, value: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(value)?;
            let value = value.clone();
            let root = self.root.clone();
            let op = self.op;
            let answer = tokio::task::spawn_blocking(move || -> Result<ToolOutput> {
                if ctx.cancellation.is_cancelled() {
                    return Err(Error::Cancelled);
                }
                match op {
                    Operation::Read => {
                        let a: ReadArgs = parse(&value)?;
                        let (window, truncated) = platform_fs(root.read_window_with_cancel(
                            &a.path,
                            a.offset,
                            a.limit,
                            READ_BYTES,
                            a.numbered,
                            &|| ctx.cancellation.is_cancelled(),
                        ))?;
                        crate::text_class::require_text(window.as_bytes())?;
                        let (body, truncated) =
                            bounded_window(window, a.offset, READ_BYTES, truncated);
                        Ok(ToolOutput {
                            content: body,
                            is_error: false,
                            truncated,
                        })
                    }
                    Operation::Write => {
                        let a: WriteArgs = parse(&value)?;
                        let changed = crate::mutate::write(
                            &root,
                            &a.path,
                            a.content.as_bytes(),
                            a.create_only,
                            a.expected_sha256.as_deref(),
                            &|| ctx.cancellation.is_cancelled(),
                        )?;
                        Ok(ToolOutput::text(
                            json!({
                                "path": a.path,
                                "bytes_written": if changed { a.content.len() } else { 0 },
                                "changed": changed
                            })
                            .to_string(),
                        ))
                    }
                }
            })
            .await
            .map_err(|_| {
                Error::Protocol("file worker failed; inspect workspace before retrying")
            })?;
            tool_failure(answer, matches!(op, Operation::Write))
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn window_marker_fits_and_never_cuts_lines() {
        let text = "x\n".repeat(100);
        let (body, truncated) = bounded_window(text, 1, 25, true);
        assert!(truncated);
        assert!(body.len() <= 25);
        assert!(body.ends_with('\n'));
        assert!(body.contains("… truncated"));
    }
}
