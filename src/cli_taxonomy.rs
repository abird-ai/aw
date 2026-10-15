//! Complete one-way canonical taxonomy over the ORIGINAL typed Clap command
//! tree. The original derives still own every value parser, default, conflict,
//! short, env key, feature condition, and executable/authorization behavior.
//!
//! This projection changes ONLY canonical long-option spelling and grouped
//! help; old aliases are NOT accepted or shipped.
//! No independent argv lexer, provider selector, feature gate or tool grant.
use clap::{Arg, Command, CommandFactory, FromArgMatches};
use serde::Serialize;
use std::ffi::OsString;

#[derive(Clone, Debug, Serialize)]
pub struct FlagEntry {
    pub command: String,
    pub id: String,
    pub short: Option<String>,
    pub canonical: String,
    pub heading: String,
    /// Name of the environmental input, never its value.
    pub env_name: Option<String>,
    /// Literal Clap schema defaults, not values from private config or env.
    pub default_values: Vec<String>,
    pub takes_value: bool,
    pub hidden: bool,
    pub required: bool,
    pub value_names: Vec<String>,
    pub possible_values: Vec<String>,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Schema {
    pub schema: u32,
    pub source: &'static str,
    pub feature_cut: &'static str,
    pub flags: Vec<FlagEntry>,
    pub commands: Vec<String>,
    pub precedence: &'static str,
    pub boundary: &'static str,
    pub counts: Counts,
}

#[derive(Clone, Debug, Serialize)]
pub struct Counts {
    pub command_paths: usize,
    pub flags: usize,
    pub categories: usize,
}

pub fn inventory(cmd: &Command) -> Vec<FlagEntry> {
    let mut found = Vec::new();
    collect(cmd, String::from(cmd.get_name()), &mut found);
    found
}

fn collect(cmd: &Command, path: String, out: &mut Vec<FlagEntry>) {
    for arg in cmd.get_arguments() {
        if let Some(long) = arg.get_long() {
            out.push(FlagEntry {
                command: path.clone(),
                id: arg.get_id().to_string(),
                short: arg.get_short().map(|c| format!("-{c}")),
                canonical: format!("--{long}"),
                heading: arg.get_help_heading().unwrap_or("Options").to_owned(),
                env_name: arg
                    .get_env()
                    .map(|name| name.to_string_lossy().into_owned()),
                default_values: arg
                    .get_default_values()
                    .iter()
                    .map(|value| value.to_string_lossy().into_owned())
                    .collect(),
                takes_value: arg.get_action().takes_values(),
                hidden: arg.is_hide_set(),
                required: arg.is_required_set(),
                value_names: arg
                    .get_value_names()
                    .unwrap_or(&[])
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                possible_values: arg
                    .get_possible_values()
                    .iter()
                    .map(|v| v.get_name().to_owned())
                    .collect(),
                description: arg.get_help().map(ToString::to_string),
            });
        }
    }
    for sub in cmd.get_subcommands() {
        collect(sub, format!("{path} {}", sub.get_name()), out);
    }
}

const PREFIXES: &[(&str, &str)] = &[
    ("resources", "Project resources & trust"),
    ("mcp", "MCP servers & grants"),
    ("extension", "Extensions & hooks"),
    ("tools", "Tools & execution"),
    ("subagent", "Subagents & delegation"),
    ("planner", "Planner & reviews"),
    ("guardian", "Planner & reviews"),
    ("provider", "Provider & model"),
    ("model", "Provider & model"),
    ("account", "Account & security"),
    ("auth", "Account & security"),
    ("security", "Account & security"),
    ("oauth", "Account & security"),
    ("oidc", "Account & security"),
    ("config", "Configuration"),
    ("context", "Context & compaction"),
    ("state", "Session & state"),
    ("session", "Session & state"),
    ("agent", "Agent & instructions"),
    ("skill", "Agent & instructions"),
    ("workspace", "Workspace & target"),
    ("domain", "Workspace & target"),
    ("machine", "Workspace & target"),
    ("fleet", "Workspace & target"),
    ("net", "Network & devices"),
    ("device", "Network & devices"),
    ("server", "Web & server"),
    ("web", "Web & server"),
    ("ui", "Interface & output"),
    ("tui", "Interface & output"),
    ("output", "Interface & output"),
    ("diagnostics", "Diagnostics & validation"),
    ("validation", "Diagnostics & validation"),
    ("identity", "Account & security"),
    ("pipeline", "Automation & routing"),
    ("schedule", "Automation & routing"),
    ("route", "Automation & routing"),
    ("team", "Automation & routing"),
    ("review", "Automation & routing"),
    ("attach", "Workspace & target"),
];

fn heading(long: &str) -> Option<&'static str> {
    PREFIXES.iter().find_map(|(prefix, heading)| {
        long.strip_prefix(prefix)
            .filter(|tail| tail.is_empty() || tail.starts_with('-'))
            .map(|_| *heading)
    })
}

