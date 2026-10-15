//! Explicit MCP discovery and data access; never auto-admit remote content.
use aw::{terminal, Error, Result};
use std::time::Duration;

pub(crate) fn inspect(
    options: aw_mcp::Options,
    action: Option<aw_mcp::DataAction>,
    json: bool,
) -> Result<u8> {
    if options.is_empty() {
        return Err(Error::Config(
            "choose --mcp-stdio SERVER=/ABS/EXEC, --mcp-http SERVER=URL or --mcp-sse SERVER=URL"
                .into(),
        ));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Error::Storage("cannot initialize MCP inspection runtime"))?;
    if let Some(action) = action {
        let rows = runtime.block_on(async {
            // Watch uses a user-specified bounded duration; all other actions
            // share a 45s outer deadline. Neither creates a model Agent.
            tokio::time::timeout(
                Duration::from_secs(65),
                aw_mcp::inspect_data(&options, &action),
            )
            .await
            .map_err(|_| Error::Timeout)?
        })?;
        if json {
            println!(
                "{}",
                serde_json::to_string(&rows)
                    .map_err(|_| Error::Protocol("MCP output serialization failed"))?
            );
        } else {
            for row in rows {
                let variables = row
                    .arguments
                    .as_ref()
                    .map(|arguments| {
                        arguments
                            .iter()
                            .map(|a| {
                                let suffix = if a.required { "*" } else { "" };
                                format!("{}{}", terminal::safe_text(&a.name), suffix)
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .map(|names| format!("\targs={names}"))
                    .unwrap_or_default();
                println!(
                    "{}\t{}\t{}{}{}{}",
                    terminal::safe_text(&row.server),
                    terminal::safe_text(&row.kind),
                    terminal::safe_text(&row.name),
                    variables,
                    row.uri
                        .as_deref()
                        .map(|u| format!("\t{}", terminal::safe_text(u)))
                        .unwrap_or_default(),
                    row.text
                        .as_deref()
                        .map(|t| format!("\n{}", terminal::safe_text(t)))
                        .unwrap_or_default(),
                );
            }
        }
        return Ok(0);
    }
    let discovered = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(45), aw_mcp::inspect(&options))
            .await
            .map_err(|_| Error::Timeout)?
    })?;
    if json {
        println!(
            "{}",
            serde_json::to_string(&discovered)
                .map_err(|_| Error::Protocol("MCP output serialization failed"))?
        );
    } else {
        for item in discovered {
            println!(
                "{}\tunknown effect\t{}",
                terminal::safe_text(&item.name),
                terminal::safe_text(&item.description)
            );
        }
    }
    Ok(0)
}
