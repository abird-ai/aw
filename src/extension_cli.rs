//! Explicit process extension discovery and data-only template rendering.
//! Does not construct an Agent or issue host grants.
use aw::{terminal, Error, Result};
use std::time::Duration;

pub(crate) fn inspect(
    options: aw_extensions::Options,
    action: Option<aw_extensions::Action>,
    json: bool,
) -> Result<u8> {
    if options.is_empty() {
        return Err(Error::Config(
            "select --extension-process ID=/absolute/executable explicitly".into(),
        ));
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Error::Storage("cannot start explicit extension inspection runtime"))?;
    match action {
        Some(aw_extensions::Action::Render {
            server,
            prompt,
            args,
        }) => {
            let data = rt
                .block_on(async {
                    tokio::time::timeout(
                        Duration::from_secs(30),
                        aw_extensions::render(&options, &server, &prompt, &args),
                    )
                    .await
                })
                .map_err(|_| Error::Timeout)??;
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&data)
                        .map_err(|_| Error::Protocol("cannot serialize extension prompt"))?
                );
            } else {
                println!("{}", terminal::safe_text(&data));
            }
        }
        None => {
            let rows = rt
                .block_on(async {
                    tokio::time::timeout(Duration::from_secs(30), aw_extensions::inspect(&options))
                        .await
                })
                .map_err(|_| Error::Timeout)??;
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&rows)
                        .map_err(|_| Error::Protocol("cannot serialize extension descriptors"))?
                );
            } else {
                for item in rows {
                    println!(
                        "{}\t{}\t{}\ttools:{}\thooks:{}\tprompts:{}",
                        terminal::safe_text(&item.id),
                        terminal::safe_text(&item.name),
                        terminal::safe_text(&item.revision),
                        terminal::safe_text(&item.tools.join(",")),
                        terminal::safe_text(&item.hooks.join(",")),
                        terminal::safe_text(&item.prompts.join(",")),
                    );
                }
            }
        }
    }
    Ok(0)
}