/// The few former unscoped root names that have a stable domain regardless of
/// which full/minimal command reuses them. Short flags stay exactly unchanged.
fn explicit(old: &str, command: &str) -> Option<&'static str> {
    let top = command.split_whitespace().nth(1).unwrap_or("");
    Some(match old {
        "profile" => "provider-profile",
        "model" => "model-id",
        "provider" => "provider-kind",
        "base-url" => "provider-base-url",
        "api-key-env" => "auth-api-key-env",
        "allow-insecure-http" => "security-allow-insecure-http",
        "config" => "config-file",
        "shared-profile" => "provider-shared-profile",
        "system" => "agent-system",
        "instructions" => "agent-instructions",
        "skill" => "agent-skill",
        "max-tokens" => "model-max-tokens",
        "idle-timeout" => "provider-idle-timeout",
        "list-profiles" => "provider-list-profiles",
        "workspace" => "workspace-id",
        "fleet" => "fleet-id",
        "route" => "route-id",
        "directory" => "workspace-directory",
        "domain" => "domain-id",
        "on" => "workspace-target",
        "account" => "account-id",
        "no-state" => "state-memory-only",
        "store-backend" => "state-backend",
        "session" => "session-id",
        "resume" => "session-resume",
        "list-sessions" => "session-list",
        "branch" => "session-branch",
        "allow-context-transfer" => "context-allow-transfer",
        "mode" => "ui-mode",
        "output" if command == "aw session export" => "output-file",
        "output" => "output-format",
        "print" => "output-print",
        "json" => "output-json",
        "format" => "output-format",
        "system-prompt" => "agent-system-prompt",
        "list" => "output-list",
        "web" => "web-open",
        "serve-web" => "web-serve",
        "web-workspace" => "web-workspace",
        "allow-web-tools" => "web-allow-tools",
        "allow-web-terminals" => "web-allow-terminals",
        "socket" => "server-socket",
        "max-context-bytes" => "context-max-bytes",
        "max-output-bytes" => "output-max-bytes",
        "max-total-output-bytes" => "output-max-total-bytes",
        "max-output-tokens" => "model-max-output-tokens",
        "output-tokens" => "model-output-tokens",
        "input-tokens" => "model-input-tokens",
        "input-usd-per-mtok" => "model-input-usd-per-mtok",
        "output-usd-per-mtok" => "model-output-usd-per-mtok",
        "cache-read-tokens" => "model-cache-read-tokens",
        "cache-write-tokens" => "model-cache-write-tokens",
        "cache-read-usd-per-mtok" => "model-cache-read-usd-per-mtok",
        "cache-write-usd-per-mtok" => "model-cache-write-usd-per-mtok",
        "subagents" => "subagent-enabled",
        "title"
            if top == "session"
                || top.is_empty()
                || top == "agent"
                || top == "web"
                || top == "rpc" =>
        {
            "session-title"
        }
        "timeout" if top.is_empty() || matches!(top, "agent" | "web" | "rpc") => "provider-timeout",
        "listen" if matches!(top, "web" | "agent") || top.is_empty() => "web-listen",
        "refresh" if top == "model" => "model-refresh",
        "seconds" if top == "schedule" => "schedule-seconds",
        "client-secret-file" => "auth-client-secret-file",
        "client-id" => "auth-client-id",
        "client-auth" => "auth-client-auth",
        "authorization-endpoint" => "auth-authorization-endpoint",
        "token-endpoint" => "auth-token-endpoint",
        "issuer" => "auth-issuer",
        "secret-file" => "auth-secret-file",
        "key-file" => "auth-key-file",
        "key" if top == "auth" => "auth-key",
        "actor" => "identity-actor",
        _ => return None,
    })
}

