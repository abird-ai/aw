//! Native UTF-8 tools. The agent handles grants/journal/scheduling; the
//! platform root handles scoped IO, staged publication and mutation locking.
use crate::{platform_fs, tool_failure};
use aw_agent_core::*;
use aw_platform::scoped_fs::Root;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

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
    fn lines(&self, ceiling: usize) -> usize {
        self.lines
            .or(self.limit)
            .unwrap_or_else(|| 4096.min(ceiling))
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
    limits: crate::LocalToolLimits,
}
impl Text {
    #[cfg(test)]
    pub(crate) fn new(root: Arc<Root>, op: Operation) -> Self {
        Self::with_limits(root, op, crate::LocalToolLimits::default())
    }
    pub(crate) fn with_limits(
        root: Arc<Root>,
        op: Operation,
        limits: crate::LocalToolLimits,
    ) -> Self {
        Self { root, op, limits }
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
        use aw_tools::filesystem::schema::Tool as C;
        let tool = match self.op {
            Operation::Read => C::Read,
            Operation::Write => C::Write,
        };
        crate::core_spec(tool, self.limits.schema)
    }
    fn validate(&self, value: &Value) -> Result<()> {
        match self.op {
            Operation::Read => {
                let a: ReadArgs = parse(value)?;
                crate::model_path(&a.path)?;
                if a.line() == 0
                    || !(1..=self.limits.schema.read_line_limit)
                        .contains(&a.lines(self.limits.schema.read_line_limit))
                    || a.byte() > aw_tools::read::MAX_OFFSET_SCAN_BYTES
                    || (a.legacy() && (a.line.is_some() || a.lines.is_some() || a.byte.is_some()))
                {
                    return Err(Error::Denied("invalid read line/lines/byte cursor"));
                }
                let _ = a.parallel;
            }
            Operation::Write => {
                let a: WriteArgs = parse(value)?;
                crate::model_path(&a.path)?;
                if a.content.len() > self.limits.schema.text_write_bytes {
                    return Err(Error::Limit(
                        "write payload exceeds configured tool ceiling",
                    ));
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
            let result_bytes = ctx.result_budget(self.limits.result_bytes)?;
            let value = value.clone();
            let root = self.root.clone();
            let op = self.op;
            let limits = self.limits;
            let answer = aw_tools::asynchronous::run_blocking(move || -> Result<ToolOutput> {
                if ctx.cancellation.is_cancelled() {
                    return Err(Error::Cancelled);
                }
                match op {
                    Operation::Read => {
                        let a: ReadArgs = parse(&value)?;
                        let content_path = crate::content_path(&root, &a.path, false)?;
                        if a.legacy() {
                            let (window, truncated) = platform_fs(root.read_window_with_cancel(
                                &content_path,
                                a.line(),
                                a.lines(limits.schema.read_line_limit),
                                result_bytes,
                                a.numbered,
                                &|| ctx.cancellation.is_cancelled(),
                            ))?;
                            crate::text_class::require_text(window.as_bytes())?;
                            let (body, truncated) =
                                bounded_window(window, a.line(), result_bytes, truncated);
                            return Ok(ToolOutput {
                                content: body,
                                is_error: false,
                                truncated,
                            });
                        }
                        let page = platform_fs(root.read_page_with_cancel(
                            &content_path,
                            aw_tools::read::PageRequest {
                                line: a.line(),
                                lines: a.lines(limits.schema.read_line_limit),
                                byte: a.byte(),
                                cap: if a.numbered {
                                    // Reserve bounded space for the continuation/fault status
                                    // appended below so the final ToolOutput stays within the
                                    // same semantic result ceiling captured by Graph nodes.
                                    result_bytes.saturating_sub(256)
                                } else {
                                    // JSON partial-page projection may escape source bytes;
                                    // half the ceiling safely reserves wrapper/escaping space.
                                    result_bytes / 2
                                },
                                numbered: a.numbered,
                            },
                            &|| ctx.cancellation.is_cancelled(),
                        ))?;
                        let partial = page.next.is_some() || page.io_fault;
                        if !partial {
                            return Ok(ToolOutput::text(page.data));
                        }
                        let status = aw_tools::filesystem::status::read(
                            page.next.map(|c| (c.line, c.byte)),
                            page.io_fault,
                        )
                        .ok_or(Error::Protocol(
                            "partial read page has no continuation status",
                        ))?;
                        Ok(ToolOutput {
                            content: aw_tools::filesystem::direct::project_read_text(
                                page.data,
                                Some(&status),
                                page.next.map(|c| (c.line, c.byte)),
                                a.numbered,
                            ),
                            is_error: page.io_fault,
                            truncated: true,
                        })
                    }
                    Operation::Write => {
                        let a: WriteArgs = parse(&value)?;
                        if !a.create_only && a.expected_sha256.is_none() {
                            let content_path = crate::content_path(&root, &a.path, true)?;
                            let outcome = aw_tools::filesystem::operations::write_text(
                                root.as_ref(),
                                &content_path,
                                &a.content,
                                limits.schema.text_write_bytes,
                                &|| ctx.cancellation.is_cancelled(),
                            )
                            .map_err(crate::file_operation_error)?;
                            Ok(ToolOutput::text(outcome.as_str()))
                        } else {
                            // Legacy conditional checks retain their pinned
                            // preimage semantics until core offers them too.
                            let content_path = if a.create_only {
                                a.path.clone()
                            } else {
                                crate::content_path(&root, &a.path, false)?
                            };
                            let changed = crate::mutate::write(
                                &root,
                                &content_path,
                                a.content.as_bytes(),
                                a.create_only,
                                a.expected_sha256.as_deref(),
                                &|| ctx.cancellation.is_cancelled(),
                            )?;
                            Ok(ToolOutput::text(aw_tools::mutation::compact_success(
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
