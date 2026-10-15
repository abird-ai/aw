mod cli_taxonomy;
mod config_persistence;
mod extension_cli;
mod mcp_cli;
mod model_cli;
mod skills;
#[cfg(test)]
mod test_support;

#[cfg(feature = "durable")]
include!("main_full.rs");

#[cfg(not(feature = "durable"))]
include!("main_minimal.rs");