fn scope(path: &str) -> &'static str {
    match path.split_whitespace().nth(1).unwrap_or("agent") {
        "model" => "model",
        "mcp" => "mcp",
        "extension" => "extension",
        "skill" => "skill",
        "session" => "session",
        "subagent" => "subagent",
        "workspace" => "workspace",
        "domain" => "domain",
        "machine" => "machine",
        "fleet" => "fleet",
        "identity" => "identity",
        "context" => "context",
        "auth" | "oidc" | "oidc-provider" | "oauth-connector" => "auth",
        "provider-account" => "account",
        "pipeline" => "pipeline",
        "schedule" => "schedule",
        "team-template" | "team" => "team",
        "review" => "review",
        "route" => "route",
        "net" => "net",
        "device" => "device",
        "attach" => "attach",
        "web" => "web",
        "agent" => "agent",
        "rpc" => "agent",
        _ => "agent",
    }
}

fn canonical(long: &str, path: &str) -> String {
    if long.starts_with("__") {
        // Original test-only failure-injection flags remain deliberately
        // undocumented/internal, with no public migration aliases.
        return long.to_owned();
    }
    if let Some(name) = explicit(long, path) {
        return name.into();
    }
    if long.starts_with("remote-control-") {
        return format!("net-{long}");
    }
    if heading(long).is_some() {
        return long.to_owned();
    }
    format!("{}-{long}", scope(path))
}

/// One recursive projection across the actual original Clap Args and nested
/// subcommands; all runtime value parsers/grants/defaults remain on their ID.
fn normalize_rec(command: Command, path: String) -> Command {
    let action_path = path.clone();
    command
        .mut_args(move |arg| normalize_arg(arg, &action_path))
        .mut_subcommands(move |sub| {
            let path = format!("{path} {}", sub.get_name());
            normalize_rec(sub, path)
        })
}

fn normalize_arg(mut arg: Arg, path: &str) -> Arg {
    // R14.5 deliberately has no legacy aliases. Fail closed if a future
    // flattened/nested Clap Args source attempts to silently reintroduce
    // any long-option aliases; this covers ALL 1,685 full option occurrences,
    // not just the representative negative executable tests.
    assert!(
        arg.get_all_aliases()
            .is_none_or(|aliases| aliases.is_empty()),
        "canonical-only aw must not admit long-flag aliases",
    );
    let Some(old) = arg.get_long().map(str::to_owned) else {
        return arg;
    };
    let next = canonical(&old, path);
    let group = heading(&next).unwrap_or("Diagnostics & validation");
    if next != old {
        // The exact same source-defined typed Arg ID, parser, env and grant
        // policy remain authoritative. No deprecated spelling is registered.
        arg = arg.long(next);
    }
    arg.help_heading(group)
}

pub fn normalize(command: Command) -> Command {
    normalize_rec(command, "aw".into())
}

/// Parse only the transformed original typed Arg IDs. No duplicate command
/// parser or independent validator exists: Clap's original conflicts, value
/// parsers, env variables and defaults remain authoritative.
/// Full trees can exceed platform-default CLI thread stacks while deriving
/// 239 deeply nested command paths. Do all full-tree construction/parsing on
/// a bounded 32-MiB-stack worker; no provider, file or tool work is moved
/// there. The resulting typed Cli is handed back to the original main thread.
/// The small minimal/stateless feature cuts keep direct zero-extra-thread parse.
#[cfg(feature = "durable")]
const FULL_SCHEMA_STACK: usize = 32 * 1024 * 1024;

