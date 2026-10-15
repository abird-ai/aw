//! Shared typed CLI model/catalog verbs for both std minimal and full builds.
//! Query/list are offline by default; refresh is the only network path here.
use aw::{
    catalog::{parse_usd_per_mtok, Cache, ModelInfo, PriceOrigin, Pricing, View},
    config::{self, Profile},
    terminal, Error, Result,
};
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Args, Debug)]
pub(crate) struct Scope {
    /// Explicit saved provider profile; defaults to current configured profile.
    #[arg(short = 'p', long, env = "AW_PROFILE")]
    profile: Option<String>,
    /// Explicit aw config file; never search a project config implicitly.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Write structured JSON instead of concise table output.
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Action {
    /// List configured and cached models; strictly offline unless --refresh.
    List {
        #[command(flatten)]
        scope: Scope,
        /// Explicitly contact the selected provider and refresh local metadata.
        #[arg(long)]
        refresh: bool,
    },
    /// Show metadata for a single model, including unknown fields.
    Show {
        model: String,
        #[command(flatten)]
        scope: Scope,
    },
    /// Explicitly refresh this provider's discovery cache over the network.
    Refresh {
        #[command(flatten)]
        scope: Scope,
    },
    /// Set, inspect, remove or estimate user-supplied API rates (USD/MTok).
    Price {
        #[command(subcommand)]
        action: PriceAction,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum PriceAction {
    /// Inspect known provider rates or user overrides; unknown isn't zero.
    Show {
        model: String,
        #[command(flatten)]
        scope: Scope,
    },
    /// Explicit per-endpoint model rate override; no billed-cost claims.
    Set {
        model: String,
        /// USD per one million ordinary input tokens, as a plain decimal.
        #[arg(long = "input-usd-per-mtok")]
        input: String,
        /// USD per one million output tokens, as a plain decimal.
        #[arg(long = "output-usd-per-mtok")]
        output: String,
        /// Optional USD per million cache-read input tokens.
        #[arg(long = "cache-read-usd-per-mtok")]
        cache_read: Option<String>,
        /// Optional USD per million cache-write input tokens.
        #[arg(long = "cache-write-usd-per-mtok")]
        cache_write: Option<String>,
        #[command(flatten)]
        scope: Scope,
    },
    /// Remove a user override; discovered rate (if any) becomes visible again.
    Remove {
        model: String,
        #[command(flatten)]
        scope: Scope,
    },
    /// Estimate cost from known rates, never an invoice or quota remaining.
    Estimate {
        model: String,
        #[arg(long = "input-tokens")]
        input: u64,
        #[arg(long = "output-tokens")]
        output: u64,
        #[arg(long = "cache-read-tokens", default_value_t = 0)]
        cache_read: u64,
        #[arg(long = "cache-write-tokens", default_value_t = 0)]
        cache_write: u64,
        #[command(flatten)]
        scope: Scope,
    },
}
#[derive(Subcommand, Debug)]
pub(crate) enum TopCommand {
    /// Inspect the exact compiled flag names and categories.
    Cli {
        #[command(subcommand)]
        action: crate::cli_taxonomy::Action,
    },
    /// Versioned local headless Agent RPC over stdio. Other Agent flags precede rpc.
    Rpc {
        /// Use strict JSONL-only stdin/stdout. This is not a TCP server.
        #[arg(long, required = true)]
        stdio: bool,
    },
    /// Inspect explicitly selected MCP servers; never grant tools or call a model.
    Mcp {
        #[command(flatten)]
        options: aw_mcp::Options,
        #[command(subcommand)]
        action: Option<aw_mcp::DataAction>,
        #[arg(long)]
        json: bool,
    },
    /// Explicitly inspect process extensions; registration never grants execution.
    Extension {
        #[command(flatten)]
        options: aw_extensions::Options,
        #[command(subcommand)]
        action: Option<aw_extensions::Action>,
        #[arg(long)]
        json: bool,
    },
    /// Inspect/discover models, capabilities and user-priced estimates.
    Model {
        #[command(subcommand)]
        command: Action,
    },
}
fn profile(scope: &Scope) -> Result<(String, Profile)> {
    let file = config::load(scope.config.as_deref())?;
    let profiles = {
        let mut providers = config::builtins();
        providers.extend(file.profiles);
        providers
    };
    let id = scope
        .profile
        .clone()
        .or(file.default_profile)
        .unwrap_or_else(|| "ollama".into());
    let selected = profiles
        .get(&id)
        .cloned()
        .ok_or_else(|| Error::Config(format!("unknown provider profile '{id}'")))?;
    selected.validate()?;
    Ok((id, selected))
}
fn catalog(cache: &Cache, profile: &Profile) -> Result<View> {
    cache.view(profile)
}
fn fmt_models(name: &str, view: &View, json: bool) -> Result<()> {
    if json {
        let result = serde_json::json!({
            "profile":name, "provider":view.provider, "endpoint":view.endpoint,
            "fetched_at":view.fetched_at, "fresh":view.fresh, "models":view.models
        });
        println!("{}", result);
        return Ok(());
    }
    println!(
        "{} · {} · {}",
        terminal::safe_text(name),
        terminal::safe_text(&view.provider),
        if view.fetched_at.is_none() {
            "configured only"
        } else if view.fresh {
            "fresh cache"
        } else {
            "stale cache (offline)"
        },
    );
    if view.models.is_empty() {
        println!("no models recorded; run aw model refresh to discover explicitly");
    } else {
        println!("reference candidates and discovered metadata are not account permission grants");
    }
    for m in &view.models {
        let in_tokens = m
            .input_tokens
            .or(m.context_tokens)
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".into());
        let output = m
            .output_tokens
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".into());
        let price = m
            .pricing
            .as_ref()
            .map(|p| match p.origin {
                PriceOrigin::UserOverride => "user price",
                PriceOrigin::ProviderReported => "provider-reported price",
            })
            .unwrap_or("unknown price");
        println!(
            "{}\tcontext {}\toutput {}\t{}\t{:?}",
            terminal::safe_text(&m.id),
            in_tokens,
            output,
            price,
            match m.origin {
                aw::catalog::ModelOrigin::BundledReference => "bundled reference",
                aw::catalog::ModelOrigin::Configured => "configured",
                aw::catalog::ModelOrigin::ProviderDiscovery => "provider-discovered",
            },
        );
    }
    Ok(())
}
fn print_single(model: &ModelInfo, scope: &Scope) -> Result<()> {
    if scope.json {
        println!(
            "{}",
            serde_json::to_string(model)
                .map_err(|_| Error::Config("unable to serialize model metadata".into()))?
        );
    } else {
        println!("model: {}", terminal::safe_text(&model.id));
        println!("source: {:?}", model.origin);
        if let Some(date) = &model.reference_as_of {
            println!("bundled reference as of: {date} (not verified for this account)");
        }
        println!(
            "input/context: {}",
            model
                .input_tokens
                .or(model.context_tokens)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".into())
        );
        println!(
            "max output: {}",
            model
                .output_tokens
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".into())
        );
        println!(
            "tools: {}",
            model
                .supports_tools
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".into())
        );
        println!(
            "thinking: {}",
            model
                .supports_thinking
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".into())
        );
        if let Some(price) = &model.pricing {
            println!(
                "rates: {} / {} microUSD per million input/output tokens ({:?})",
                price.input_micro_usd_per_mtok, price.output_micro_usd_per_mtok, price.origin
            );
        } else {
            println!("rates: unknown");
        }
    }
    Ok(())
}
fn get<'a>(view: &'a View, model: &str) -> Result<&'a ModelInfo> {
    view.models.iter().find(|m| m.id == model)
        .ok_or_else(|| Error::Config("model is not configured or in the cached catalog; explicitly refresh or enter it as a profile model".into()))
}
fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Error::Config("unable to create discovery runtime".into()))
}
pub(crate) fn run(command: Action) -> Result<u8> {
    match command {
        Action::List { scope, refresh } => {
            let (name, profile) = profile(&scope)?;
            let cache = Cache::default_user()?;
            let view = if refresh {
                runtime()?.block_on(cache.refresh(&profile))?
            } else {
                catalog(&cache, &profile)?
            };
            fmt_models(&name, &view, scope.json)?;
        }
        Action::Refresh { scope } => {
            let (name, profile) = profile(&scope)?;
            let cache = Cache::default_user()?;
            let view = runtime()?.block_on(cache.refresh(&profile))?;
            fmt_models(&name, &view, scope.json)?;
        }
        Action::Show { model, scope } => {
            config::validate_model(&model)?;
            let (_, profile) = profile(&scope)?;
            let view = catalog(&Cache::default_user()?, &profile)?;
            print_single(get(&view, &model)?, &scope)?;
        }
        Action::Price { action } => match action {
            PriceAction::Show { model, scope } => {
                config::validate_model(&model)?;
                let (_, profile) = profile(&scope)?;
                let cache = Cache::default_user()?;
                let view = catalog(&cache, &profile)?;
                let item = if let Some(info) = view.models.iter().find(|m| m.id == model) {
                    info.clone()
                } else {
                    let mut info = ModelInfo::configured(model);
                    info.pricing = cache.price(&profile, &info.id)?;
                    info
                };
                print_single(&item, &scope)?;
            }
            PriceAction::Set {
                model,
                input,
                output,
                cache_read,
                cache_write,
                scope,
            } => {
                let (_, profile) = profile(&scope)?;
                let price = Pricing {
                    input_micro_usd_per_mtok: parse_usd_per_mtok(&input)?,
                    output_micro_usd_per_mtok: parse_usd_per_mtok(&output)?,
                    cache_read_micro_usd_per_mtok: cache_read
                        .as_deref()
                        .map(parse_usd_per_mtok)
                        .transpose()?,
                    cache_write_micro_usd_per_mtok: cache_write
                        .as_deref()
                        .map(parse_usd_per_mtok)
                        .transpose()?,
                    origin: PriceOrigin::UserOverride,
                };
                Cache::default_user()?.set_price(&profile, &model, price.clone())?;
                if scope.json {
                    println!(
                        "{}",
                        serde_json::json!({"model":model,"rate":price,"status":"user_override_saved"})
                    );
                } else {
                    println!(
                        "saved user-supplied price for {} (not a provider invoice)",
                        terminal::safe_text(&model)
                    );
                }
            }
            PriceAction::Remove { model, scope } => {
                let (_, profile) = profile(&scope)?;
                Cache::default_user()?.remove_price(&profile, &model)?;
                if scope.json {
                    println!(
                        "{}",
                        serde_json::json!({"model":model,"status":"user_override_removed"})
                    );
                } else {
                    println!("removed user override for {}", terminal::safe_text(&model));
                }
            }
            PriceAction::Estimate {
                model,
                input,
                output,
                cache_read,
                cache_write,
                scope,
            } => {
                config::validate_model(&model)?;
                let (_, profile) = profile(&scope)?;
                let cache = Cache::default_user()?;
                let view = catalog(&cache, &profile)?;
                let price = match view.models.iter().find(|m| m.id == model) {
                    Some(info) => info.pricing.clone(),
                    None => cache.price(&profile, &model)?,
                };
                let amount = price.as_ref().and_then(|rate| {
                    rate.estimate_with_cache_micro_usd(input, output, cache_read, cache_write)
                });
                if scope.json {
                    println!(
                        "{}",
                        serde_json::json!({"model":model,"estimated_micro_usd":amount,
                        "input_tokens":input,"output_tokens":output,
                        "cache_read_tokens":cache_read,"cache_write_tokens":cache_write,
                        "warning":"estimate from user or provider-reported rate, not an invoice"})
                    );
                } else if let Some(amount) = amount {
                    println!(
                        "{}: approximately {} microUSD (rate estimate, not billed cost)",
                        terminal::safe_text(&model),
                        amount
                    );
                } else {
                    println!(
                        "{}: cost unknown (missing required input/output/cache rate)",
                        terminal::safe_text(&model)
                    );
                }
            }
        },
    }
    Ok(0)
}
