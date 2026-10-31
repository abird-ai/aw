//! Native UTF-8 tools. The agent handles grants/journal/scheduling; the
//! platform root handles scoped IO, staged publication and mutation locking.
use crate::{platform_fs, tool_failure};
use aw_agent_core::*;
use aw_platform::scoped_fs::Root;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

const READ_BYTES: usize = 96 * 1024;
const MAX_WRITE_BYTES: usize = 48 * 1024;
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
    line: Option<usize>,
    lines: Option<usize>,
    byte: Option<usize>,
    #[serde(default = "yes")]
    numbered: bool,
    // The old offset/limit read envelope is an input-only compatibility
    // adapter, absent from the shared advertised schema.
    offset: Option<usize>,
    limit: Option<usize>,
    #[serde(default)]
    parallel: bool,
}
impl ReadArgs {
    fn legacy(&self) -> bool {
        self.offset.is_some() || self.limit.is_some()
    }
    fn line(&self) -> usize {
        self.line.or(self.offset).unwrap_or(1)
    }
    fn lines(&self) -> usize {
        self.lines.or(self.limit).unwrap_or(4096)
    }
    fn byte(&self) -> usize {
        self.byte.unwrap_or(0)
    }
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
        use aw_tools_core::filesystem::schema::{SchemaCaps, Tool as C};
        let tool = match self.op {
            Operation::Read => C::Read,
            Operation::Write => C::Write,
        };
        crate::core_spec(
            tool,
            SchemaCaps {
                parallel_reads: true,
                text_write_bytes: MAX_WRITE_BYTES,
                ..Default::default()
            },
        )
    }
    fn validate(&self, value: &Value) -> Result<()> {
        match self.op {
            Operation::Read => {
                let a: ReadArgs = parse(value)?;
                crate::model_path(&a.path)?;
                if a.line() == 0
                    || !(1..=65536).contains(&a.lines())
                    || a.byte() > aw_tools_core::read::MAX_OFFSET_SCAN_BYTES
                    || (a.legacy() && (a.line.is_some() || a.lines.is_some() || a.byte.is_some()))
                {
                    return Err(Error::Denied("invalid read line/lines/byte cursor"));
                }
                let _ = a.parallel;
            }
            Operation::Write => {
                let a: WriteArgs = parse(value)?;
                crate::model_path(&a.path)?;
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
            let answer =
                aw_tools_core::asynchronous::run_blocking(move || -> Result<ToolOutput> {
                    if ctx.cancellation.is_cancelled() {
                        return Err(Error::Cancelled);
                    }
                    match op {
                        Operation::Read => {
                            let a: ReadArgs = parse(&value)?;
                            if a.legacy() {
                                let (window, truncated) =
                                    platform_fs(root.read_window_with_cancel(
                                        &a.path,
                                        a.line(),
                                        a.lines(),
                                        READ_BYTES,
                                        a.numbered,
                                        &|| ctx.cancellation.is_cancelled(),
                                    ))?;
                                crate::text_class::require_text(window.as_bytes())?;
                                let (body, truncated) =
                                    bounded_window(window, a.line(), READ_BYTES, truncated);
                                return Ok(ToolOutput {
                                    content: body,
                                    is_error: false,
                                    truncated,
                                });
                            }
                            let page = platform_fs(root.read_page_with_cancel(
                                &a.path,
                                aw_tools_core::read::PageRequest {
                                    line: a.line(),
                                    lines: a.lines(),
                                    byte: a.byte(),
                                    cap: if a.numbered {
                                        READ_BYTES
                                    } else {
                                        READ_BYTES / 2
                                    },
                                    numbered: a.numbered,
                                },
                                &|| ctx.cancellation.is_cancelled(),
                            ))?;
                            let partial = page.next.is_some() || page.io_fault;
                            if !partial {
                                return Ok(ToolOutput::text(page.data));
                            }
                            let status = aw_tools_core::filesystem::status::read(
                                page.next.map(|c| (c.line, c.byte)),
                                page.io_fault,
                            )
                            .expect("partial page has a status");
                            if a.numbered {
                                let mut data = page.data;
                                if !data.ends_with('\n') {
                                    data.push('\n');
                                }
                                data.push_str(&status);
                                Ok(ToolOutput {
                                    content: data,
                                    is_error: page.io_fault,
                                    truncated: true,
                                })
                            } else {
                                // AW currently exposes one text ToolOutput,
                                // not separate exact-data/status MCP blocks.
                                // JSON preserves data bytes unambiguously on
                                // partial pages; complete reads stay raw.
                                Ok(ToolOutput {
                                    content: serde_json::json!({
                                        "data":page.data,
                                        "status":if page.io_fault{"mixed"}else{"more"},
                                        "next":page.next.map(|c|serde_json::json!({
                                            "line":c.line,"byte":c.byte
                                        }))
                                    })
                                    .to_string(),
                                    is_error: page.io_fault,
                                    truncated: true,
                                })
                            }
                        }
                        Operation::Write => {
                            let a: WriteArgs = parse(&value)?;
                            if !a.create_only && a.expected_sha256.is_none() {
                                let outcome = aw_tools_core::filesystem::operations::write_text(
                                    root.as_ref(),
                                    &a.path,
                                    &a.content,
                                    MAX_WRITE_BYTES,
                                    &|| ctx.cancellation.is_cancelled(),
                                )
                                .map_err(crate::file_operation_error)?;
                                Ok(ToolOutput::text(outcome.as_str()))
                            } else {
                                // Legacy conditional checks retain their pinned
                                // preimage semantics until core offers them too.
                                let changed = crate::mutate::write(
                                    &root,
                                    &a.path,
                                    a.content.as_bytes(),
                                    a.create_only,
                                    a.expected_sha256.as_deref(),
                                    &|| ctx.cancellation.is_cancelled(),
                                )?;
                                Ok(ToolOutput::text(aw_tools_core::mutation::compact_success(
                                    usize::from(changed),
                                    None,
                                )))
                            }
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