fn typed_parse<T: CommandFactory + FromArgMatches>(values: Vec<OsString>) -> T {
    let mut command = normalize(T::command());
    let mut matches = command
        .try_get_matches_from_mut(values)
        .unwrap_or_else(|error| error.exit());
    T::from_arg_matches_mut(&mut matches).unwrap_or_else(|error| error.exit())
}

pub fn parse<T: CommandFactory + FromArgMatches + Send + 'static>(
    values: impl IntoIterator<Item = impl Into<OsString> + Clone>,
) -> T {
    let values = values
        .into_iter()
        .map(Into::into)
        .collect::<Vec<OsString>>();
    #[cfg(feature = "durable")]
    {
        let worker = std::thread::Builder::new()
            .name("aw-cli-parse".into())
            .stack_size(FULL_SCHEMA_STACK)
            .spawn(move || typed_parse::<T>(values))
            .unwrap_or_else(|_| {
                eprintln!("aw: cannot allocate bounded full CLI parser stack");
                std::process::exit(2)
            });
        worker.join().unwrap_or_else(|_| {
            eprintln!("aw: full CLI parser stopped unexpectedly");
            std::process::exit(2)
        })
    }
    #[cfg(not(feature = "durable"))]
    {
        typed_parse::<T>(values)
    }
}

/// Construct the ORIGINAL typed command tree off a larger stack when full
/// is compiled. The offline 'cli schema' command needs this protection too.
pub fn command_tree<T: CommandFactory + 'static>() -> Command {
    #[cfg(feature = "durable")]
    {
        let worker = std::thread::Builder::new()
            .name("aw-cli-schema".into())
            .stack_size(FULL_SCHEMA_STACK)
            .spawn(T::command)
            .unwrap_or_else(|_| {
                eprintln!("aw: cannot allocate bounded CLI schema stack");
                std::process::exit(2)
            });
        worker.join().unwrap_or_else(|_| {
            eprintln!("aw: CLI schema builder stopped unexpectedly");
            std::process::exit(2)
        })
    }
    #[cfg(not(feature = "durable"))]
    {
        T::command()
    }
}

fn command_paths(cmd: &Command, path: String, out: &mut Vec<String>) {
    out.push(path.clone());
    for sub in cmd.get_subcommands() {
        command_paths(sub, format!("{path} {}", sub.get_name()), out);
    }
}

pub fn schema(cmd: Command) -> Schema {
    let original = inventory(&cmd);
    let normalized = normalize(cmd);
    let mut flags = inventory(&normalized);
    let mut paths = Vec::new();
    command_paths(&normalized, "aw".into(), &mut paths);
    assert_eq!(
        flags.len(),
        original.len(),
        "taxonomy cannot drop an argument"
    );
    for (flag, prior) in flags.iter_mut().zip(original.iter()) {
        assert_eq!(
            (flag.command.as_str(), flag.id.as_str()),
            (prior.command.as_str(), prior.id.as_str()),
            "canonical projection must retain exact typed Arg ID and command path"
        );
        assert_eq!(flag.short, prior.short, "short aliases cannot change");
        assert_eq!(
            flag.env_name, prior.env_name,
            "environmental authority must be unchanged"
        );
        assert_eq!(
            flag.default_values, prior.default_values,
            "CLI defaults cannot change"
        );
        assert_eq!(
            flag.takes_value, prior.takes_value,
            "value action cannot change"
        );
        assert_eq!(flag.required, prior.required, "requiredness cannot change");
        assert_eq!(
            flag.possible_values, prior.possible_values,
            "allowed values cannot change"
        );
    }
    let headings = flags
        .iter()
        .map(|f| f.heading.clone())
        .collect::<std::collections::BTreeSet<_>>();
    Schema {
        schema: 2,
        source: "clap::CommandFactory/FromArgMatches (one typed schema)",
        feature_cut: if cfg!(feature = "durable") {
            "full"
        } else if cfg!(feature = "jsonl") {
            "minimal"
        } else {
            "stateless"
        },
        precedence: "unchanged: derived Clap explicit flags > declared env keys > existing config/profile defaults; no runtime custody/grant changes",
        boundary: "names and grouped help only; original typed IDs, parsers, value ranges, conflicts, short flags, feature cuts and effect grants retained",
        counts: Counts {
            command_paths: paths.len(),
            flags: flags.len(),
            categories: headings.len(),
        },
        flags,
        commands: paths,
    }
}

/// Explicit inspect-only CLI subcommands. Never loads config, initializes
/// a model/owner, launches a tool, or consults the environment for grants.
#[derive(Clone, Debug, clap::Subcommand)]
pub enum Action {
    /// Export every supported canonical flag, original typed argument ID,
    /// short spelling, help heading and compiled feature cut as JSON.
    Schema,
}

pub fn run(action: Action, command: Command) -> aw::Result<u8> {
    match action {
        Action::Schema => {
            let value = schema(command);
            println!(
                "{}",
                serde_json::to_string(&value)
                    .map_err(|_| aw::Error::Protocol("failed to serialize typed CLI schema"))?
            );
            Ok(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{Arg, ArgAction};

    fn sample() -> Command {
        Command::new("aw")
            .arg(
                Arg::new("profile")
                    .long("profile")
                    .short('p')
                    .env("AW_R14_DO_NOT_USE")
                    .action(ArgAction::Set),
            )
            .arg(
                Arg::new("model")
                    .long("model")
                    .short('m')
                    .action(ArgAction::Set),
            )
            .arg(
                Arg::new("tools")
                    .long("tools-mode")
                    .default_value("off")
                    .action(ArgAction::Set),
            )
    }

    #[test]
    fn only_canonical_long_options_are_accepted() {
        let command = normalize(sample());
        let parsed = command
            .clone()
            .try_get_matches_from([
                "aw",
                "--provider-profile",
                "llama",
                "--model-id",
                "one",
                "--tools-mode",
                "off",
            ])
            .unwrap();
        assert_eq!(
            parsed.get_one::<String>("profile").map(String::as_str),
            Some("llama")
        );
        assert_eq!(
            parsed.get_one::<String>("model").map(String::as_str),
            Some("one")
        );
        for legacy in ["--profile", "--model", "--tools"] {
            let bad = command
                .clone()
                .try_get_matches_from(["aw", legacy, "value"]);
            assert!(bad.is_err(), "legacy spelling {legacy} must be removed");
        }
        assert_eq!(
            command
                .get_arguments()
                .filter(|a| a.get_all_aliases().is_some_and(|x| !x.is_empty()))
                .count(),
            0
        );
    }
    #[test]
    fn schema_exposes_only_supported_flags_and_retains_original_typed_metadata() {
        let doc = schema(sample());
        assert_eq!(doc.schema, 2);
        assert_eq!(doc.counts.flags, 3);
        assert_eq!(doc.counts.command_paths, 1);
        let profile = &doc.flags[0];
        assert_eq!(profile.canonical, "--provider-profile");
        assert_eq!(profile.id, "profile");
        assert_eq!(profile.short.as_deref(), Some("-p"));
        assert_eq!(profile.env_name.as_deref(), Some("AW_R14_DO_NOT_USE"));
        assert_eq!(doc.flags[2].default_values, vec!["off"]);
        assert_eq!(doc.flags[2].canonical, "--tools-mode");
        let json = serde_json::to_string(&doc).unwrap();
        for old_field in [
            "before",
            "old_aliases",
            "deprecated_aliases",
            "removed_aliases",
            "cli_mode",
        ] {
            assert!(
                !json.contains(old_field),
                "no historical migration data in current schema"
            );
        }
    }
    #[test]
    fn contextual_canonical_names_do_not_affect_effect_scope() {
        assert_eq!(canonical("timeout", "aw"), "provider-timeout");
        assert_eq!(canonical("timeout", "aw net pair"), "net-timeout");
        assert_eq!(canonical("output", "aw session export"), "output-file");
        assert_eq!(canonical("format", "aw session export"), "output-format");
        assert_eq!(
            canonical("remote-control-grant", "aw agent"),
            "net-remote-control-grant"
        );
        assert_eq!(heading("tools-parallel"), Some("Tools & execution"));
    }
}
