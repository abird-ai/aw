use aw::{config, context::ExecutionContext, sessions, Error, Result};
use aw_store::SessionStore;
use aw_store_api::{SessionBackend, StoreCapability};
use clap::{Args as ClapArgs, Subcommand, ValueEnum};
#[cfg(any(feature = "net", feature = "oidc"))]
use std::time::Duration;
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    net::SocketAddr,
    path::PathBuf,
};
use zeroize::Zeroizing;

#[derive(Clone, Debug, ClapArgs)]
pub(crate) struct StoreArgs {
    /// Owned private aw state directory.
    #[arg(long, env = "AW_STATE_DIR")]
    state_dir: Option<PathBuf>,
    /// Select a Domain by opaque ID or display name where applicable.
    #[arg(long)]
    domain: Option<String>,
    /// Machine-readable output.
    #[arg(long)]
    json: bool,
}
fn store_dir(opts: &StoreArgs) -> Result<PathBuf> {
    opts.state_dir
        .clone()
        .map(Ok)
        .unwrap_or_else(sessions::default_directory)
}
fn store_and_actor(opts: &StoreArgs) -> Result<(SessionStore, aw::PrincipalId)> {
    let dir = store_dir(opts)?;
    let ctx = ExecutionContext::capture(std::path::Path::new("."))?;
    Ok((SessionStore::open(&dir)?, sessions::actor(&ctx, false)?))
}


#[derive(Clone, Debug, ClapArgs)]
pub(crate) struct ConfigArgs {
    /// Explicit aw configuration file; defaults to the normal user config.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Machine-readable output.
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum SkillCommand {
    /// List registered skills. Registry presence never activates a skill.
    List {
        #[command(flatten)]
        opts: ConfigArgs,
    },
    /// Show one registered skill definition.
    Show {
        name: String,
        #[command(flatten)]
        opts: ConfigArgs,
    },
    /// Register one skill source after validating the same Phase-60 admission bounds.
    Add {
        name: String,
        path: PathBuf,
        #[arg(long)]
        description: Option<String>,
        /// Explicitly replace an existing registry entry with this name.
        #[arg(long)]
        replace: bool,
        #[command(flatten)]
        opts: ConfigArgs,
    },
    /// Remove one registry entry. This does not alter already-running Agents.
    Remove {
        name: String,
        #[command(flatten)]
        opts: ConfigArgs,
    },
    /// Export one registered instruction skill as a portable versioned bundle.
    Export {
        name: String,
        output: PathBuf,
        #[arg(long)]
        replace: bool,
        #[command(flatten)]
        opts: ConfigArgs,
    },
    /// Verify a portable skill bundle without installing or activating it.
    Verify { bundle: PathBuf },
    /// Install/import a bundle into the local registry without activating it.
    #[command(alias = "import")]
    Install {
        bundle: PathBuf,
        #[arg(long)]
        replace: bool,
        #[command(flatten)]
        opts: ConfigArgs,
    },
    /// Update an existing registered skill from a valid bundle.
    Update {
        bundle: PathBuf,
        #[command(flatten)]
        opts: ConfigArgs,
    },
}

pub(crate) fn skill(command: SkillCommand) -> Result<u8> {
    fn load_config(opts: &ConfigArgs) -> Result<config::FileConfig> {
        if opts.config.as_ref().is_some_and(|path| !path.exists()) {
            return Ok(config::FileConfig::default());
        }
        config::load(opts.config.as_deref())
    }
    fn mutation_config_path(opts: &ConfigArgs) -> Result<PathBuf> {
        config::resolved_config_path(opts.config.as_deref())
    }
    match command {
        SkillCommand::List { opts } => {
            let cfg = load_config(&opts)?;
            print(&cfg.skills, opts.json)?;
        }
        SkillCommand::Show { name, opts } => {
            config::validate_skill_name(&name)?;
            let cfg = load_config(&opts)?;
            let skill = cfg
                .skills
                .get(&name)
                .ok_or_else(|| Error::Config(format!("unknown registered skill {name}")))?;
            print(skill, opts.json)?;
        }
        SkillCommand::Add { name, path, description, replace, opts } => {
            config::validate_skill_name(&name)?;
            if let Some(description) = &description {
                if description.len() > 512 || description.chars().any(char::is_control) {
                    return Err(Error::Config(
                        "skill description must be at most 512 bytes without control characters".into(),
                    ));
                }
            }
            let path = std::fs::canonicalize(path)?;
            let spec: crate::skills::SkillSpec = format!("{name}={}", path.display())
                .parse()
                .map_err(Error::Config)?;
            crate::skills::load(std::slice::from_ref(&spec), &std::collections::BTreeMap::new())?;
            let (cfg, ()) = config::mutate(opts.config.as_deref(), |cfg| {
                if cfg.skills.contains_key(&name) && !replace {
                    return Err(Error::Conflict(
                        "skill already registered; use --replace to change it",
                    ));
                }
                cfg.skills.insert(
                    name.clone(),
                    config::SkillConfig { path: path.clone(), description: description.clone() },
                );
                Ok(())
            })?;
            print(&cfg.skills[&name], opts.json)?;
        }
        SkillCommand::Remove { name, opts } => {
            config::validate_skill_name(&name)?;
            let (_cfg, removed) = config::mutate(opts.config.as_deref(), |cfg| {
                cfg.skills
                    .remove(&name)
                    .ok_or_else(|| Error::Config(format!("unknown registered skill {name}")))
            })?;
            print(&removed, opts.json)?;
        }
        SkillCommand::Export { name, output, replace, opts } => {
            config::validate_skill_name(&name)?;
            let cfg = load_config(&opts)?;
            let registered = cfg
                .skills
                .get(&name)
                .ok_or_else(|| Error::Config(format!("unknown registered skill {name}")))?;
            let spec: crate::skills::SkillSpec = name.parse().map_err(Error::Config)?;
            let loaded = crate::skills::load(std::slice::from_ref(&spec), &cfg.skills)?;
            let skill = loaded
                .first()
                .ok_or_else(|| Error::Config("registered skill did not load".into()))?;
            let bundle = crate::skill_package::SkillBundle::build(
                name,
                registered.description.clone(),
                skill.text().to_owned(),
            )?;
            let bytes = bundle.encode_pretty()?;
            crate::skill_package::write_new_or_replace(&output, &bytes, replace)?;
            print(&bundle, opts.json)?;
        }
        SkillCommand::Verify { bundle } => {
            let bundle = crate::skill_package::SkillBundle::read(&bundle)?;
            print(&bundle, true)?;
        }
        SkillCommand::Install { bundle, replace, opts } => {
            let bundle = crate::skill_package::SkillBundle::read(&bundle)?;
            let config_path = mutation_config_path(&opts)?;
            let (cfg, ()) = config::mutate(Some(&config_path), |cfg| {
                if cfg.skills.contains_key(&bundle.name) && !replace {
                    return Err(Error::Conflict(
                        "skill already registered; use --replace to install this bundle",
                    ));
                }
                let managed = crate::skill_package::publish_managed_content(&bundle, &config_path)?;
                cfg.skills.insert(
                    bundle.name.clone(),
                    config::SkillConfig { path: managed, description: bundle.description.clone() },
                );
                Ok(())
            })?;
            print(&cfg.skills[&bundle.name], opts.json)?;
        }
        SkillCommand::Update { bundle, opts } => {
            let bundle = crate::skill_package::SkillBundle::read(&bundle)?;
            let config_path = mutation_config_path(&opts)?;
            let (cfg, ()) = config::mutate(Some(&config_path), |cfg| {
                if !cfg.skills.contains_key(&bundle.name) {
                    return Err(Error::Config(format!("unknown registered skill {}", bundle.name)));
                }
                let managed = crate::skill_package::publish_managed_content(&bundle, &config_path)?;
                cfg.skills.insert(
                    bundle.name.clone(),
                    config::SkillConfig { path: managed, description: bundle.description.clone() },
                );
                Ok(())
            })?;
            print(&cfg.skills[&bundle.name], opts.json)?;
        }
    }
    Ok(0)
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub(crate) enum SessionStoreBackendArg {
    Sqlite,
    Jsonl,
}

#[derive(Clone, Debug, ClapArgs)]
pub(crate) struct SessionStoreArgs {
    /// Session backend. SQLite is the full durable store; JSONL is a lightweight
    /// single-owner transcript/journal backend with explicit limited capabilities.
    #[arg(long = "store-backend", value_enum, default_value_t = SessionStoreBackendArg::Sqlite)]
    backend: SessionStoreBackendArg,
    /// Owned private aw state directory.
    #[arg(long, env = "AW_STATE_DIR")]
    state_dir: Option<PathBuf>,
    /// Machine-readable output.
    #[arg(long)]
    json: bool,
}

enum SessionBackendHandle {
    Sqlite(SessionStore),
    #[cfg(feature = "jsonl")]
    Jsonl(aw_store_jsonl::JsonlStore),
}

impl SessionBackendHandle {
    fn backend(&self) -> &dyn SessionBackend {
        match self {
            Self::Sqlite(store) => store,
            #[cfg(feature = "jsonl")]
            Self::Jsonl(store) => store,
        }
    }

    fn sqlite(&self) -> Result<&SessionStore> {
        match self {
            Self::Sqlite(store) => Ok(store),
            #[cfg(feature = "jsonl")]
            Self::Jsonl(_) => Err(Error::Denied(
                "selected session backend does not provide SQLite-only resource/context capabilities",
            )),
        }
    }
}

fn session_store_dir(opts: &SessionStoreArgs) -> Result<PathBuf> {
    opts.state_dir
        .clone()
        .map(Ok)
        .unwrap_or_else(sessions::default_directory)
}

fn session_store_and_actor(
    opts: &SessionStoreArgs,
) -> Result<(SessionBackendHandle, aw::PrincipalId)> {
    let dir = session_store_dir(opts)?;
    let ctx = ExecutionContext::capture(std::path::Path::new("."))?;
    let actor = sessions::actor(&ctx, false)?;
    let store = match opts.backend {
        SessionStoreBackendArg::Sqlite => SessionBackendHandle::Sqlite(SessionStore::open(&dir)?),
        SessionStoreBackendArg::Jsonl => {
            #[cfg(feature = "jsonl")]
            {
                SessionBackendHandle::Jsonl(sessions::jsonl_store(&dir)?)
            }
            #[cfg(not(feature = "jsonl"))]
            {
                return Err(Error::Config(
                    "this aw build does not include the jsonl session backend".into(),
                ));
            }
        }
    };
    Ok((store, actor))
}

#[derive(Clone, Debug, ClapArgs)]
pub(crate) struct IdentityStoreArgs {
    /// Owned private aw state directory.
    #[arg(long, env = "AW_STATE_DIR")]
    state_dir: Option<PathBuf>,
    /// Machine-readable output.
    #[arg(long)]
    json: bool,
}

fn identity_store_dir(opts: &IdentityStoreArgs) -> Result<PathBuf> {
    opts.state_dir
        .clone()
        .map(Ok)
        .unwrap_or_else(sessions::default_directory)
}

fn identity_store_and_actor(opts: &IdentityStoreArgs) -> Result<(SessionStore, aw::PrincipalId)> {
    let dir = identity_store_dir(opts)?;
    let ctx = ExecutionContext::capture(std::path::Path::new("."))?;
    Ok((SessionStore::open(&dir)?, sessions::actor(&ctx, false)?))
}
fn print<T: serde::Serialize>(value: &T, _json: bool) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value)
            .map_err(|_| Error::Storage("cannot encode CLI result"))?
    );
    Ok(())
}

fn read_private_bytes(path: &std::path::Path, max: usize, what: &'static str) -> Result<Vec<u8>> {
    let mut file = aw_platform::open_private_existing(path, false).map_err(|_| {
        Error::Denied(
            "private credential/token files require platform-verified owner-only custody",
        )
    })?;
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(Error::Limit(what));
    }
    Ok(bytes)
}

fn write_private_bytes_new(path: &std::path::Path, bytes: &[u8], what: &'static str) -> Result<()> {
    let _ = what;
    if path.exists() {
        return Err(Error::Conflict(
            "refusing to overwrite an existing private credential/token file",
        ));
    }
    let mut file = aw_platform::open_private_new(path).map_err(|_| {
        Error::Denied(
            "private credential/token file creation requires platform-verified owner-only custody",
        )
    })?;
    file.write_all(bytes)?;
    file.sync_all()?;
    aw_platform::sync_parent(path)?;
    Ok(())
}

#[cfg(feature = "pipelines")]
#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum PipelineScopeArg {
    User,
    Session,
}

#[cfg(feature = "pipelines")]
impl From<PipelineScopeArg> for aw_store::PipelineBindingScope {
    fn from(value: PipelineScopeArg) -> Self {
        match value {
            PipelineScopeArg::User => Self::User,
            PipelineScopeArg::Session => Self::Session,
        }
    }
}

#[cfg(feature = "pipelines")]
#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum PipelineModeArg {
    Gate,
    Transform,
    Observer,
    Task,
}

#[cfg(feature = "pipelines")]
impl From<PipelineModeArg> for aw_store::PipelineBindingMode {
    fn from(value: PipelineModeArg) -> Self {
        match value {
            PipelineModeArg::Gate => Self::Gate,
            PipelineModeArg::Transform => Self::Transform,
            PipelineModeArg::Observer => Self::Observer,
            PipelineModeArg::Task => Self::Task,
        }
    }
}

#[cfg(feature = "pipelines")]
#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum ScheduleOverlapArg { Skip, Serial }
#[cfg(feature = "pipelines")]
impl From<ScheduleOverlapArg> for aw_store::ScheduleOverlapPolicy {
    fn from(value: ScheduleOverlapArg) -> Self {
        match value { ScheduleOverlapArg::Skip => Self::Skip, ScheduleOverlapArg::Serial => Self::Serial }
    }
}

#[cfg(feature = "pipelines")]
#[derive(Subcommand, Debug)]
pub(crate) enum ScheduleCommand {
    Define {
        name: String,
        #[arg(long)] binding: String,
        #[arg(long = "every-seconds", value_parser=clap::value_parser!(u64).range(60..=2592000))]
        every_seconds: u64,
        #[arg(long = "delay-seconds", default_value_t = 0, value_parser=clap::value_parser!(u64).range(0..=2592000))]
        delay_seconds: u64,
        #[arg(long, value_enum, default_value = "serial")] overlap: ScheduleOverlapArg,
        #[arg(value_name="INPUT",num_args=0..)] input: Vec<String>,
        #[command(flatten)] store: StoreArgs,
    },
    List { #[command(flatten)] store: StoreArgs },
    Show { schedule: String, #[command(flatten)] store: StoreArgs },
    Pause { schedule: String, #[arg(long)] expected_generation: u64, #[command(flatten)] store: StoreArgs },
    Resume { schedule: String, #[arg(long)] expected_generation: u64, #[command(flatten)] store: StoreArgs },
    Occurrences { schedule: String, #[command(flatten)] store: StoreArgs },
    RunDue { #[command(flatten)] args: Box<crate::ScheduleRunDueCliArgs> },
}

#[cfg(feature = "pipelines")]
pub(crate) fn schedule(command: ScheduleCommand) -> Result<u8> {
    match command {
        ScheduleCommand::RunDue { .. } => Err(Error::Config("schedule run-due must be dispatched by the root CLI".into())),
        ScheduleCommand::Define { name, binding, every_seconds, delay_seconds, overlap, input, store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(&store.schedule_define(&actor, &name, &binding, &input.join(" "), every_seconds, overlap.into(), delay_seconds)?, o.json)?;
            Ok(0)
        }
        ScheduleCommand::List { store: o } => { let (store, actor)=store_and_actor(&o)?; print(&store.schedules(&actor)?,o.json)?; Ok(0) }
        ScheduleCommand::Show { schedule, store: o } => { let (store, actor)=store_and_actor(&o)?; print(&store.schedule(&actor,&schedule)?,o.json)?; Ok(0) }
        ScheduleCommand::Pause { schedule, expected_generation, store: o } => { let (store, actor)=store_and_actor(&o)?; print(&store.schedule_set_paused(&actor,&schedule,expected_generation,true)?,o.json)?; Ok(0) }
        ScheduleCommand::Resume { schedule, expected_generation, store: o } => { let (store, actor)=store_and_actor(&o)?; print(&store.schedule_set_paused(&actor,&schedule,expected_generation,false)?,o.json)?; Ok(0) }
        ScheduleCommand::Occurrences { schedule, store: o } => { let (store, actor)=store_and_actor(&o)?; print(&store.schedule_occurrences(&actor,&schedule)?,o.json)?; Ok(0) }
    }
}

#[cfg(feature = "pipelines")]
#[derive(Subcommand, Debug)]
pub(crate) enum PipelineCommand {
    /// Create a new immutable revision from a typed JSON definition.
    Define {
        file: PathBuf,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// List the caller's pipelines at their latest revision.
    #[command(alias = "ls")]
    List {
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Show one immutable pipeline revision.
    Show {
        pipeline: String,
        #[arg(long)]
        revision: Option<String>,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Bind an immutable revision to a manual governed scope.
    Bind {
        pipeline: String,
        #[arg(long)]
        revision: Option<String>,
        #[arg(long, value_enum)]
        scope: PipelineScopeArg,
        #[arg(long)]
        scope_id: String,
        #[arg(long, value_enum, default_value = "task")]
        mode: PipelineModeArg,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// List bindings owned by the caller.
    Bindings {
        #[command(flatten)]
        store: StoreArgs,
    },
    /// CAS-update a binding's enabled state or pinned revision.
    BindingSet {
        binding: String,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        #[arg(long, conflicts_with = "enable")]
        disable: bool,
        #[arg(long)]
        revision: Option<String>,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Start a manual task pipeline using the normal caller provider/tool authority.
    Run {
        #[command(flatten)]
        args: crate::PipelineRunCliArgs,
    },
    /// Resume a durable pipeline paused after approval or between settled steps.
    Resume {
        #[command(flatten)]
        args: crate::PipelineResumeCliArgs,
    },
    /// Show one durable pipeline run and step receipts.
    Status {
        run: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Decide one waiting approval. Resume is a separate explicit command.
    Approve {
        approval: String,
        #[arg(long)]
        deny: bool,
        #[command(flatten)]
        store: StoreArgs,
    },
}

#[cfg(feature = "pipelines")]
pub(crate) fn pipeline(command: PipelineCommand) -> Result<u8> {
    match command {
        PipelineCommand::Define { file, store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            let mut bytes = Vec::new();
            std::fs::File::open(file)?
                .take((256 * 1024 + 1) as u64)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 256 * 1024 {
                return Err(Error::Limit("pipeline definition exceeds 256 KiB"));
            }
            let definition: aw_pipeline::PipelineDefinition = serde_json::from_slice(&bytes)
                .map_err(|_| Error::Config("invalid typed pipeline definition JSON".into()))?;
            print(&store.pipeline_define(&actor, definition)?, o.json)?;
        }
        PipelineCommand::List { store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(&store.pipelines(&actor)?, o.json)?;
        }
        PipelineCommand::Show { pipeline, revision, store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(
                &store.pipeline_revision(&actor, &pipeline, revision.as_deref())?,
                o.json,
            )?;
        }
        PipelineCommand::Bind { pipeline, revision, scope, scope_id, mode, store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(
                &store.pipeline_bind(
                    &actor,
                    &pipeline,
                    revision.as_deref(),
                    scope.into(),
                    &scope_id,
                    mode.into(),
                )?,
                o.json,
            )?;
        }
        PipelineCommand::Bindings { store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(&store.pipeline_bindings(&actor)?, o.json)?;
        }
        PipelineCommand::BindingSet { binding, expected_revision, enable, disable, revision, store: o } => {
            if !enable && !disable {
                return Err(Error::Config("binding-set requires --enable or --disable".into()));
            }
            let (store, actor) = store_and_actor(&o)?;
            print(
                &store.pipeline_binding_set(
                    &actor,
                    &binding,
                    expected_revision,
                    enable,
                    revision.as_deref(),
                )?,
                o.json,
            )?;
        }
        PipelineCommand::Status { run, store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(
                &serde_json::json!({
                    "run": store.pipeline_run(&actor, &run)?,
                    "steps": store.pipeline_steps(&actor, &run)?,
                }),
                o.json,
            )?;
        }
        PipelineCommand::Approve { approval, deny, store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(&store.pipeline_approve(&actor, &approval, !deny)?, o.json)?;
        }
        PipelineCommand::Run { .. } | PipelineCommand::Resume { .. } => {
            return Err(Error::Config("pipeline execution must be dispatched by the root CLI".into()))
        }
    }
    Ok(0)
}

#[cfg(feature = "pipelines")]
#[derive(Subcommand, Debug)]
pub(crate) enum TeamTemplateCommand {
    /// Create a new immutable bounded team-template revision from JSON.
    Define {
        file: PathBuf,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// List the caller's team templates at their latest revision.
    #[command(alias = "ls")]
    List {
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Show one immutable team-template revision.
    Show {
        team: String,
        #[arg(long)]
        revision: Option<String>,
        #[command(flatten)]
        store: StoreArgs,
    },
}

#[cfg(feature = "pipelines")]
pub(crate) fn team_template(command: TeamTemplateCommand) -> Result<u8> {
    match command {
        TeamTemplateCommand::Define { file, store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            let mut bytes = Vec::new();
            std::fs::File::open(file)?
                .take((64 * 1024 + 1) as u64)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 64 * 1024 {
                return Err(Error::Limit("team template exceeds 64 KiB"));
            }
            let definition: aw_pipeline::TeamDefinition = serde_json::from_slice(&bytes)
                .map_err(|_| Error::Config("invalid typed team-template JSON".into()))?;
            print(&store.team_define(&actor, definition)?, o.json)?;
        }
        TeamTemplateCommand::List { store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(&store.teams(&actor)?, o.json)?;
        }
        TeamTemplateCommand::Show {
            team,
            revision,
            store: o,
        } => {
            let (store, actor) = store_and_actor(&o)?;
            print(
                &store.team_revision(&actor, &team, revision.as_deref())?,
                o.json,
            )?;
        }
    }
    Ok(0)
}

#[cfg(all(feature = "pipelines", feature = "routing"))]
#[derive(Subcommand, Debug)]
pub(crate) enum ReviewCommand {
    /// List durable guardian review operations and approval/release state.
    #[command(alias = "ls")]
    List {
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Redacted pending human approvals for one exact owned Session/branch;
    /// decision still uses the original explicit `aw review approve/deny`.
    Pending {
        session_id:String,
        branch_id:String,
        #[command(flatten)]
        store:StoreArgs,
    },
    /// Show one durable guardian review operation.
    Show {
        review: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Explicitly approve one review waiting for human judgment.
    Approve {
        review: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Explicitly deny one review waiting for human judgment.
    Deny {
        review: String,
        #[command(flatten)]
        store: StoreArgs,
    },
}

#[cfg(all(feature = "pipelines", feature = "routing"))]
pub(crate) fn review(command: ReviewCommand) -> Result<u8> {
    match command {
        ReviewCommand::List { store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(&store.reviews(&actor)?, o.json)?;
        }
        ReviewCommand::Pending{session_id,branch_id,store:o}=>{
            let (store,actor)=store_and_actor(&o)?;
            print(&aw::approval_ui::pending_for_user(&store,&actor,&session_id,&branch_id)?,o.json)?;
        }
        ReviewCommand::Show { review, store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(&store.review_status(&actor, &review)?, o.json)?;
        }
        ReviewCommand::Approve { review, store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(&store.review_decide_human(&actor, &review, true)?, o.json)?;
        }
        ReviewCommand::Deny { review, store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(&store.review_decide_human(&actor, &review, false)?, o.json)?;
        }
    }
    Ok(0)
}

#[cfg(feature = "routing")]
#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum RoutingScopeArg {
    User,
    Session,
}

#[cfg(feature = "routing")]
impl From<RoutingScopeArg> for aw_store::RoutingBindingScope {
    fn from(value: RoutingScopeArg) -> Self {
        match value {
            RoutingScopeArg::User => Self::User,
            RoutingScopeArg::Session => Self::Session,
        }
    }
}

#[cfg(feature = "routing")]
#[derive(Subcommand, Debug)]
pub(crate) enum RouteCommand {
    /// Create an immutable routing-strategy revision from typed JSON.
    Define {
        file: PathBuf,
        /// Explicit aw config used to resolve/pin route profile destinations.
        #[arg(long)]
        config: Option<PathBuf>,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// List the caller's routing strategies at their latest revision.
    #[command(alias = "ls")]
    List {
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Show one immutable routing-strategy revision.
    Show {
        route: String,
        #[arg(long)]
        revision: Option<String>,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Bind an immutable strategy revision to this user or one owned Session.
    Bind {
        route: String,
        #[arg(long)]
        revision: Option<String>,
        #[arg(long, value_enum)]
        scope: RoutingScopeArg,
        #[arg(long)]
        scope_id: String,
        #[arg(long, default_value = "main")]
        role: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// List routing bindings owned by the caller.
    Bindings {
        #[command(flatten)]
        store: StoreArgs,
    },
    /// CAS-update a routing binding's enabled state or pinned revision.
    BindingSet {
        binding: String,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        #[arg(long, conflicts_with = "enable")]
        disable: bool,
        #[arg(long)]
        revision: Option<String>,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Inspect sticky affinity for one exact user/Session/branch/role/lineage key.
    Affinity {
        #[arg(long)]
        session: String,
        #[arg(long, default_value = "main")]
        branch: String,
        #[arg(long, default_value = "main")]
        role: String,
        #[arg(long, default_value = "main")]
        lineage: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// CAS-clear sticky affinity. The next route-once prompt must classify again.
    AffinityClear {
        #[arg(long)]
        session: String,
        #[arg(long, default_value = "main")]
        branch: String,
        #[arg(long, default_value = "main")]
        role: String,
        #[arg(long, default_value = "main")]
        lineage: String,
        #[arg(long)]
        expected_revision: u64,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Show one durable routing operation and its classifier attempt, if any.
    Status {
        operation: String,
        #[command(flatten)]
        store: StoreArgs,
    },
}

#[cfg(feature = "routing")]
pub(crate) fn route(command: RouteCommand) -> Result<u8> {
    match command {
        RouteCommand::Define { file, config, store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            let mut bytes = Vec::new();
            std::fs::File::open(file)?
                .take((256 * 1024 + 1) as u64)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 256 * 1024 {
                return Err(Error::Limit("routing strategy definition exceeds 256 KiB"));
            }
            let mut definition: aw_routing::RoutingStrategyDefinition = serde_json::from_slice(&bytes)
                .map_err(|_| Error::Config("invalid typed routing strategy JSON".into()))?;
            let cfg = aw::config::load(config.as_deref())?;
            let mut profiles = aw::config::builtins();
            profiles.extend(cfg.profiles);
            for target in definition
                .candidates
                .iter_mut()
                .chain(definition.classifier.iter_mut())
            {
                let profile = profiles.get(&target.account_id).ok_or_else(|| {
                    Error::Config(format!(
                        "routing target {} references unknown configured profile {}",
                        target.id, target.account_id
                    ))
                })?;
                if !profile.effective_models().iter().any(|model| model == &target.model) {
                    return Err(Error::Denied(
                        "routing target model is not explicitly allowed by its configured profile",
                    ));
                }
                target.destination_fingerprint = aw::sessions::destination_fingerprint(profile)?;
            }
            print(&store.routing_strategy_define(&actor, definition)?, o.json)?;
        }
        RouteCommand::List { store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(&store.routing_strategies(&actor)?, o.json)?;
        }
        RouteCommand::Show { route, revision, store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(
                &store.routing_strategy(&actor, &route, revision.as_deref())?,
                o.json,
            )?;
        }
        RouteCommand::Bind { route, revision, scope, scope_id, role, store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(
                &store.routing_bind(
                    &actor,
                    &route,
                    revision.as_deref(),
                    scope.into(),
                    &scope_id,
                    &role,
                )?,
                o.json,
            )?;
        }
        RouteCommand::Bindings { store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(&store.routing_bindings(&actor)?, o.json)?;
        }
        RouteCommand::BindingSet { binding, expected_revision, enable, disable, revision, store: o } => {
            if !enable && !disable {
                return Err(Error::Config("binding-set requires --enable or --disable".into()));
            }
            let (store, actor) = store_and_actor(&o)?;
            print(
                &store.routing_binding_set(
                    &actor,
                    &binding,
                    expected_revision,
                    enable,
                    revision.as_deref(),
                )?,
                o.json,
            )?;
        }
        RouteCommand::Affinity { session, branch, role, lineage, store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(
                &store.routing_affinity(&actor, &session, &branch, &role, &lineage)?,
                o.json,
            )?;
        }
        RouteCommand::AffinityClear { session, branch, role, lineage, expected_revision, store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            store.routing_affinity_clear(
                &actor,
                &session,
                &branch,
                &role,
                &lineage,
                expected_revision,
            )?;
            print(&serde_json::json!({"cleared":true}), o.json)?;
        }
        RouteCommand::Status { operation, store: o } => {
            let (store, actor) = store_and_actor(&o)?;
            print(
                &serde_json::json!({
                    "operation": store.routing_operation(&actor, &operation)?,
                    "attempt": store.routing_attempt(&actor, &operation)?,
                }),
                o.json,
            )?;
        }
    }
    Ok(0)
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)] // One typed Clap tree intentionally keeps flattened command args explicit.
pub(crate) enum WorkspaceCommand {
    /// Open the native fullscreen Workspace UI.
    Open {
        name: String,
        #[command(flatten)]
        args: crate::WorkspaceOpenCliArgs,
    },
    /// Attach a second native terminal to a human-invited, live PTY.
    #[cfg(all(feature = "workspace", unix))]
    Attach { invite: PathBuf },
    /// Explicit, independent owner for a persistent local PTY (Unix only).
    #[cfg(all(feature = "workspace", unix))]
    Persistent {
        #[command(subcommand)]
        command: PersistentTerminalCommand,
    },
    /// List accessible Workspaces.
    #[command(alias = "ls")]
    List {
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Create a hosted Workspace on this AwNode.
    Create {
        name: String,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long)]
        fleet: Option<String>,
    },
    /// Show one Workspace.
    Show {
        name: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Archive a non-local Workspace without deleting its Sessions.
    Archive {
        name: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Show the current mutable-ref writer AwNode and epoch for one Workspace.
    WriterShow {
        name: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Show immutable Workspace writer-epoch history.
    WriterEvents {
        name: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Fail over Workspace mutable-ref authority under current Domain authority.
    WriterFailover {
        name: String,
        target_node: String,
        #[arg(long)]
        expected_epoch: u64,
        #[arg(long, default_value = "manual_failover")]
        reason: String,
        #[command(flatten)]
        store: StoreArgs,
    },
}
#[cfg(all(feature = "workspace", unix))]
#[derive(Subcommand, Debug)]
pub(crate) enum PersistentTerminalCommand {
    /// Spawn one unsandboxed Bash in its own local PTY broker (24h idle limit).
    Start {
        root: PathBuf,
        shell: PathBuf,
    },
    /// Terminate the explicitly owned persistent PTY, after admission.
    Stop { invite: PathBuf },
    /// Inspect an original-PTY migration without changing custody.
    #[cfg(target_os = "linux")]
    Inspect { directory: PathBuf },
    /// Internal inert recipient; accepts no shell/executable argument.
    #[cfg(target_os = "linux")]
    #[command(hide = true)]
    Adopt { directory: PathBuf },
    /// Internal broker; never use this as a shell/Agent command.
    #[command(hide = true)]
    Serve {
        root: PathBuf,
        shell: PathBuf,
        directory: PathBuf,
    },
}

pub(crate) fn workspace(command: WorkspaceCommand) -> Result<u8> {
    match command {
        #[cfg(all(feature = "workspace", unix))]
        WorkspaceCommand::Attach { .. } => return Err(Error::Denied("native PTY attach must be dispatched by root CLI")),
        #[cfg(all(feature = "workspace", unix))]
        WorkspaceCommand::Persistent { .. } => return Err(Error::Denied("persistent PTY requires direct native CLI dispatch")),
        WorkspaceCommand::Open { .. } => {
            return Err(Error::Config(
                "workspace open must be dispatched by the root CLI".into(),
            ))
        }
        WorkspaceCommand::List { store: o } => {
            let (s, a) = store_and_actor(&o)?;
            let d = match o.domain.as_deref() {
                Some(v) => Some(s.domain(&a, v)?.id),
                None => None,
            };
            print(&s.workspaces(&a, d.as_deref())?, o.json)?;
        }
        WorkspaceCommand::Create {
            name,
            store: o,
            fleet,
        } => {
            let (s, a) = store_and_actor(&o)?;
            let v = if let Some(f) = fleet {
                s.create_fleet_workspace(&a, o.domain.as_deref(), &name, &f)?
            } else {
                s.create_workspace(&a, o.domain.as_deref(), &name)?
            };
            print(&v, o.json)?;
        }
        WorkspaceCommand::Show { name, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            let d = match o.domain.as_deref() {
                Some(v) => Some(s.domain(&a, v)?.id),
                None => None,
            };
            print(&s.workspace(&a, d.as_deref(), &name)?, o.json)?;
        }
        WorkspaceCommand::Archive { name, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            let d = match o.domain.as_deref() {
                Some(v) => Some(s.domain(&a, v)?.id),
                None => None,
            };
            print(&s.archive_workspace(&a, d.as_deref(), &name)?, o.json)?;
        }
        WorkspaceCommand::WriterShow { name, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            let d = match o.domain.as_deref() {
                Some(v) => Some(s.domain(&a, v)?.id),
                None => None,
            };
            print(&s.workspace_writer(&a, d.as_deref(), &name)?, o.json)?;
        }
        WorkspaceCommand::WriterEvents { name, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            let d = match o.domain.as_deref() {
                Some(v) => Some(s.domain(&a, v)?.id),
                None => None,
            };
            print(&s.workspace_writer_events(&a, d.as_deref(), &name)?, o.json)?;
        }
        WorkspaceCommand::WriterFailover {
            name,
            target_node,
            expected_epoch,
            reason,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            let d = match o.domain.as_deref() {
                Some(v) => Some(s.domain(&a, v)?.id),
                None => None,
            };
            print(
                &s.failover_workspace_writer(
                    &a,
                    d.as_deref(),
                    &name,
                    expected_epoch,
                    &target_node,
                    &reason,
                )?,
                o.json,
            )?;
        }
    }
    Ok(0)
}

#[derive(Subcommand, Debug)]
pub(crate) enum DomainCommand {
    #[command(alias = "ls")]
    List {
        #[command(flatten)]
        store: StoreArgs,
    },
    Create {
        name: String,
        #[arg(long)]
        parent: Option<String>,
        #[command(flatten)]
        store: StoreArgs,
    },
    Show {
        name: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Show the current Domain AuthoritySet/quorum state.
    AuthorityShow {
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Replace the Domain AuthoritySet. Remote members must already be trusted Domain Machines.
    AuthorityConfigure {
        #[arg(long = "member", required = true)]
        members: Vec<String>,
        #[arg(long)]
        quorum: u64,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Export the exact AuthoritySet bundle a remote voter must explicitly accept.
    AuthorityBundle {
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Explicitly accept a foreign Domain AuthoritySet as a local quorum voter.
    AuthorityAccept {
        source_node: String,
        input: PathBuf,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// List foreign Domain AuthoritySet grants explicitly accepted by this local user.
    AuthorityGrants {
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Revoke a previously accepted foreign Domain authority voter grant.
    AuthorityRevokeGrant {
        grant: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Begin collecting a quorum for a short local authority leader lease.
    AuthorityElectionBegin {
        #[arg(long, default_value_t = 30)]
        ttl: u64,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Inspect one local Domain authority election and its live vote count.
    AuthorityElectionShow {
        election: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Activate a collected authority election after live votes reach quorum.
    AuthorityElectionActivate {
        election: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Inspect the current replicated standby snapshot for a foreign Domain authority.
    AuthorityReplicaShow {
        authority_domain: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Begin a fresh quorum election to promote one replicated standby Domain authority.
    AuthorityPromotionBegin {
        authority_domain: String,
        #[arg(long, default_value_t = 30)]
        ttl: u64,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Inspect one standby promotion election.
    AuthorityPromotionShow {
        election: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Activate a standby promotion election after the old leader lease expired and quorum is live.
    AuthorityPromotionActivate {
        election: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Materialize the minimal replicated Domain authority core after a live quorum promotion.
    AuthorityPromote {
        election: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    Federate {
        peer: String,
        remote_domain: String,
        #[arg(long, default_value = "outbound")]
        direction: String,
        #[arg(long, default_value = "{}")]
        scope: String,
        #[arg(long)]
        permissions: String,
        #[arg(long)]
        include_child_domains: bool,
        #[arg(long)]
        allow_redelegation: bool,
        #[command(flatten)]
        store: StoreArgs,
    },
    Federations {
        #[command(flatten)]
        store: StoreArgs,
    },
    Lease {
        federation: String,
        #[arg(long, default_value_t = 300)]
        ttl: u64,
        #[command(flatten)]
        store: StoreArgs,
    },
    LeaseCheck {
        lease: String,
        peer: String,
        action: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    RevokeLease {
        lease: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    RevokeFederation {
        federation: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    SyncExport {
        lease: String,
        peer: String,
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long, default_value_t = 128)]
        limit: usize,
        #[command(flatten)]
        store: StoreArgs,
    },
    SyncImport {
        lease: String,
        peer: String,
        input: PathBuf,
        #[command(flatten)]
        store: StoreArgs,
    },
    SyncInbox {
        federation: String,
        source_store: String,
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long, default_value_t = 128)]
        limit: usize,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Authorize explicit local projection of one exact remote semantic entity class.
    MaterializationPolicyCreate {
        federation: String,
        source_store: String,
        event_kind: String,
        entity_type: String,
        entity_id: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// List semantic materialization policies for one federation.
    MaterializationPolicies {
        federation: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Revoke a semantic materialization policy without deleting projected facts.
    RevokeMaterializationPolicy {
        policy: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Explicitly project one imported semantic event into an immutable local fact.
    Materialize {
        policy: String,
        source_seq: u64,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// List immutable local facts explicitly materialized from remote evidence.
    MaterializedFacts {
        federation: String,
        source_store: String,
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long, default_value_t = 128)]
        limit: usize,
        #[command(flatten)]
        store: StoreArgs,
    },
}
pub(crate) fn domain(command: DomainCommand) -> Result<u8> {
    match command {
        DomainCommand::List { store: o } => {
            let (s, a) = store_and_actor(&o)?;
            print(&s.domains(&a)?, o.json)?;
        }
        DomainCommand::Create {
            name,
            parent,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            print(&s.create_domain(&a, &name, parent.as_deref())?, o.json)?;
        }
        DomainCommand::Show { name, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            print(&s.domain(&a, &name)?, o.json)?;
        }
        DomainCommand::AuthorityShow { store: o } => {
            let (s, a) = store_and_actor(&o)?;
            print(&s.domain_authority_set(&a, o.domain.as_deref())?, o.json)?;
        }
        DomainCommand::AuthorityConfigure {
            members,
            quorum,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            print(
                &s.configure_domain_authority(&a, o.domain.as_deref(), &members, quorum)?,
                o.json,
            )?;
        }
        DomainCommand::AuthorityBundle { store: o } => {
            let (s, a) = store_and_actor(&o)?;
            print(
                &s.domain_authority_grant_bundle(&a, o.domain.as_deref())?,
                o.json,
            )?;
        }
        DomainCommand::AuthorityAccept {
            source_node,
            input,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            let bytes = std::fs::read(&input)?;
            if bytes.len() > 64 * 1024 {
                return Err(Error::Limit("Domain authority grant bundle exceeds 64 KiB"));
            }
            let bundle: aw_store::DomainAuthorityGrantBundle = serde_json::from_slice(&bytes)
                .map_err(|_| Error::Config("invalid Domain authority grant bundle JSON".into()))?;
            print(
                &s.accept_domain_authority_peer_grant(&a, &source_node, &bundle)?,
                o.json,
            )?;
        }
        DomainCommand::AuthorityGrants { store: o } => {
            let (s, a) = store_and_actor(&o)?;
            print(&s.domain_authority_peer_grants(&a)?, o.json)?;
        }
        DomainCommand::AuthorityRevokeGrant { grant, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            print(&s.revoke_domain_authority_peer_grant(&a, &grant)?, o.json)?;
        }
        DomainCommand::AuthorityElectionBegin { ttl, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            print(
                &s.begin_domain_authority_election(&a, o.domain.as_deref(), ttl)?,
                o.json,
            )?;
        }
        DomainCommand::AuthorityElectionShow { election, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            print(&s.domain_authority_election(&a, &election)?, o.json)?;
        }
        DomainCommand::AuthorityElectionActivate { election, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            print(
                &s.activate_domain_authority_election(&a, &election)?,
                o.json,
            )?;
        }
        DomainCommand::AuthorityReplicaShow {
            authority_domain,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            print(&s.domain_authority_replica(&a, &authority_domain)?, o.json)?;
        }
        DomainCommand::AuthorityPromotionBegin {
            authority_domain,
            ttl,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            print(
                &s.begin_domain_authority_replica_election(&a, &authority_domain, ttl)?,
                o.json,
            )?;
        }
        DomainCommand::AuthorityPromotionShow { election, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            print(&s.domain_authority_replica_election(&a, &election)?, o.json)?;
        }
        DomainCommand::AuthorityPromotionActivate { election, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            print(
                &s.activate_domain_authority_replica_election(&a, &election)?,
                o.json,
            )?;
        }
        DomainCommand::AuthorityPromote { election, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            print(&s.promote_domain_authority_replica(&a, &election)?, o.json)?;
        }
        DomainCommand::Federate {
            peer,
            remote_domain,
            direction,
            scope,
            permissions,
            include_child_domains,
            allow_redelegation,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            let scope: serde_json::Value = serde_json::from_str(&scope)
                .map_err(|_| Error::Config("--scope must be valid JSON".into()))?;
            let permissions: serde_json::Value = serde_json::from_str(&permissions)
                .map_err(|_| Error::Config("--permissions must be valid JSON".into()))?;
            print(
                &s.create_domain_federation(
                    &a,
                    o.domain.as_deref(),
                    &peer,
                    &remote_domain,
                    &direction,
                    scope,
                    permissions,
                    include_child_domains,
                    allow_redelegation,
                )?,
                o.json,
            )?;
        }
        DomainCommand::Federations { store: o } => {
            let (s, a) = store_and_actor(&o)?;
            print(&s.domain_federations(&a, o.domain.as_deref())?, o.json)?;
        }
        DomainCommand::Lease {
            federation,
            ttl,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            print(&s.issue_federation_lease(&a, &federation, ttl)?, o.json)?;
        }
        DomainCommand::LeaseCheck {
            lease,
            peer,
            action,
            store: o,
        } => {
            let (s, _) = store_and_actor(&o)?;
            print(
                &s.validate_federation_lease(&lease, &peer, &action)?,
                o.json,
            )?;
        }
        DomainCommand::RevokeLease { lease, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            print(&s.revoke_federation_lease(&a, &lease)?, o.json)?;
        }
        DomainCommand::RevokeFederation {
            federation,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            print(&s.revoke_domain_federation(&a, &federation)?, o.json)?;
        }
        DomainCommand::SyncExport {
            lease,
            peer,
            after,
            limit,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            print(
                &s.semantic_sync_export(&a, &lease, &peer, after, limit)?,
                o.json,
            )?;
        }
        DomainCommand::SyncImport {
            lease,
            peer,
            input,
            store: o,
        } => {
            let (s, _) = store_and_actor(&o)?;
            let bytes = std::fs::read(&input)?;
            if bytes.len() > 4 * 1024 * 1024 {
                return Err(Error::Limit("semantic sync batch file exceeds 4 MiB"));
            }
            let batch: aw_store::SemanticSyncBatch = serde_json::from_slice(&bytes)
                .map_err(|_| Error::Config("invalid semantic sync batch JSON".into()))?;
            print(&s.semantic_sync_apply(&lease, &peer, &batch)?, o.json)?;
        }
        DomainCommand::SyncInbox {
            federation,
            source_store,
            after,
            limit,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            print(
                &s.semantic_sync_inbox(&a, &federation, &source_store, after, limit)?,
                o.json,
            )?;
        }
        DomainCommand::MaterializationPolicyCreate {
            federation,
            source_store,
            event_kind,
            entity_type,
            entity_id,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            print(
                &s.create_semantic_materialization_policy(
                    &a,
                    &federation,
                    &source_store,
                    &event_kind,
                    &entity_type,
                    &entity_id,
                )?,
                o.json,
            )?;
        }
        DomainCommand::MaterializationPolicies {
            federation,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            print(
                &s.semantic_materialization_policies(&a, &federation)?,
                o.json,
            )?;
        }
        DomainCommand::RevokeMaterializationPolicy { policy, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            print(
                &s.revoke_semantic_materialization_policy(&a, &policy)?,
                o.json,
            )?;
        }
        DomainCommand::Materialize {
            policy,
            source_seq,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            print(
                &s.materialize_semantic_fact(&a, &policy, source_seq)?,
                o.json,
            )?;
        }
        DomainCommand::MaterializedFacts {
            federation,
            source_store,
            after,
            limit,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            print(
                &s.semantic_materialized_facts(&a, &federation, &source_store, after, limit)?,
                o.json,
            )?;
        }
    }
    Ok(0)
}

#[derive(Subcommand, Debug)]
pub(crate) enum MachineCommand {
    #[command(alias = "ls")]
    List {
        #[command(flatten)]
        store: StoreArgs,
    },
    Show {
        name: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Explicitly register an already-trusted AwNet peer as a Domain remote Machine.
    Register {
        peer: String,
        #[arg(long)]
        name: Option<String>,
        #[command(flatten)]
        store: StoreArgs,
    },
}
pub(crate) fn machine(command: MachineCommand) -> Result<u8> {
    match command {
        MachineCommand::List { store: o } => {
            let (s, a) = store_and_actor(&o)?;
            let d = match o.domain.as_deref() {
                Some(v) => Some(s.domain(&a, v)?.id),
                None => None,
            };
            print(&s.machines(&a, d.as_deref())?, o.json)?;
        }
        MachineCommand::Show { name, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            let d = match o.domain.as_deref() {
                Some(v) => Some(s.domain(&a, v)?.id),
                None => None,
            };
            print(&s.machine(&a, d.as_deref(), &name)?, o.json)?;
        }
        MachineCommand::Register {
            peer,
            name,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            print(
                &s.register_remote_machine(&a, o.domain.as_deref(), &peer, name.as_deref())?,
                o.json,
            )?;
        }
    }
    Ok(0)
}

#[derive(Subcommand, Debug)]
pub(crate) enum FleetCommand {
    #[command(alias = "ls")]
    List {
        #[command(flatten)]
        store: StoreArgs,
    },
    Create {
        name: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    Show {
        name: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    Add {
        fleet: String,
        machine: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Create or inspect an immutable Fleet fan-out snapshot.
    Plan {
        fleet: String,
        #[arg(long)]
        operation_id: String,
        #[arg(long)]
        request_sha256: String,
        #[arg(long)]
        workspace: Option<String>,
        #[arg(long, default_value = "all")]
        policy: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Inspect durable Fleet fan-out operations.
    Runs {
        fleet: String,
        #[arg(long)]
        operation_id: Option<String>,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Dispatch one member from an existing pinned Fleet operation into a durable PeerWorkspace Session.
    DispatchMember {
        operation_id: String,
        machine: String,
        peer: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Update one member result of an existing pinned Fleet operation.
    RunUpdate {
        operation_id: String,
        machine: String,
        state: String,
        #[arg(long)]
        remote_ref: Option<String>,
        #[arg(long)]
        result_sha256: Option<String>,
        #[command(flatten)]
        store: StoreArgs,
    },
}
pub(crate) fn fleet(command: FleetCommand) -> Result<u8> {
    match command {
        FleetCommand::List { store: o } => {
            let (s, a) = store_and_actor(&o)?;
            let d = match o.domain.as_deref() {
                Some(v) => Some(s.domain(&a, v)?.id),
                None => None,
            };
            print(&s.fleets(&a, d.as_deref())?, o.json)?;
        }
        FleetCommand::Create { name, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            print(&s.create_fleet(&a, o.domain.as_deref(), &name)?, o.json)?;
        }
        FleetCommand::Show { name, store: o } => {
            let (s, a) = store_and_actor(&o)?;
            let d = match o.domain.as_deref() {
                Some(v) => Some(s.domain(&a, v)?.id),
                None => None,
            };
            let f = s.fleet_in_domain(&a, d.as_deref(), &name)?;
            print(
                &serde_json::json!({"fleet":f,"members":s.fleet_members(&a,&f.id)?}),
                o.json,
            )?;
        }
        FleetCommand::Add {
            fleet,
            machine,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            let d = match o.domain.as_deref() {
                Some(v) => Some(s.domain(&a, v)?.id),
                None => None,
            };
            print(
                &s.add_fleet_member_in_domain(&a, d.as_deref(), &fleet, &machine)?,
                o.json,
            )?;
        }
        FleetCommand::Plan {
            fleet,
            operation_id,
            request_sha256,
            workspace,
            policy,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            print(
                &s.create_fleet_operation(
                    &a,
                    &fleet,
                    workspace.as_deref(),
                    &operation_id,
                    &request_sha256,
                    &policy,
                )?,
                o.json,
            )?;
        }
        FleetCommand::Runs {
            fleet,
            operation_id,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            if let Some(operation_id) = operation_id {
                print(&s.fleet_operation(&a, &operation_id)?, o.json)?;
            } else {
                print(&s.fleet_operations(&a, &fleet)?, o.json)?;
            }
        }
        FleetCommand::DispatchMember {
            operation_id,
            machine,
            peer,
            room,
            rendezvous,
            title,
            lease,
            listen,
            stun,
            timeout,
            store: o,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    operation_id,
                    machine,
                    peer,
                    room,
                    rendezvous,
                    title,
                    lease,
                    listen,
                    stun,
                    timeout,
                    o,
                );
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, actor) = store_and_actor(&o)?;
                let result = crate::awnet::dispatch_fleet_member_session(
                    &s,
                    &dir,
                    crate::awnet::FleetMemberDispatchOptions {
                        actor,
                        operation_id,
                        machine_id: machine,
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::RemoteSession,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        title,
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        FleetCommand::RunUpdate {
            operation_id,
            machine,
            state,
            remote_ref,
            result_sha256,
            store: o,
        } => {
            let (s, a) = store_and_actor(&o)?;
            print(
                &s.update_fleet_operation_member(
                    &a,
                    &operation_id,
                    &machine,
                    &state,
                    remote_ref.as_deref(),
                    result_sha256.as_deref(),
                )?,
                o.json,
            )?;
        }
    }
    Ok(0)
}

#[derive(Subcommand, Debug)]
pub(crate) enum SubagentCommand {
    /// List visible child Sessions plus any unbound/retained execution bindings.
    #[command(alias = "ls")]
    List {
        #[command(flatten)]
        store: StoreArgs,
        /// Restrict child Sessions to one parent Session.
        #[arg(long)]
        parent: Option<String>,
    },
    /// Show one subagent and its optional isolated execution binding.
    Show {
        id: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Show the delegation envelope for a root or child Session.
    Budget {
        session: String,
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Remove a retained managed worktree without deleting Session/history.
    Cleanup {
        /// Subagent ID or unbound execution-binding ID.
        id: String,
        /// Explicitly discard dirty worktree changes.
        #[arg(long)]
        discard: bool,
        #[command(flatten)]
        store: StoreArgs,
    },
}

pub(crate) fn subagent(command: SubagentCommand) -> Result<u8> {
    match command {
        SubagentCommand::List { store: opts, parent } => {
            let (store, actor) = store_and_actor(&opts)?;
            let relations = if let Some(parent) = parent {
                store.subagents_for_parent(&actor, &parent)?
            } else {
                store.visible_subagents(&actor)?
            };
            let mut children = Vec::with_capacity(relations.len());
            for relation in relations {
                let execution = store
                    .subagent_execution_binding_for_subagent(&actor, &relation.id)?;
                let allocation = relation
                    .delegation_allocation_id
                    .as_deref()
                    .map(|id| store.delegation_child_allocation(&actor, id))
                    .transpose()?;
                children.push(serde_json::json!({
                    "subagent": relation,
                    "execution": execution,
                    "allocation": allocation,
                }));
            }
            let unbound = store
                .visible_subagent_execution_bindings(&actor)?
                .into_iter()
                .filter(|binding| binding.child_session_id.is_none())
                .collect::<Vec<_>>();
            let roots = store.visible_delegation_roots(&actor)?;
            print(
                &serde_json::json!({
                    "subagents": children,
                    "unbound_bindings": unbound,
                    "delegation_roots": roots,
                }),
                opts.json,
            )?;
        }
        SubagentCommand::Show { id, store: opts } => {
            let (store, actor) = store_and_actor(&opts)?;
            let relation = store.subagent(&actor, &id)?;
            let execution = store.subagent_execution_binding_for_subagent(&actor, &id)?;
            let allocation = relation
                .delegation_allocation_id
                .as_deref()
                .map(|allocation| store.delegation_child_allocation(&actor, allocation))
                .transpose()?;
            let budget = store.delegation_root_for_session(&actor, &relation.child_session_id)?;
            print(
                &serde_json::json!({
                    "subagent": relation,
                    "execution": execution,
                    "allocation": allocation,
                    "delegation_root": budget,
                }),
                opts.json,
            )?;
        }
        SubagentCommand::Budget { session, store: opts } => {
            let (store, actor) = store_and_actor(&opts)?;
            let budget = store
                .delegation_root_for_session(&actor, &session)?
                .ok_or(Error::Denied("Session has no Phase 43 delegation root"))?;
            print(&budget, opts.json)?;
        }
        SubagentCommand::Cleanup {
            id,
            discard,
            store: opts,
        } => {
            #[cfg(not(feature = "workspace"))]
            {
                let _ = (id, discard, opts);
                return Err(Error::Denied(
                    "this build does not include managed Git worktree cleanup",
                ));
            }
            #[cfg(feature = "workspace")]
            {
                let (store, actor) = store_and_actor(&opts)?;
                let binding = store.begin_subagent_execution_cleanup(&actor, &id)?;
                let git = std::env::var_os("AW_GIT")
                    .map(PathBuf::from)
                    .or_else(|| option_env!("AW_DEFAULT_GIT").map(PathBuf::from))
                    .unwrap_or_else(|| PathBuf::from("/usr/bin/git"));
                let repo = match aw_git::Repository::discover(&git, &binding.parent_repo_root) {
                    Ok(repo) => repo,
                    Err(error) => {
                        let _ = store.cancel_subagent_execution_cleanup(&actor, &binding.id);
                        return Err(Error::Config(format!(
                            "cannot inspect managed worktree for cleanup: {error}"
                        )));
                    }
                };
                match repo.remove_managed_worktree(&binding.execution_root, discard) {
                    Ok(()) => {
                        let removed =
                            store.finish_subagent_execution_cleanup(&actor, &binding.id)?;
                        print(&removed, opts.json)?;
                    }
                    Err(aw_git::Error::MutationUnknown) => {
                        let _ = store.mark_subagent_execution_unknown(
                            &actor,
                            &binding.id,
                            "managed worktree removal outcome unknown; inspect Git worktrees before retry",
                        );
                        return Err(Error::Storage(
                            "managed worktree removal outcome requires inspection; no automatic retry",
                        ));
                    }
                    Err(error) => {
                        let _ = store.cancel_subagent_execution_cleanup(&actor, &binding.id);
                        return Err(Error::Config(format!(
                            "managed worktree was retained: {error}"
                        )));
                    }
                }
            }
        }
    }
    Ok(0)
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum SessionFormatArg {
    Aw,
    Pi,
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)] // Session open carries the complete typed run surface; boxing would add indirection only for CLI parsing.
pub(crate) enum SessionCommand {
    /// Resume one durable Session in the ordinary interactive UI.
    Open {
        session: String,
        #[command(flatten)]
        run: crate::OpenCliArgs,
    },
    /// Show the selected session backend and its declared capabilities.
    Capabilities {
        #[command(flatten)]
        store: SessionStoreArgs,
    },
    #[command(alias = "ls")]
    List {
        #[command(flatten)]
        store: SessionStoreArgs,
        /// SQLite-only Workspace filter.
        #[arg(long)]
        workspace: Option<String>,
    },
    Show {
        id: String,
        #[arg(long, default_value = "main")]
        branch: String,
        #[command(flatten)]
        store: SessionStoreArgs,
    },
    /// Set the durable display title for one Session.
    Title {
        id: String,
        title: String,
        #[command(flatten)]
        store: SessionStoreArgs,
    },
    Tree {
        id: String,
        #[arg(long, default_value = "main")]
        branch: String,
        #[command(flatten)]
        store: SessionStoreArgs,
    },
    /// Fork a Session. JSONL creates a new session file containing the selected history.
    Fork {
        id: String,
        name: String,
        #[arg(long, default_value = "main")]
        branch: String,
        #[command(flatten)]
        store: SessionStoreArgs,
    },
    /// Export one branch as portable aw JSONL or Pi v3-compatible JSONL.
    #[cfg(feature = "session-interop")]
    Export {
        id: String,
        #[arg(long, default_value = "main")]
        branch: String,
        #[arg(long, value_enum, default_value_t = SessionFormatArg::Aw)]
        format: SessionFormatArg,
        /// Output file. Omit to write JSONL to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[command(flatten)]
        store: SessionStoreArgs,
    },
    /// Import portable aw JSONL or a supported Pi session into the selected backend.
    #[cfg(feature = "session-interop")]
    Import {
        input: PathBuf,
        /// Override format detection.
        #[arg(long, value_enum)]
        format: Option<SessionFormatArg>,
        /// Trusted execution working directory for the imported Session.
        #[arg(short = 'C', long = "directory")]
        directory: Option<PathBuf>,
        #[command(flatten)]
        store: SessionStoreArgs,
    },
}

#[cfg(feature = "session-interop")]
fn session_format(format: SessionFormatArg) -> aw_session_format::Format {
    match format {
        SessionFormatArg::Aw => aw_session_format::Format::Aw,
        SessionFormatArg::Pi => aw_session_format::Format::Pi,
    }
}

#[cfg(feature = "session-interop")]
fn read_session_file(path: &std::path::Path) -> Result<Vec<u8>> {
    const MAX: usize = 32 * 1024 * 1024;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX {
        return Err(Error::Limit("session import exceeds 32 MiB"));
    }
    Ok(bytes)
}

#[cfg(feature = "session-interop")]
fn write_session_export(path: Option<&std::path::Path>, bytes: &[u8]) -> Result<()> {
    if let Some(path) = path {
        if path.exists() {
            return Err(Error::Conflict(
                "refusing to overwrite an existing session export",
            ));
        }
        let mut file = if aw_platform::secure_private_files_supported() {
            aw_platform::open_private_new(path)?
        } else {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?
        };
        file.write_all(bytes)?;
        file.sync_all()?;
        aw_platform::sync_parent(path)?;
    } else {
        std::io::stdout().write_all(bytes)?;
    }
    Ok(())
}

#[cfg(feature = "session-interop")]
fn imported_binding(directory: Option<&std::path::Path>) -> Result<aw_store_api::SessionBinding> {
    let ctx = ExecutionContext::capture(directory.unwrap_or(std::path::Path::new(".")))?;
    Ok(aw_store_api::SessionBinding {
        working_directory: ctx.working_directory.clone(),
        tool_root: ctx.working_directory,
        system_prompt: String::new(),
        profile_hint: "imported".into(),
        model_hint: "unbound".into(),
        destination_fingerprint: "0".repeat(64),
    })
}

pub(crate) fn session(command: SessionCommand) -> Result<u8> {
    match command {
        SessionCommand::Open { .. } => {
            return Err(Error::Config(
                "session open must be dispatched by the root CLI".into(),
            ))
        }
        SessionCommand::Capabilities { store: options } => {
            let (store, _) = session_store_and_actor(&options)?;
            print(&store.backend().descriptor(), options.json)?;
        }
        SessionCommand::List {
            store: options,
            workspace,
        } => {
            let (store, actor) = session_store_and_actor(&options)?;
            let mut rows = store.backend().list(&actor)?;
            if let Some(workspace) = workspace {
                let sqlite = store.sqlite()?;
                let ws = sqlite.workspace(&actor, None, &workspace)?;
                rows.retain(|session| session.workspace_id == ws.id);
            }
            print(&rows, options.json)?;
        }
        SessionCommand::Show {
            id,
            branch,
            store: options,
        } => {
            let (store, actor) = session_store_and_actor(&options)?;
            let cursor = store.backend().open(&actor, &id, &branch)?;
            let view = cursor.view()?;
            print(
                &serde_json::json!({
                    "session": view.session,
                    "branch": view.branch,
                    "history": view.history,
                    "store": store.backend().descriptor(),
                }),
                options.json,
            )?;
        }
        SessionCommand::Title {
            id,
            title,
            store: options,
        } => {
            let (store, actor) = session_store_and_actor(&options)?;
            let info = store.backend().set_title(&actor, &id, &title)?;
            print(&info, options.json)?;
        }
        SessionCommand::Tree {
            id,
            branch,
            store: options,
        } => {
            let (store, actor) = session_store_and_actor(&options)?;
            let cursor = store.backend().open(&actor, &id, &branch)?;
            let view = cursor.view()?;
            let branches = store.backend().branches(&actor, &id)?;
            let mut value = serde_json::json!({
                "session": view.session,
                "branch": view.branch,
                "branches": branches,
                "history": view.history,
                "store": store.backend().descriptor(),
            });
            if store
                .backend()
                .descriptor()
                .capabilities
                .contains(StoreCapability::ContextFlow)
            {
                value["nodes"] = serde_json::to_value(store.sqlite()?.tree(&actor, &id)?)
                    .map_err(|_| Error::Storage("cannot encode Session tree"))?;
            }
            print(&value, options.json)?;
        }
        SessionCommand::Fork {
            id,
            name,
            branch,
            store: options,
        } => {
            let (store, actor) = session_store_and_actor(&options)?;
            store
                .backend()
                .descriptor()
                .capabilities
                .require(StoreCapability::Forks)?;
            let cursor = store.backend().fork(&actor, &id, &branch, &name)?;
            print(&cursor.view()?, options.json)?;
        }
        #[cfg(feature = "session-interop")]
        SessionCommand::Export {
            id,
            branch,
            format,
            output,
            store: options,
        } => {
            let (store, actor) = session_store_and_actor(&options)?;
            store
                .backend()
                .descriptor()
                .capabilities
                .require(StoreCapability::ArchiveExport)?;
            let document = store.backend().export_document(&actor, &id, &branch)?;
            let bytes = aw_session_format::encode(session_format(format), &document)?;
            write_session_export(output.as_deref(), &bytes)?;
        }
        #[cfg(feature = "session-interop")]
        SessionCommand::Import {
            input,
            format,
            directory,
            store: options,
        } => {
            let bytes = read_session_file(&input)?;
            let decoded = aw_session_format::decode(format.map(session_format), &bytes)?;
            let binding = imported_binding(directory.as_deref())?;
            let (store, actor) = session_store_and_actor(&options)?;
            store
                .backend()
                .descriptor()
                .capabilities
                .require(StoreCapability::ArchiveImport)?;
            let (session, branch, cursor) =
                store.backend().import_document(&actor, decoded.document, binding)?;
            print(
                &serde_json::json!({
                    "session": session,
                    "branch": branch,
                    "history": cursor.view()?.history,
                    "warnings": decoded.warnings,
                    "store": store.backend().descriptor(),
                    "review_required": true,
                }),
                options.json,
            )?;
        }
    }
    Ok(0)
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum ContextApplyModeArg {
    Append,
    NextPrompt,
    Fork,
    NewSession,
}

#[derive(Subcommand, Debug)]
pub(crate) enum ContextCommand {
    /// Show the exact current target head/revision for a Session branch.
    Target {
        session: String,
        branch: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    /// Deterministically project exact already-materialized remote fact IDs into an immutable context artifact candidate.
    MaterializedProject {
        session: String,
        branch: String,
        #[arg(long = "fact", required = true)]
        facts: Vec<String>,
        #[arg(long = "request")]
        request_id: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    /// Inspect one context artifact candidate without attaching it.
    Artifact {
        artifact: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    /// Explicitly attach one artifact to its exact current target using existing context CAS/disclosure rules.
    Apply {
        artifact: String,
        session: String,
        branch: String,
        #[arg(long = "request")]
        request_id: String,
        #[arg(long, value_enum, default_value_t = ContextApplyModeArg::Append)]
        mode: ContextApplyModeArg,
        #[arg(long, requires = "mode")]
        name: Option<String>,
        #[arg(long)]
        confirm_disclosure: bool,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    /// Inspect pending one-shot next-prompt attachment receipts.
    Pending {
        session: String,
        branch: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    /// Explicitly discard one pending next-prompt attachment receipt.
    DiscardPending {
        id: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
}

fn context_apply_mode(
    mode: ContextApplyModeArg,
    name: Option<String>,
) -> Result<aw_store::ApplyMode> {
    Ok(match mode {
        ContextApplyModeArg::Append => {
            if name.is_some() {
                return Err(Error::Config(
                    "--context-name is only valid with --ui-mode fork or new-session".into(),
                ));
            }
            aw_store::ApplyMode::Append
        }
        ContextApplyModeArg::NextPrompt => {
            if name.is_some() {
                return Err(Error::Config(
                    "--context-name is only valid with --ui-mode fork or new-session".into(),
                ));
            }
            aw_store::ApplyMode::NextPrompt
        }
        ContextApplyModeArg::Fork => aw_store::ApplyMode::Fork {
            name: name.ok_or_else(|| Error::Config("--ui-mode fork requires --context-name".into()))?,
        },
        ContextApplyModeArg::NewSession => aw_store::ApplyMode::NewSession {
            title: name
                .ok_or_else(|| Error::Config("--ui-mode new-session requires --context-name".into()))?,
        },
    })
}

pub(crate) fn context(command: ContextCommand) -> Result<u8> {
    match command {
        ContextCommand::Target {
            session,
            branch,
            store: o,
        } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            print(&s.context_target(&actor, &session, &branch)?, o.json)?;
        }
        ContextCommand::MaterializedProject {
            session,
            branch,
            facts,
            request_id,
            store: o,
        } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            let target = s.context_target(&actor, &session, &branch)?;
            print(
                &s.project_materialized_context(
                    &actor,
                    aw_store::MaterializedContextRequest {
                        target,
                        materialization_ids: facts,
                        request_id,
                    },
                )?,
                o.json,
            )?;
        }
        ContextCommand::Artifact { artifact, store: o } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            print(&s.context_artifact(&actor, &artifact)?, o.json)?;
        }
        ContextCommand::Apply {
            artifact,
            session,
            branch,
            request_id,
            mode,
            name,
            confirm_disclosure,
            store: o,
        } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            let target = s.context_target(&actor, &session, &branch)?;
            let mode = context_apply_mode(mode, name)?;
            print(
                &s.context_apply(
                    &actor,
                    aw_store::ApplyContext {
                        artifact_id: artifact,
                        target,
                        mode,
                        request_id,
                        confirmed_disclosure: confirm_disclosure,
                    },
                )?,
                o.json,
            )?;
        }
        ContextCommand::Pending {
            session,
            branch,
            store: o,
        } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            print(&s.context_pending(&actor, &session, &branch)?, o.json)?;
        }
        ContextCommand::DiscardPending { id, store: o } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            s.context_discard_pending(&actor, &id)?;
            print(&serde_json::json!({"discarded":true,"id":id}), o.json)?;
        }
    }
    Ok(0)
}

#[derive(Subcommand, Debug)]
pub(crate) enum IdentityCommand {
    /// Show the current local principal's stable opaque aw user identity.
    Show {
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    /// Generate a private Ed25519 user-device key file and register its public key.
    DeviceCreate {
        out: PathBuf,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    /// Register the public key from an existing private user-device key file.
    DeviceRegister {
        key: PathBuf,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    /// Revoke one registered user-device authentication key.
    DeviceRevoke {
        device: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    /// Explicitly link OIDC issuer+subject metadata to the current stable user.
    OidcLink {
        issuer: String,
        subject: String,
        #[arg(long)]
        provider: Option<String>,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    /// Revoke one explicitly linked OIDC issuer+subject binding.
    OidcRevoke {
        identity: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    /// Explicitly accept portable identity metadata from one paired Domain authority/control AwNode.
    ContinuityAccept {
        source_node: String,
        /// Exact Domain ID whose portable identity trust is being managed.
        #[arg(long = "domain")]
        domain_id: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    /// Inspect accepted identity continuity source grants for one Domain ID.
    ContinuityGrants {
        /// Exact Domain ID whose continuity grants should be inspected.
        #[arg(long = "domain")]
        domain_id: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    /// Revoke one accepted identity continuity source immediately.
    ContinuityRevoke {
        grant: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    /// Inspect imported portable device/OIDC bindings for one Domain ID.
    Bindings {
        /// Exact Domain ID whose imported portable bindings should be inspected.
        #[arg(long = "domain")]
        domain_id: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
}

#[cfg(feature = "net")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct UserDeviceKeyFile {
    version: u16,
    seed: String,
}

#[cfg(feature = "net")]
fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|v| format!("{v:02x}")).collect()
}

#[cfg(feature = "net")]
fn decode_user_device_seed(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Storage("user-device key seed must be 32-byte hex"));
    }
    let mut out = [0u8; 32];
    let bytes = value.as_bytes();
    for (index, slot) in out.iter_mut().enumerate() {
        let start = index * 2;
        let pair = std::str::from_utf8(&bytes[start..start + 2])
            .map_err(|_| Error::Storage("invalid user-device key seed"))?;
        *slot = u8::from_str_radix(pair, 16)
            .map_err(|_| Error::Storage("invalid user-device key seed"))?;
    }
    Ok(out)
}

#[cfg(feature = "net")]
fn load_user_device_key(path: &std::path::Path) -> Result<ed25519_dalek::SigningKey> {
    let mut file = aw_platform::open_private_existing(path, false).map_err(|_| {
        Error::Denied("user-device key requires platform-verified owner-only custody")
    })?;

    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(4097)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(Error::Limit("user-device key file exceeds 4096 bytes"));
    }
    let raw: UserDeviceKeyFile = serde_json::from_slice(&bytes)
        .map_err(|_| Error::Storage("invalid user-device key file"))?;
    if raw.version != 1 {
        return Err(Error::Storage("unsupported user-device key file version"));
    }
    Ok(ed25519_dalek::SigningKey::from_bytes(
        &decode_user_device_seed(&raw.seed)?,
    ))
}

#[cfg(feature = "net")]
fn create_user_device_key(path: &std::path::Path) -> Result<ed25519_dalek::SigningKey> {
    if path.exists() {
        return Err(Error::Conflict(
            "refusing to overwrite an existing user-device key file",
        ));
    }
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed)
        .map_err(|_| Error::Storage("OS randomness unavailable for user-device key"))?;
    let raw = UserDeviceKeyFile {
        version: 1,
        seed: encode_hex(&seed),
    };
    let body = serde_json::to_vec_pretty(&raw)
        .map_err(|_| Error::Storage("cannot encode user-device key file"))?;
    let mut file = aw_platform::open_private_new(path).map_err(|_| {
        Error::Denied("user-device key creation requires platform-verified owner-only custody")
    })?;
    file.write_all(&body)?;
    file.sync_all()?;
    aw_platform::sync_parent(path)?;
    Ok(ed25519_dalek::SigningKey::from_bytes(&seed))
}

pub(crate) fn identity(command: IdentityCommand) -> Result<u8> {
    match command {
        IdentityCommand::Show { store: o } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            print(&s.current_user(&actor)?, o.json)?;
        }
        IdentityCommand::DeviceCreate { out, store: o } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (out, o);
                return Err(Error::Config(
                    "user-device key generation requires the net feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                // Validate/open the target store before creating secret material.
                // If ordinary registration fails, clean up the newly created key
                // so the command does not leave confusing half-created state.
                let (s, actor) = identity_store_and_actor(&o)?;
                let signing = create_user_device_key(&out)?;
                let public_key = encode_hex(&signing.verifying_key().to_bytes());
                let registered = match s.register_identity_device_key(&actor, &public_key) {
                    Ok(info) => info,
                    Err(error) => {
                        let _ = std::fs::remove_file(&out);
                        return Err(error);
                    }
                };
                print(&registered, o.json)?;
            }
        }
        IdentityCommand::DeviceRegister { key, store: o } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (key, o);
                return Err(Error::Config(
                    "user-device key registration requires the net feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let signing = load_user_device_key(&key)?;
                let public_key = encode_hex(&signing.verifying_key().to_bytes());
                let (s, actor) = identity_store_and_actor(&o)?;
                print(
                    &s.register_identity_device_key(&actor, &public_key)?,
                    o.json,
                )?;
            }
        }
        IdentityCommand::DeviceRevoke { device, store: o } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            print(&s.revoke_identity_device_key(&actor, &device)?, o.json)?;
        }
        IdentityCommand::OidcLink {
            issuer,
            subject,
            provider,
            store: o,
        } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            print(
                &s.link_oidc_identity(&actor, provider.as_deref(), &issuer, &subject)?,
                o.json,
            )?;
        }
        IdentityCommand::OidcRevoke { identity, store: o } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            print(&s.revoke_oidc_identity(&actor, &identity)?, o.json)?;
        }
        IdentityCommand::ContinuityAccept {
            source_node,
            domain_id,
            store: o,
        } => {
            let domain = domain_id.as_str();
            let (s, actor) = identity_store_and_actor(&o)?;
            print(
                &s.identity_continuity_accept(&actor, domain, &source_node)?,
                o.json,
            )?;
        }
        IdentityCommand::ContinuityGrants {
            domain_id,
            store: o,
        } => {
            let domain = domain_id.as_str();
            let (s, actor) = identity_store_and_actor(&o)?;
            print(&s.identity_continuity_grants(&actor, domain)?, o.json)?;
        }
        IdentityCommand::ContinuityRevoke { grant, store: o } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            print(&s.revoke_identity_continuity_grant(&actor, &grant)?, o.json)?;
        }
        IdentityCommand::Bindings {
            domain_id,
            store: o,
        } => {
            let domain = domain_id.as_str();
            let (s, actor) = identity_store_and_actor(&o)?;
            print(&s.portable_identity_bindings(&actor, domain)?, o.json)?;
        }
    }
    Ok(0)
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum ProviderKindArg {
    Openai,
    Anthropic,
    Gemini,
    Ollama,
}
impl ProviderKindArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::Ollama => "ollama",
        }
    }
    fn default_base_url(self) -> &'static str {
        match self {
            Self::Openai => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com/v1",
            Self::Gemini => "https://generativelanguage.googleapis.com/v1beta",
            Self::Ollama => "http://127.0.0.1:11434",
        }
    }
}
#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum ProviderGrantRoleArg {
    Use,
    Admin,
}
impl ProviderGrantRoleArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::Use => "use",
            Self::Admin => "admin",
        }
    }
}

#[derive(Subcommand, Debug)]
pub(crate) enum ProviderAccountCommand {
    Create {
        account: String,
        #[arg(long, value_enum)]
        provider: ProviderKindArg,
        /// Pinned provider API prefix/server root. Defaults to the built-in endpoint for the provider kind.
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        payer: String,
        #[arg(long = "model", required = true)]
        models: Vec<String>,
        #[arg(long)]
        secret_file: PathBuf,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    List {
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    Rotate {
        account: String,
        #[arg(long)]
        secret_file: PathBuf,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    Grant {
        account: String,
        user_id: String,
        #[arg(long,value_enum,default_value_t=ProviderGrantRoleArg::Use)]
        role: ProviderGrantRoleArg,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    Grants {
        account: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    RevokeGrant {
        account: String,
        user_id: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    Versions {
        account: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    Revoke {
        account: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
}

pub(crate) fn provider_account(command: ProviderAccountCommand) -> Result<u8> {
    match command {
        ProviderAccountCommand::Create {
            account,
            provider,
            base_url,
            payer,
            models,
            secret_file,
            store: o,
        } => {
            let secret = Zeroizing::new(read_private_bytes(
                &secret_file,
                aw_store::MAX_SECRET_BYTES,
                "provider secret file exceeds 64 KiB",
            )?);
            let (s, actor) = identity_store_and_actor(&o)?;
            let secrets = s.local_secret_store()?;
            print(
                &s.create_server_provider_account(
                    &actor,
                    &secrets,
                    &account,
                    provider.as_str(),
                    base_url
                        .as_deref()
                        .unwrap_or_else(|| provider.default_base_url()),
                    &payer,
                    "local",
                    models.into_iter().collect::<BTreeSet<_>>(),
                    secret.as_slice(),
                )?,
                o.json,
            )?;
        }
        ProviderAccountCommand::List { store: o } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            print(&s.provider_accounts(&actor)?, o.json)?;
        }
        ProviderAccountCommand::Rotate {
            account,
            secret_file,
            store: o,
        } => {
            let secret = Zeroizing::new(read_private_bytes(
                &secret_file,
                aw_store::MAX_SECRET_BYTES,
                "provider secret file exceeds 64 KiB",
            )?);
            let (s, actor) = identity_store_and_actor(&o)?;
            let secrets = s.local_secret_store()?;
            print(
                &s.rotate_server_provider_secret(&actor, &secrets, &account, secret.as_slice())?,
                o.json,
            )?;
        }
        ProviderAccountCommand::Grant {
            account,
            user_id,
            role,
            store: o,
        } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            print(
                &s.grant_provider_account(&actor, &account, &user_id, role.as_str())?,
                o.json,
            )?;
        }
        ProviderAccountCommand::Grants { account, store: o } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            print(&s.provider_account_grants(&actor, &account)?, o.json)?;
        }
        ProviderAccountCommand::RevokeGrant {
            account,
            user_id,
            store: o,
        } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            print(
                &s.revoke_provider_account_grant(&actor, &account, &user_id)?,
                o.json,
            )?;
        }
        ProviderAccountCommand::Versions { account, store: o } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            print(&s.provider_secret_versions(&actor, &account)?, o.json)?;
        }
        ProviderAccountCommand::Revoke { account, store: o } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            let secrets = s.local_secret_store()?;
            print(
                &s.revoke_server_provider_account(&actor, &secrets, &account)?,
                o.json,
            )?;
        }
    }
    Ok(0)
}

#[derive(Subcommand, Debug)]
pub(crate) enum AuthCommand {
    Issue {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 86400)]
        ttl: u64,
        #[arg(long)]
        client: Option<String>,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    Verify {
        token: PathBuf,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    Rotate {
        token: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    Revoke {
        token: PathBuf,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    List {
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
}
fn read_refresh_token(path: &std::path::Path) -> Result<Zeroizing<String>> {
    let bytes = Zeroizing::new(read_private_bytes(
        path,
        512,
        "auth-session token file exceeds 512 bytes",
    )?);
    String::from_utf8(bytes.to_vec())
        .map(Zeroizing::new)
        .map_err(|_| Error::Config("auth-session token file must be UTF-8".into()))
}
pub(crate) fn auth(command: AuthCommand) -> Result<u8> {
    match command {
        AuthCommand::Issue {
            out,
            ttl,
            client,
            store: o,
        } => {
            if out.exists() {
                return Err(Error::Conflict(
                    "refusing to overwrite an existing private credential/token file",
                ));
            }
            let (s, actor) = identity_store_and_actor(&o)?;
            let issued = s.issue_auth_session(
                &actor,
                "unix",
                None,
                ttl,
                serde_json::json!({"client":client.unwrap_or_else(||"aw-cli".into())}),
            )?;
            if let Err(error) = write_private_bytes_new(
                &out,
                issued.refresh_token.as_bytes(),
                "auth-session token output",
            ) {
                let _ = s.revoke_auth_session_by_token(&issued.refresh_token);
                return Err(error);
            }
            print(&issued.info, o.json)?;
        }
        AuthCommand::Verify { token, store: o } => {
            let value = read_refresh_token(&token)?;
            let (s, _) = identity_store_and_actor(&o)?;
            print(&s.authenticate_auth_session(&value)?, o.json)?;
        }
        AuthCommand::Rotate {
            token,
            out,
            store: o,
        } => {
            if out.exists() {
                return Err(Error::Conflict(
                    "refusing to overwrite an existing private credential/token file",
                ));
            }
            let value = read_refresh_token(&token)?;
            let (s, _) = identity_store_and_actor(&o)?;
            let issued = s.rotate_auth_session(&value)?;
            if let Err(error) = write_private_bytes_new(
                &out,
                issued.refresh_token.as_bytes(),
                "auth-session token output",
            ) {
                let _ = s.revoke_auth_session_by_token(&issued.refresh_token);
                return Err(error);
            }
            print(&issued.info, o.json)?;
        }
        AuthCommand::Revoke { token, store: o } => {
            let value = read_refresh_token(&token)?;
            let (s, _) = identity_store_and_actor(&o)?;
            print(&s.revoke_auth_session_by_token(&value)?, o.json)?;
        }
        AuthCommand::List { store: o } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            print(&s.auth_sessions(&actor)?, o.json)?;
        }
    }
    Ok(0)
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum ClientAuthArg {
    None,
    Post,
    Basic,
}
impl ClientAuthArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Post => "client_secret_post",
            Self::Basic => "client_secret_basic",
        }
    }
    fn needs_secret(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Subcommand, Debug)]
pub(crate) enum OidcProviderCommand {
    Create {
        provider: String,
        #[arg(long)]
        issuer: String,
        #[arg(long)]
        client_id: String,
        #[arg(long,value_enum,default_value_t=ClientAuthArg::None)]
        client_auth: ClientAuthArg,
        #[arg(long = "scope", required = true)]
        scopes: Vec<String>,
        #[arg(long)]
        client_secret_file: Option<PathBuf>,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    List {
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    RotateSecret {
        provider: String,
        #[arg(long)]
        secret_file: PathBuf,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    Revoke {
        provider: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
}

pub(crate) fn oidc_provider(command: OidcProviderCommand) -> Result<u8> {
    match command {
        OidcProviderCommand::Create {
            provider,
            issuer,
            client_id,
            client_auth,
            scopes,
            client_secret_file,
            store: o,
        } => {
            if client_auth.needs_secret() != client_secret_file.is_some() {
                return Err(Error::Config(
                    "OIDC client-secret file presence must match --client-auth".into(),
                ));
            }
            let secret = client_secret_file
                .as_deref()
                .map(|path| {
                    read_private_bytes(
                        path,
                        aw_store::MAX_SECRET_BYTES,
                        "OIDC client secret exceeds 64 KiB",
                    )
                    .map(Zeroizing::new)
                })
                .transpose()?;
            let (s, actor) = identity_store_and_actor(&o)?;
            let secrets = s.local_secret_store()?;
            print(
                &s.create_oidc_provider(
                    &actor,
                    &secrets,
                    &provider,
                    &issuer,
                    &client_id,
                    client_auth.as_str(),
                    scopes.into_iter().collect(),
                    secret.as_ref().map(|v| v.as_slice()),
                )?,
                o.json,
            )?;
        }
        OidcProviderCommand::List { store: o } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            print(&s.oidc_providers(&actor)?, o.json)?;
        }
        OidcProviderCommand::RotateSecret {
            provider,
            secret_file,
            store: o,
        } => {
            let secret = Zeroizing::new(read_private_bytes(
                &secret_file,
                aw_store::MAX_SECRET_BYTES,
                "OIDC client secret exceeds 64 KiB",
            )?);
            let (s, actor) = identity_store_and_actor(&o)?;
            let secrets = s.local_secret_store()?;
            print(
                &s.rotate_oidc_provider_secret(&actor, &secrets, &provider, secret.as_slice())?,
                o.json,
            )?;
        }
        OidcProviderCommand::Revoke { provider, store: o } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            let secrets = s.local_secret_store()?;
            print(
                &s.revoke_oidc_provider(&actor, &secrets, &provider)?,
                o.json,
            )?;
        }
    }
    Ok(0)
}

#[derive(Subcommand, Debug)]
pub(crate) enum OidcCommand {
    Login {
        provider: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 86400)]
        ttl: u64,
        #[arg(long, default_value = "127.0.0.1:0")]
        listen: SocketAddr,
        #[arg(long, default_value_t = 300)]
        timeout: u64,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    Link {
        provider: String,
        #[arg(long, default_value = "127.0.0.1:0")]
        listen: SocketAddr,
        #[arg(long, default_value_t = 300)]
        timeout: u64,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
}

#[cfg(feature = "oidc")]
fn login_runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::from)
}
#[cfg(feature = "oidc")]
fn show_authorization_url(url: &str) {
    eprintln!("Open this authorization URL in your browser:\n{url}");
}

pub(crate) fn oidc(command: OidcCommand) -> Result<u8> {
    #[cfg(not(feature = "oidc"))]
    {
        let _ = command;
        Err(Error::Config("this build has no OIDC/OAuth feature".into()))
    }
    #[cfg(feature = "oidc")]
    {
        match command {
            OidcCommand::Login {
                provider,
                out,
                ttl,
                listen,
                timeout,
                store: o,
            } => {
                if out.exists() {
                    return Err(Error::Conflict(
                        "refusing to overwrite an existing private credential/token file",
                    ));
                }
                let (s, _) = identity_store_and_actor(&o)?;
                let result = login_runtime()?.block_on(aw::oidc_flow::oidc_login(
                    &s,
                    &provider,
                    ttl,
                    listen,
                    Duration::from_secs(timeout),
                    show_authorization_url,
                ))?;
                if let Err(error) = write_private_bytes_new(
                    &out,
                    result.refresh_token.as_bytes(),
                    "OIDC native-session token output",
                ) {
                    let _ = s.revoke_auth_session_by_token(&result.refresh_token);
                    return Err(error);
                }
                print(
                    &serde_json::json!({"login":result.login,"session":result.session}),
                    o.json,
                )?;
            }
            OidcCommand::Link {
                provider,
                listen,
                timeout,
                store: o,
            } => {
                let (s, actor) = identity_store_and_actor(&o)?;
                let user = s.current_user(&actor)?;
                let result = login_runtime()?.block_on(aw::oidc_flow::oidc_link(
                    &s,
                    &provider,
                    &user.user_id,
                    listen,
                    Duration::from_secs(timeout),
                    show_authorization_url,
                ))?;
                print(&result, o.json)?;
            }
        }
        Ok(0)
    }
}

#[derive(Subcommand, Debug)]
pub(crate) enum OAuthConnectorCommand {
    Create {
        connector: String,
        #[arg(long)]
        authorization_endpoint: String,
        #[arg(long)]
        token_endpoint: String,
        #[arg(long)]
        client_id: String,
        #[arg(long,value_enum,default_value_t=ClientAuthArg::None)]
        client_auth: ClientAuthArg,
        #[arg(long = "scope", required = true)]
        scopes: Vec<String>,
        #[arg(long)]
        client_secret_file: Option<PathBuf>,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    List {
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    Authorize {
        connector: String,
        #[arg(long, default_value = "127.0.0.1:0")]
        listen: SocketAddr,
        #[arg(long, default_value_t = 300)]
        timeout: u64,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    Refresh {
        connector: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    RotateSecret {
        connector: String,
        #[arg(long)]
        secret_file: PathBuf,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
    Revoke {
        connector: String,
        #[command(flatten)]
        store: IdentityStoreArgs,
    },
}

pub(crate) fn oauth_connector(command: OAuthConnectorCommand) -> Result<u8> {
    match command {
        OAuthConnectorCommand::Create {
            connector,
            authorization_endpoint,
            token_endpoint,
            client_id,
            client_auth,
            scopes,
            client_secret_file,
            store: o,
        } => {
            if client_auth.needs_secret() != client_secret_file.is_some() {
                return Err(Error::Config(
                    "OAuth client-secret file presence must match --client-auth".into(),
                ));
            }
            let secret = client_secret_file
                .as_deref()
                .map(|path| {
                    read_private_bytes(
                        path,
                        aw_store::MAX_SECRET_BYTES,
                        "OAuth client secret exceeds 64 KiB",
                    )
                    .map(Zeroizing::new)
                })
                .transpose()?;
            let (s, actor) = identity_store_and_actor(&o)?;
            let secrets = s.local_secret_store()?;
            print(
                &s.create_oauth_connector(
                    &actor,
                    &secrets,
                    &connector,
                    &authorization_endpoint,
                    &token_endpoint,
                    &client_id,
                    client_auth.as_str(),
                    scopes.into_iter().collect(),
                    secret.as_ref().map(|v| v.as_slice()),
                )?,
                o.json,
            )?;
        }
        OAuthConnectorCommand::List { store: o } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            print(&s.oauth_connectors(&actor)?, o.json)?;
        }
        OAuthConnectorCommand::RotateSecret {
            connector,
            secret_file,
            store: o,
        } => {
            let secret = Zeroizing::new(read_private_bytes(
                &secret_file,
                aw_store::MAX_SECRET_BYTES,
                "OAuth client secret exceeds 64 KiB",
            )?);
            let (s, actor) = identity_store_and_actor(&o)?;
            let secrets = s.local_secret_store()?;
            print(
                &s.rotate_oauth_connector_client_secret(
                    &actor,
                    &secrets,
                    &connector,
                    secret.as_slice(),
                )?,
                o.json,
            )?;
        }
        OAuthConnectorCommand::Revoke {
            connector,
            store: o,
        } => {
            let (s, actor) = identity_store_and_actor(&o)?;
            let secrets = s.local_secret_store()?;
            print(
                &s.revoke_oauth_connector(&actor, &secrets, &connector)?,
                o.json,
            )?;
        }
        OAuthConnectorCommand::Authorize {
            connector,
            listen,
            timeout,
            store: o,
        } => {
            #[cfg(not(feature = "oidc"))]
            {
                let _ = (connector, listen, timeout, o);
                return Err(Error::Config("this build has no OIDC/OAuth feature".into()));
            }
            #[cfg(feature = "oidc")]
            {
                let (s, actor) = identity_store_and_actor(&o)?;
                let result = login_runtime()?.block_on(aw::oidc_flow::oauth_authorize(
                    &s,
                    &actor,
                    &connector,
                    listen,
                    Duration::from_secs(timeout),
                    show_authorization_url,
                ))?;
                print(&result, o.json)?;
            }
        }
        OAuthConnectorCommand::Refresh {
            connector,
            store: o,
        } => {
            #[cfg(not(feature = "oidc"))]
            {
                let _ = (connector, o);
                return Err(Error::Config("this build has no OIDC/OAuth feature".into()));
            }
            #[cfg(feature = "oidc")]
            {
                let (s, actor) = identity_store_and_actor(&o)?;
                let result = login_runtime()?
                    .block_on(aw::oidc_flow::oauth_refresh(&s, &actor, &connector))?;
                print(&result, o.json)?;
            }
        }
    }
    Ok(0)
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum RelayTransportArg {
    Udp,
    Tcp,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum NetServeRoleArg {
    Relay,
    Rendezvous,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum MachineReadArg {
    MachineInfo,
    Workspaces,
    FileList,
    FileRead,
    GitStatus,
    Sessions,
}

#[cfg(feature = "net")]
fn trusted_git_path() -> Result<PathBuf> {
    let path = std::env::var_os("AW_GIT")
        .map(PathBuf::from)
        .or_else(|| option_env!("AW_DEFAULT_GIT").map(PathBuf::from))
        .ok_or_else(|| {
            Error::Config(
                "remote Git read service requires trusted AW_GIT or build-time AW_DEFAULT_GIT"
                    .into(),
            )
        })?;
    if !path.is_absolute() {
        return Err(Error::Config(
            "remote Git read service requires an absolute trusted Git executable".into(),
        ));
    }
    Ok(path)
}

#[derive(Subcommand, Debug)]
pub(crate) enum NetCommand {
    Status {
        #[command(flatten)]
        store: StoreArgs,
    },
    Peers {
        #[command(flatten)]
        store: StoreArgs,
    },
    /// Create or accept a one-use direct QUIC pairing invitation.
    Pair {
        /// Pairing code from another AwNode. Omit to listen and create a code.
        code: Option<String>,
        #[command(flatten)]
        store: StoreArgs,
        /// Local UDP address for the temporary pairing listener.
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        /// Concrete address embedded in the pairing code (for NAT/manual routing).
        #[arg(long)]
        advertise: Option<SocketAddr>,
        /// Display name presented to the peer; defaults to the local Machine name.
        #[arg(long)]
        name: Option<String>,
        /// One-use listener lifetime in seconds (10..=900).
        #[arg(long, default_value_t = 300)]
        timeout: u64,
    },
    /// Discover host/server-reflexive UDP candidates without granting authority.
    Candidates {
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        /// STUN server(s) used only for XOR-MAPPED-ADDRESS discovery.
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long)]
        json: bool,
    },
    /// Generate a random rendezvous room identifier; this grants no authority.
    Room {
        #[arg(long)]
        json: bool,
    },
    /// Run a bounded one-shot untrusted UDP rendezvous helper.
    Rendezvous {
        #[arg(long, default_value = "127.0.0.1:0")]
        listen: SocketAddr,
        #[arg(long, default_value_t = 120)]
        timeout: u64,
        #[arg(long)]
        json: bool,
    },
    /// Establish a direct QUIC route to an already-paired peer through rendezvous punching.
    Punch {
        peer: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Run a bounded one-pair relay. UDP forwards raw QUIC datagrams; TCP forwards opaque TLS bytes.
    Relay {
        #[arg(long, value_enum, default_value_t = RelayTransportArg::Udp)]
        transport: RelayTransportArg,
        #[arg(long, default_value = "127.0.0.1:0")]
        listen: SocketAddr,
        #[arg(long, default_value_t = 120)]
        timeout: u64,
        #[arg(long)]
        json: bool,
    },
    /// Run reusable self-hosted AwNet coordination. Relay is the backwards-compatible default.
    Serve {
        #[arg(long, value_enum, default_value_t = NetServeRoleArg::Relay)]
        role: NetServeRoleArg,
        #[arg(long, default_value = "0.0.0.0:4433")]
        listen: SocketAddr,
        #[arg(long, default_value_t = 300)]
        lifetime: u64,
        #[arg(long, default_value_t = 120)]
        room_ttl: u64,
        #[arg(long, default_value_t = 256)]
        max_rooms: usize,
        #[arg(long)]
        json: bool,
    },
    /// Select the best available authenticated route in direct -> QUIC relay -> TLS/TCP order.
    Connect {
        peer: String,
        #[arg(long)]
        room: String,
        #[command(flatten)]
        store: StoreArgs,
        /// Temporary rendezvous for direct QUIC punching.
        #[arg(long)]
        rendezvous: Option<SocketAddr>,
        /// UDP relay for end-to-end QUIC when direct reachability fails.
        #[arg(long)]
        relay: Option<SocketAddr>,
        /// TCP relay carrying end-to-end TLS as the final fallback.
        #[arg(long = "tcp-relay")]
        tcp_relay: Option<SocketAddr>,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 15)]
        timeout: u64,
    },
    /// Serve one authenticated MachineLink capability negotiation over a punched QUIC route.
    MachineServe {
        peer: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long = "grant", default_value = "machine-info")]
        grant: Vec<String>,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Request a short-lived typed MachineLink lease from a paired peer.
    MachineConnect {
        peer: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long = "request", default_value = "machine-info")]
        request: Vec<String>,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Serve one signed Domain AuthoritySet quorum vote over a typed MachineLink.
    AuthorityVoteServe {
        peer: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Request one signed quorum vote for a collecting local Domain authority election.
    AuthorityVote {
        peer: String,
        election: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Serve one signed Domain authority-core snapshot to an AuthoritySet standby.
    AuthorityReplicaServe {
        peer: String,
        authority_domain: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Pull one signed Domain authority-core snapshot from the current authority leader.
    AuthorityReplicaSync {
        peer: String,
        authority_domain: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Request one signed quorum vote for a standby Domain authority promotion election.
    AuthorityReplicaVote {
        peer: String,
        election: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Serve one signed safe stable-user/device/OIDC metadata snapshot to an explicitly trusted peer.
    IdentityContinuityServe {
        peer: String,
        domain_id: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: IdentityStoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Pull one signed identity-continuity snapshot after a local continuity source grant exists.
    IdentityContinuitySync {
        peer: String,
        domain_id: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: IdentityStoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Serve one live stable-user device-key authentication challenge.
    IdentityAuthServe {
        peer: String,
        domain_id: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: IdentityStoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        /// Explicitly allow verified device proofs to mint native auth sessions up to this TTL.
        #[arg(long)]
        auth_session_max_ttl: Option<u64>,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Prove a local private user-device key to another AwNode and optionally authorize one Session.
    IdentityAuth {
        peer: String,
        domain_id: String,
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        session: Option<String>,
        /// Write a server-issued native refresh token to this new private file.
        #[arg(long)]
        auth_session_out: Option<PathBuf>,
        /// Requested native auth-session lifetime; requires --auth-session-out.
        #[arg(long, requires = "auth_session_out")]
        auth_session_ttl: Option<u64>,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: IdentityStoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Serve one signed semantic Workspace snapshot to its designated writer/failback peer.
    WorkspaceHandoffServe {
        peer: String,
        workspace: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Pull and durably apply one signed semantic Workspace handoff snapshot.
    WorkspaceHandoffSync {
        peer: String,
        workspace: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Serve one exact writer-epoch Workspace ref operation from the recorded control peer.
    WorkspaceWriterServe {
        peer: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Fence the previous remote writer with the exact current Domain-authorized writer event.
    WorkspaceWriterFence {
        peer: String,
        workspace: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Read exact branch refs from the current remote Workspace writer.
    WorkspaceWriterBranches {
        peer: String,
        workspace: String,
        session: String,
        #[arg(long)]
        writer_epoch: u64,
        #[arg(long)]
        authority_term: u64,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Advance one branch on the current remote writer using head+revision+writer-epoch CAS.
    WorkspaceWriterCheckpoint {
        peer: String,
        workspace: String,
        session: String,
        branch: String,
        #[arg(long)]
        writer_epoch: u64,
        #[arg(long)]
        authority_term: u64,
        #[arg(long)]
        expected_head: String,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long)]
        summary: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Create an explicit branch on the current remote Workspace writer.
    WorkspaceWriterFork {
        peer: String,
        workspace: String,
        session: String,
        name: String,
        at: String,
        #[arg(long)]
        writer_epoch: u64,
        #[arg(long)]
        authority_term: u64,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Serve one capability-bound typed read-only Machine service request.
    MachineServiceServe {
        peer: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long = "grant", default_value = "machine-info")]
        grant: Vec<String>,
        /// Locally chosen exported root. The remote peer cannot replace this path.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Make one typed read-only request under a short-lived MachineLink lease.
    MachineRead {
        peer: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, value_enum)]
        op: MachineReadArg,
        /// Relative path for file-list/file-read only.
        #[arg(long)]
        path: Option<String>,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Serve one federation-authorized semantic sync operation over MachineLink.
    FederationServe {
        peer: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(
            long = "grant",
            default_values_t = [
                String::from("semantic-sync-read"),
                String::from("semantic-sync-write")
            ]
        )]
        grant: Vec<String>,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Export semantic commit evidence from a peer under its federation lease.
    FederationSyncExport {
        peer: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[arg(long)]
        federation_lease: String,
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long, default_value_t = 128)]
        limit: usize,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Import one exact semantic sync batch into a peer under its federation lease.
    FederationSyncImport {
        peer: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[arg(long)]
        federation_lease: String,
        input: PathBuf,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Explicit owner grants one signed AwNet peer access to the current Actor.
    ActorPeerGrant {
        session_id: String, branch_id: String, peer: String,
        #[arg(long,default_value="local")] site: String,
        #[arg(long="permission", required=true)] permission: Vec<String>,
        #[arg(long,default_value_t=60)] ttl: u64,
        #[command(flatten)] store: StoreArgs,
    },
    /// Revoke a current peer Actor grant; prior signed MachineLink is insufficient.
    ActorPeerRevoke {
        session_id: String, grant_id: String,
        #[command(flatten)] store: StoreArgs,
    },
    /// Enumerate bounded grants under the owning Session.
    ActorPeerList {
        session_id: String,
        #[command(flatten)] store: StoreArgs,
    },
    /// Request one signed, capability-fenced Actor operation from a paired node.
    ActorRequest {
        peer: String, session_id: String, branch_id: String,
        #[arg(long)] room: String,
        #[arg(long)] rendezvous: SocketAddr,
        #[arg(long="request-file")] request_file: PathBuf,
        /// Optional concurrent exact active-Turn Control spec during Step.
        /// Explicit JSON {"delay_ms":100,"control":{"op":"control",...}}.
        #[arg(long="control-file")] control_file: Option<PathBuf>,
        #[command(flatten)] store: StoreArgs,
        #[arg(long,default_value_t=60)] lease: u64,
        #[arg(long,default_value="0.0.0.0:0")] listen: SocketAddr,
        #[arg(long="stun")] stun: Vec<SocketAddr>,
        #[arg(long,default_value_t=30)] timeout: u64,
    },
    /// Submit one exact prompt to a delegated remote Session. Reuse --execution-id only for lookup/recovery.
    RemoteSessionExecute {
        peer: String,
        session_id: String,
        branch_id: String,
        prompt: String,
        #[arg(long)]
        execution_id: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
        /// Validation-only: cancel the accepted run on the same MachineLink after this delay.
        #[arg(long = "__cancel-after-ms", hide = true)]
        cancel_after_ms: Option<u64>,
    },
    /// Query remote Session owner/execution state without submitting work.
    RemoteSessionStatus {
        peer: String,
        session_id: String,
        branch_id: String,
        #[arg(long)]
        execution_id: Option<String>,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Read the durable transcript/history of a delegated remote Session.
    RemoteSessionTranscript {
        peer: String,
        session_id: String,
        branch_id: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Request cancellation of the exact currently active remote run.
    RemoteSessionCancel {
        peer: String,
        session_id: String,
        branch_id: String,
        run_id: u64,
        #[arg(long)]
        execution_id: Option<String>,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Serve one idempotent durable remote Session creation request in a PeerWorkspace.
    RemoteSessionServe {
        peer: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        /// Local execution root bound to the created remote Session.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
        /// Validation fault injection: create durably, then drop the response.
        #[arg(long = "__drop-response-after-create", hide = true)]
        drop_response_after_create: bool,
    },
    /// Create or retry one durable remote Session by immutable logical operation ID.
    RemoteSessionCreate {
        peer: String,
        #[arg(long)]
        room: String,
        #[arg(long)]
        rendezvous: SocketAddr,
        #[command(flatten)]
        store: StoreArgs,
        /// Source Workspace selector; defaults to the source Domain's reserved local Workspace.
        #[arg(long, default_value = "local")]
        source_workspace: String,
        #[arg(long)]
        source_session: Option<String>,
        #[arg(long, requires = "source_session")]
        source_branch: Option<String>,
        #[arg(long, default_value = "Remote session")]
        title: String,
        /// Reuse this exact operation ID after an unknown outcome.
        #[arg(long)]
        operation_id: Option<String>,
        #[arg(long, default_value_t = 60)]
        lease: u64,
        #[arg(long, default_value = "0.0.0.0:0")]
        listen: SocketAddr,
        #[arg(long = "stun")]
        stun: Vec<SocketAddr>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Inspect durable source-side RemoteSessionRefs without contacting a peer.
    RemoteSessions {
        #[command(flatten)]
        store: StoreArgs,
        #[arg(long)]
        operation_id: Option<String>,
    },
}
pub(crate) fn net(command: NetCommand) -> Result<u8> {
    match command {
        NetCommand::Status { store: o } => {
            let (s, _) = store_and_actor(&o)?;
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                print(&crate::awnet::status(&s, &dir)?, o.json)?;
            }
            #[cfg(not(feature = "net"))]
            print(&s.node_status()?, o.json)?;
        }
        NetCommand::Peers { store: o } => {
            let (s, _) = store_and_actor(&o)?;
            print(&s.awnet_peers()?, o.json)?;
        }
        NetCommand::Pair {
            code,
            store: o,
            listen,
            advertise,
            name,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (code, o, listen, advertise, name, timeout);
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, _) = store_and_actor(&o)?;
                let display_name = match name {
                    Some(v) => v,
                    None => s.local_machine()?.display_name,
                };
                let peer = if let Some(code) = code {
                    crate::awnet::accept_pairing_code(&s, &dir, &code, &display_name)?
                } else {
                    let opts = crate::awnet::OfferOptions {
                        listen,
                        advertise,
                        display_name,
                        timeout: Duration::from_secs(timeout),
                    };
                    crate::awnet::offer_pair(&s, &dir, opts, |offer| {
                        print(offer, o.json)?;
                        let mut stdout = std::io::stdout();
                        stdout.flush()?;
                        Ok(())
                    })?
                };
                print(&serde_json::json!({"paired":peer}), o.json)?;
            }
        }
        NetCommand::Candidates { listen, stun, json } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (listen, stun, json);
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let socket = std::net::UdpSocket::bind(listen)?;
                print(&crate::awnet::gather_candidates(&socket, &stun)?, json)?;
            }
        }
        NetCommand::Room { json } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = json;
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            print(&serde_json::json!({"room":crate::awnet::new_room()?}), json)?;
        }
        NetCommand::Rendezvous {
            listen,
            timeout,
            json,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (listen, timeout, json);
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let matched = crate::awnet::serve_rendezvous(
                    crate::awnet::RendezvousOptions {
                        listen,
                        timeout: Duration::from_secs(timeout),
                    },
                    |bound| {
                        print(bound, json)?;
                        let mut stdout = std::io::stdout();
                        stdout.flush()?;
                        Ok(())
                    },
                )?;
                print(&serde_json::json!({"matched":matched}), json)?;
            }
        }
        NetCommand::Punch {
            peer,
            room,
            rendezvous,
            store: o,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (peer, room, rendezvous, o, listen, stun, timeout);
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, _) = store_and_actor(&o)?;
                let route = crate::awnet::punch_peer(
                    &s,
                    &dir,
                    crate::awnet::PunchOptions {
                        peer_node_id: peer,
                        room,
                        rendezvous,
                        listen,
                        stun,
                        timeout: Duration::from_secs(timeout),
                    },
                )?;
                print(&serde_json::json!({"route":route}), o.json)?;
            }
        }
        NetCommand::Relay {
            transport,
            listen,
            timeout,
            json,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (transport, listen, timeout, json);
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let opts = crate::awnet::RelayServeOptions {
                    listen,
                    timeout: Duration::from_secs(timeout),
                };
                let announce = |bound: &crate::awnet::RelayBound| {
                    print(bound, json)?;
                    let mut stdout = std::io::stdout();
                    stdout.flush()?;
                    Ok(())
                };
                let matched = match transport {
                    RelayTransportArg::Udp => crate::awnet::serve_udp_relay(opts, announce)?,
                    RelayTransportArg::Tcp => crate::awnet::serve_tcp_relay(opts, announce)?,
                };
                print(&serde_json::json!({"matched":matched}), json)?;
            }
        }
        NetCommand::Serve {
            role,
            listen,
            lifetime,
            room_ttl,
            max_rooms,
            json,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (role, listen, lifetime, room_ttl, max_rooms, json);
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let announce = |bound: &crate::awnet::RendezvousBound| {
                    print(bound, json)?;
                    let mut stdout = std::io::stdout();
                    stdout.flush()?;
                    Ok(())
                };
                match role {
                    NetServeRoleArg::Relay => {
                        let stats = crate::awnet::serve_udp_relay_service(
                            crate::awnet::RelayServiceOptions {
                                listen,
                                lifetime: Duration::from_secs(lifetime),
                                room_ttl: Duration::from_secs(room_ttl),
                                max_rooms,
                            },
                            |bound| {
                                print(bound, json)?;
                                let mut stdout = std::io::stdout();
                                stdout.flush()?;
                                Ok(())
                            },
                        )?;
                        print(&stats, json)?;
                    }
                    NetServeRoleArg::Rendezvous => {
                        let stats = crate::awnet::serve_rendezvous_service(
                            crate::awnet::RendezvousServiceOptions {
                                listen,
                                lifetime: Duration::from_secs(lifetime),
                                room_ttl: Duration::from_secs(room_ttl),
                                max_rooms,
                            },
                            announce,
                        )?;
                        print(&stats, json)?;
                    }
                }
            }
        }
        NetCommand::Connect {
            peer,
            room,
            store: o,
            rendezvous,
            relay,
            tcp_relay,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer, room, o, rendezvous, relay, tcp_relay, listen, stun, timeout,
                );
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                if rendezvous.is_none() && relay.is_none() && tcp_relay.is_none() {
                    return Err(Error::Config(
                        "aw net connect needs at least one rendezvous/relay route".into(),
                    ));
                }
                let dir = store_dir(&o)?;
                let (s, _) = store_and_actor(&o)?;
                let timeout = Duration::from_secs(timeout);
                let selection = crate::awnet::select_route(
                    &s,
                    &dir,
                    crate::awnet::SelectRouteOptions {
                        direct: rendezvous.map(|rendezvous| crate::awnet::PunchOptions {
                            peer_node_id: peer.clone(),
                            room: room.clone(),
                            rendezvous,
                            listen,
                            stun,
                            timeout,
                        }),
                        quic_relay: relay.map(|relay| crate::awnet::RelayQuicOptions {
                            peer_node_id: peer.clone(),
                            room: room.clone(),
                            relay,
                            listen,
                            timeout,
                        }),
                        tcp_relay: tcp_relay.map(|relay| crate::awnet::TlsTcpRelayOptions {
                            peer_node_id: peer,
                            room,
                            relay,
                            timeout,
                        }),
                    },
                )?;
                print(&selection, o.json)?;
            }
        }
        NetCommand::MachineServe {
            peer,
            room,
            rendezvous,
            store: o,
            grant,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer, room, rendezvous, o, grant, lease, listen, stun, timeout,
                );
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let allowed = grant
                    .iter()
                    .map(|value| crate::awnet::MachineCapability::parse(value))
                    .collect::<Result<BTreeSet<_>>>()?;
                let dir = store_dir(&o)?;
                let (s, _) = store_and_actor(&o)?;
                let lease = crate::awnet::serve_machine_link(
                    &s,
                    &dir,
                    crate::awnet::MachineLinkServeOptions {
                        route: crate::awnet::PunchOptions {
                            peer_node_id: peer,
                            room,
                            rendezvous,
                            listen,
                            stun,
                            timeout: Duration::from_secs(timeout),
                        },
                        allowed,
                        lease: Duration::from_secs(lease),
                    },
                )?;
                print(&serde_json::json!({"machine_link":lease}), o.json)?;
            }
        }
        NetCommand::MachineConnect {
            peer,
            room,
            rendezvous,
            store: o,
            request,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer, room, rendezvous, o, request, lease, listen, stun, timeout,
                );
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let requested = request
                    .iter()
                    .map(|value| crate::awnet::MachineCapability::parse(value))
                    .collect::<Result<BTreeSet<_>>>()?;
                let dir = store_dir(&o)?;
                let (s, _) = store_and_actor(&o)?;
                let lease = crate::awnet::connect_machine_link(
                    &s,
                    &dir,
                    crate::awnet::MachineLinkConnectOptions {
                        route: crate::awnet::PunchOptions {
                            peer_node_id: peer,
                            room,
                            rendezvous,
                            listen,
                            stun,
                            timeout: Duration::from_secs(timeout),
                        },
                        requested,
                        lease: Duration::from_secs(lease),
                    },
                )?;
                print(&serde_json::json!({"machine_link":lease}), o.json)?;
            }
        }
        NetCommand::AuthorityVoteServe {
            peer,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (peer, room, rendezvous, o, lease, listen, stun, timeout);
                return Err(Error::Config(
                    "Domain authority voting requires the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, _) = store_and_actor(&o)?;
                let result = crate::awnet::serve_authority_vote(
                    &s,
                    &dir,
                    crate::awnet::AuthorityVoteServeOptions {
                        link: crate::awnet::MachineLinkServeOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            allowed: BTreeSet::from([
                                crate::awnet::MachineCapability::DomainAuthorityVote,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::AuthorityVote {
            peer,
            election,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer, election, room, rendezvous, o, lease, listen, stun, timeout,
                );
                return Err(Error::Config(
                    "Domain authority voting requires the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, actor) = store_and_actor(&o)?;
                let result = crate::awnet::request_authority_vote(
                    &s,
                    &dir,
                    crate::awnet::AuthorityVoteRequestOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::DomainAuthorityVote,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        actor,
                        election_id: election,
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::AuthorityReplicaServe {
            peer,
            authority_domain,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer,
                    authority_domain,
                    room,
                    rendezvous,
                    o,
                    lease,
                    listen,
                    stun,
                    timeout,
                );
                return Err(Error::Config(
                    "Domain authority replication requires the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, actor) = store_and_actor(&o)?;
                let result = crate::awnet::serve_authority_replica_snapshot(
                    &s,
                    &dir,
                    crate::awnet::AuthorityReplicaServeOptions {
                        link: crate::awnet::MachineLinkServeOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            allowed: BTreeSet::from([
                                crate::awnet::MachineCapability::DomainAuthorityReplicate,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        actor,
                        authority_domain_id: authority_domain,
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::AuthorityReplicaSync {
            peer,
            authority_domain,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer,
                    authority_domain,
                    room,
                    rendezvous,
                    o,
                    lease,
                    listen,
                    stun,
                    timeout,
                );
                return Err(Error::Config(
                    "Domain authority replication requires the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, actor) = store_and_actor(&o)?;
                let result = crate::awnet::request_authority_replica_snapshot(
                    &s,
                    &dir,
                    crate::awnet::AuthorityReplicaRequestOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::DomainAuthorityReplicate,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        actor,
                        authority_domain_id: authority_domain,
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::AuthorityReplicaVote {
            peer,
            election,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer, election, room, rendezvous, o, lease, listen, stun, timeout,
                );
                return Err(Error::Config(
                    "Domain authority voting requires the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, actor) = store_and_actor(&o)?;
                let result = crate::awnet::request_authority_replica_vote(
                    &s,
                    &dir,
                    crate::awnet::AuthorityReplicaVoteRequestOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::DomainAuthorityVote,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        actor,
                        election_id: election,
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::IdentityContinuityServe {
            peer,
            domain_id,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer, domain_id, room, rendezvous, o, lease, listen, stun, timeout,
                );
                return Err(Error::Config(
                    "identity continuity requires the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = identity_store_dir(&o)?;
                let (s, actor) = identity_store_and_actor(&o)?;
                let result = crate::awnet::serve_identity_continuity(
                    &s,
                    &dir,
                    crate::awnet::IdentityContinuityServeOptions {
                        link: crate::awnet::MachineLinkServeOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            allowed: BTreeSet::from([
                                crate::awnet::MachineCapability::IdentityContinuity,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        actor,
                        domain_id,
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::IdentityContinuitySync {
            peer,
            domain_id,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer, domain_id, room, rendezvous, o, lease, listen, stun, timeout,
                );
                return Err(Error::Config(
                    "identity continuity requires the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = identity_store_dir(&o)?;
                let (s, actor) = identity_store_and_actor(&o)?;
                let result = crate::awnet::request_identity_continuity(
                    &s,
                    &dir,
                    crate::awnet::IdentityContinuitySyncOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::IdentityContinuity,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        actor,
                        domain_id,
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::IdentityAuthServe {
            peer,
            domain_id,
            room,
            rendezvous,
            store: o,
            lease,
            auth_session_max_ttl,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer,
                    domain_id,
                    room,
                    rendezvous,
                    o,
                    lease,
                    auth_session_max_ttl,
                    listen,
                    stun,
                    timeout,
                );
                return Err(Error::Config(
                    "identity authentication requires the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = identity_store_dir(&o)?;
                let (s, _) = identity_store_and_actor(&o)?;
                let result = crate::awnet::serve_identity_auth(
                    &s,
                    &dir,
                    crate::awnet::IdentityAuthServeOptions {
                        link: crate::awnet::MachineLinkServeOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            allowed: BTreeSet::from([
                                crate::awnet::MachineCapability::IdentityAuthenticate,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        domain_id,
                        auth_session_max_ttl,
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::IdentityAuth {
            peer,
            domain_id,
            key,
            session,
            auth_session_out,
            auth_session_ttl,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer,
                    domain_id,
                    key,
                    session,
                    auth_session_out,
                    auth_session_ttl,
                    room,
                    rendezvous,
                    o,
                    lease,
                    listen,
                    stun,
                    timeout,
                );
                return Err(Error::Config(
                    "identity authentication requires the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let signing_key = load_user_device_key(&key)?;
                let dir = identity_store_dir(&o)?;
                let (s, _) = identity_store_and_actor(&o)?;
                let result = crate::awnet::request_identity_auth(
                    &s,
                    &dir,
                    crate::awnet::IdentityAuthRequestOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::IdentityAuthenticate,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        domain_id,
                        session_id: session,
                        auth_session_ttl: auth_session_out
                            .as_ref()
                            .map(|_| auth_session_ttl.unwrap_or(86_400)),
                        signing_key,
                    },
                )?;
                if let Some(path) = auth_session_out {
                    let token = result.refresh_token.as_ref().ok_or(Error::Denied(
                        "remote AwNode did not issue the requested native auth session",
                    ))?;
                    write_private_bytes_new(
                        &path,
                        token.as_bytes(),
                        "native auth-session token output",
                    )?;
                }
                print(&result, o.json)?;
            }
        }
        NetCommand::WorkspaceHandoffServe {
            peer,
            workspace,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer, workspace, room, rendezvous, o, lease, listen, stun, timeout,
                );
                return Err(Error::Config(
                    "Workspace replica handoff requires the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, actor) = store_and_actor(&o)?;
                let result = crate::awnet::serve_workspace_replica_snapshot(
                    &s,
                    &dir,
                    crate::awnet::WorkspaceReplicaServeOptions {
                        link: crate::awnet::MachineLinkServeOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            allowed: BTreeSet::from([
                                crate::awnet::MachineCapability::WorkspaceReplicaHandoff,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        actor,
                        workspace_id: workspace,
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::WorkspaceHandoffSync {
            peer,
            workspace,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer, workspace, room, rendezvous, o, lease, listen, stun, timeout,
                );
                return Err(Error::Config(
                    "Workspace replica handoff requires the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, actor) = store_and_actor(&o)?;
                let result = crate::awnet::request_workspace_replica_snapshot(
                    &s,
                    &dir,
                    crate::awnet::WorkspaceReplicaRequestOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::WorkspaceReplicaHandoff,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        actor,
                        workspace_id: workspace,
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::WorkspaceWriterServe {
            peer,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (peer, room, rendezvous, o, lease, listen, stun, timeout);
                return Err(Error::Config(
                    "Workspace writer ref service requires the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, _) = store_and_actor(&o)?;
                let result = crate::awnet::serve_workspace_writer_request(
                    &s,
                    &dir,
                    crate::awnet::WorkspaceWriterServeOptions {
                        link: crate::awnet::MachineLinkServeOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            allowed: BTreeSet::from([
                                crate::awnet::MachineCapability::WorkspaceWriterRef,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::WorkspaceWriterFence {
            peer,
            workspace,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer, workspace, room, rendezvous, o, lease, listen, stun, timeout,
                );
                return Err(Error::Config(
                    "Workspace writer ref service requires the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, actor) = store_and_actor(&o)?;
                let domain = match o.domain.as_deref() {
                    Some(value) => Some(s.domain(&actor, value)?.id),
                    None => None,
                };
                let writer = s.workspace_writer(&actor, domain.as_deref(), &workspace)?;
                let events = s.workspace_writer_events(&actor, domain.as_deref(), &workspace)?;
                let event = events.last().cloned().ok_or(Error::Storage(
                    "Workspace writer history is missing the current handoff event",
                ))?;
                if event.new_writer_epoch != writer.writer_epoch
                    || event.new_writer_node_id != writer.writer_node_id
                    || event.authority_term != writer.authority_term
                {
                    return Err(Error::Conflict(
                        "latest Workspace writer event does not match current writer state",
                    ));
                }
                let result = crate::awnet::request_workspace_writer_service(
                    &s,
                    &dir,
                    crate::awnet::WorkspaceWriterRequestOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::WorkspaceWriterRef,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        request: crate::awnet::WorkspaceWriterRequest::Fence {
                            workspace_id: writer.workspace_id,
                            event,
                        },
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::WorkspaceWriterBranches {
            peer,
            workspace,
            session,
            writer_epoch,
            authority_term,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer,
                    workspace,
                    session,
                    writer_epoch,
                    authority_term,
                    room,
                    rendezvous,
                    o,
                    lease,
                    listen,
                    stun,
                    timeout,
                );
                return Err(Error::Config(
                    "Workspace writer ref service requires the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, actor) = store_and_actor(&o)?;
                let actor_user_id = s.current_user(&actor)?.user_id;
                let result = crate::awnet::request_workspace_writer_service(
                    &s,
                    &dir,
                    crate::awnet::WorkspaceWriterRequestOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::WorkspaceWriterRef,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        request: crate::awnet::WorkspaceWriterRequest::Branches {
                            actor_user_id,
                            workspace_id: workspace,
                            writer_epoch,
                            authority_term,
                            session_id: session,
                        },
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::WorkspaceWriterCheckpoint {
            peer,
            workspace,
            session,
            branch,
            writer_epoch,
            authority_term,
            expected_head,
            expected_revision,
            summary,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer,
                    workspace,
                    session,
                    branch,
                    writer_epoch,
                    authority_term,
                    expected_head,
                    expected_revision,
                    summary,
                    room,
                    rendezvous,
                    o,
                    lease,
                    listen,
                    stun,
                    timeout,
                );
                return Err(Error::Config(
                    "Workspace writer ref service requires the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, actor) = store_and_actor(&o)?;
                let actor_user_id = s.current_user(&actor)?.user_id;
                let result = crate::awnet::request_workspace_writer_service(
                    &s,
                    &dir,
                    crate::awnet::WorkspaceWriterRequestOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::WorkspaceWriterRef,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        request: crate::awnet::WorkspaceWriterRequest::Checkpoint {
                            actor_user_id,
                            workspace_id: workspace,
                            writer_epoch,
                            authority_term,
                            session_id: session,
                            branch_id: branch,
                            expected_head,
                            expected_revision,
                            summary,
                        },
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::WorkspaceWriterFork {
            peer,
            workspace,
            session,
            name,
            at,
            writer_epoch,
            authority_term,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer,
                    workspace,
                    session,
                    name,
                    at,
                    writer_epoch,
                    authority_term,
                    room,
                    rendezvous,
                    o,
                    lease,
                    listen,
                    stun,
                    timeout,
                );
                return Err(Error::Config(
                    "Workspace writer ref service requires the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, actor) = store_and_actor(&o)?;
                let actor_user_id = s.current_user(&actor)?.user_id;
                let result = crate::awnet::request_workspace_writer_service(
                    &s,
                    &dir,
                    crate::awnet::WorkspaceWriterRequestOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::WorkspaceWriterRef,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        request: crate::awnet::WorkspaceWriterRequest::Fork {
                            actor_user_id,
                            workspace_id: workspace,
                            writer_epoch,
                            authority_term,
                            session_id: session,
                            name,
                            at,
                        },
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::MachineServiceServe {
            peer,
            room,
            rendezvous,
            store: o,
            grant,
            root,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer, room, rendezvous, o, grant, root, lease, listen, stun, timeout,
                );
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let allowed = grant
                    .iter()
                    .map(|value| crate::awnet::MachineCapability::parse(value))
                    .collect::<Result<BTreeSet<_>>>()?;
                let dir = store_dir(&o)?;
                let (s, actor) = store_and_actor(&o)?;
                let result = crate::awnet::serve_machine_service(
                    &s,
                    &dir,
                    crate::awnet::MachineServiceServeOptions {
                        link: crate::awnet::MachineLinkServeOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            allowed,
                            lease: Duration::from_secs(lease),
                        },
                        actor,
                        root,
                        git: trusted_git_path()?,
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::MachineRead {
            peer,
            room,
            rendezvous,
            store: o,
            op,
            path,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer, room, rendezvous, o, op, path, lease, listen, stun, timeout,
                );
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let request = match op {
                    MachineReadArg::MachineInfo => {
                        if path.is_some() {
                            return Err(Error::Config(
                                "--path is only valid for file-list/file-read".into(),
                            ));
                        }
                        crate::awnet::MachineReadRequest::MachineInfo
                    }
                    MachineReadArg::Workspaces => {
                        if path.is_some() {
                            return Err(Error::Config(
                                "--path is only valid for file-list/file-read".into(),
                            ));
                        }
                        crate::awnet::MachineReadRequest::Workspaces
                    }
                    MachineReadArg::FileList => crate::awnet::MachineReadRequest::FileList {
                        path: path.unwrap_or_default(),
                    },
                    MachineReadArg::FileRead => crate::awnet::MachineReadRequest::FileRead {
                        path: path
                            .ok_or_else(|| Error::Config("file-read requires --path".into()))?,
                    },
                    MachineReadArg::GitStatus => {
                        if path.is_some() {
                            return Err(Error::Config(
                                "--path is only valid for file-list/file-read".into(),
                            ));
                        }
                        crate::awnet::MachineReadRequest::GitStatus
                    }
                    MachineReadArg::Sessions => {
                        if path.is_some() {
                            return Err(Error::Config(
                                "--path is only valid for file-list/file-read".into(),
                            ));
                        }
                        crate::awnet::MachineReadRequest::Sessions
                    }
                };
                let required = request.required_capability();
                let requested = BTreeSet::from([required]);
                let dir = store_dir(&o)?;
                let (s, _) = store_and_actor(&o)?;
                let result = crate::awnet::request_machine_service(
                    &s,
                    &dir,
                    crate::awnet::MachineServiceRequestOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested,
                            lease: Duration::from_secs(lease),
                        },
                        request,
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::FederationServe {
            peer,
            room,
            rendezvous,
            store: o,
            grant,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer, room, rendezvous, o, grant, lease, listen, stun, timeout,
                );
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let allowed = grant
                    .iter()
                    .map(|value| crate::awnet::MachineCapability::parse(value))
                    .collect::<Result<BTreeSet<_>>>()?;
                let dir = store_dir(&o)?;
                let (s, actor) = store_and_actor(&o)?;
                let result = crate::awnet::serve_federation_request(
                    &s,
                    &dir,
                    crate::awnet::FederationServeOptions {
                        link: crate::awnet::MachineLinkServeOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            allowed,
                            lease: Duration::from_secs(lease),
                        },
                        actor,
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::FederationSyncExport {
            peer,
            room,
            rendezvous,
            federation_lease,
            after,
            limit,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer,
                    room,
                    rendezvous,
                    federation_lease,
                    after,
                    limit,
                    o,
                    lease,
                    listen,
                    stun,
                    timeout,
                );
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, _) = store_and_actor(&o)?;
                let result = crate::awnet::request_federation_service(
                    &s,
                    &dir,
                    crate::awnet::FederationRequestOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::SemanticSyncRead,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        request: crate::awnet::FederationRequest::SemanticSyncExport {
                            federation_lease_id: federation_lease,
                            after_seq: after,
                            limit,
                        },
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::FederationSyncImport {
            peer,
            room,
            rendezvous,
            federation_lease,
            input,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer,
                    room,
                    rendezvous,
                    federation_lease,
                    input,
                    o,
                    lease,
                    listen,
                    stun,
                    timeout,
                );
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let bytes = std::fs::read(&input)?;
                if bytes.len() > 4 * 1024 * 1024 {
                    return Err(Error::Limit("semantic sync batch file exceeds 4 MiB"));
                }
                let batch: aw_store::SemanticSyncBatch = serde_json::from_slice(&bytes)
                    .map_err(|_| Error::Config("invalid semantic sync batch JSON".into()))?;
                let dir = store_dir(&o)?;
                let (s, _) = store_and_actor(&o)?;
                let result = crate::awnet::request_federation_service(
                    &s,
                    &dir,
                    crate::awnet::FederationRequestOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::SemanticSyncWrite,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        request: crate::awnet::FederationRequest::SemanticSyncImport {
                            federation_lease_id: federation_lease,
                            batch,
                        },
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::RemoteSessionExecute {
            peer,
            session_id,
            branch_id,
            prompt,
            execution_id,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
            cancel_after_ms,
        } => {
            #[cfg(not(all(feature = "net", target_os = "linux")))]
            {
                let _ = (
                    peer,
                    session_id,
                    branch_id,
                    prompt,
                    execution_id,
                    room,
                    rendezvous,
                    o,
                    lease,
                    listen,
                    stun,
                    timeout,
                    cancel_after_ms,
                );
                return Err(Error::Config(
                    "remote Session execution currently requires Linux AwNet".into(),
                ));
            }
            #[cfg(all(feature = "net", target_os = "linux"))]
            {
                if cancel_after_ms.is_some_and(|ms| ms == 0 || ms > 10_000) {
                    return Err(Error::Config(
                        "validation cancel delay must be between 1 and 10000 ms".into(),
                    ));
                }
                let dir = store_dir(&o)?;
                let (s, _) = store_and_actor(&o)?;
                let mut requested =
                    BTreeSet::from([crate::awnet::MachineCapability::RemoteSessionExecute]);
                if cancel_after_ms.is_some() {
                    requested.insert(crate::awnet::MachineCapability::RemoteSessionCancel);
                }
                let mut connection = crate::awnet::RemoteControlConnection::connect(
                    &s,
                    &dir,
                    crate::awnet::MachineLinkConnectOptions {
                        route: crate::awnet::PunchOptions {
                            peer_node_id: peer,
                            room,
                            rendezvous,
                            listen,
                            stun,
                            timeout: Duration::from_secs(timeout),
                        },
                        requested,
                        lease: Duration::from_secs(lease),
                    },
                )?;
                let execute = connection.request(crate::awnet::RemoteControlRequest::Execute {
                    session_id: session_id.clone(),
                    branch_id: branch_id.clone(),
                    execution_id: execution_id.clone(),
                    prompt,
                })?;
                if let Some(delay_ms) = cancel_after_ms {
                    let run_id = match &execute.response {
                        crate::awnet::RemoteControlResponse::Execution {
                            receipt: Some(receipt),
                            ..
                        } => receipt.run_id,
                        _ => {
                            connection.close();
                            return Err(Error::Conflict(
                                "remote execution did not return an active run receipt",
                            ));
                        }
                    };
                    std::thread::sleep(Duration::from_millis(delay_ms));
                    let cancel =
                        connection.request(crate::awnet::RemoteControlRequest::Cancel {
                            session_id,
                            branch_id,
                            run_id,
                            execution_id: Some(execution_id),
                        })?;
                    connection.close();
                    print(
                        &serde_json::json!({
                            "execute": execute,
                            "cancel": cancel,
                        }),
                        o.json,
                    )?;
                } else {
                    connection.close();
                    print(&execute, o.json)?;
                }
            }
        }
        NetCommand::RemoteSessionStatus {
            peer,
            session_id,
            branch_id,
            execution_id,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(all(feature = "net", target_os = "linux")))]
            {
                let _ = (
                    peer,
                    session_id,
                    branch_id,
                    execution_id,
                    room,
                    rendezvous,
                    o,
                    lease,
                    listen,
                    stun,
                    timeout,
                );
                return Err(Error::Config(
                    "remote Session status currently requires Linux AwNet".into(),
                ));
            }
            #[cfg(all(feature = "net", target_os = "linux"))]
            {
                let dir = store_dir(&o)?;
                let (s, _) = store_and_actor(&o)?;
                let result = crate::awnet::request_remote_control(
                    &s,
                    &dir,
                    crate::awnet::RemoteControlRequestOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::RemoteSessionStatus,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        request: crate::awnet::RemoteControlRequest::Status {
                            session_id,
                            branch_id,
                            execution_id,
                        },
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::RemoteSessionTranscript {
            peer,
            session_id,
            branch_id,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(all(feature = "net", target_os = "linux")))]
            {
                let _ = (
                    peer, session_id, branch_id, room, rendezvous, o, lease, listen, stun, timeout,
                );
                return Err(Error::Config(
                    "remote Session transcript currently requires Linux AwNet".into(),
                ));
            }
            #[cfg(all(feature = "net", target_os = "linux"))]
            {
                let dir = store_dir(&o)?;
                let (s, _) = store_and_actor(&o)?;
                let result = crate::awnet::request_remote_control(
                    &s,
                    &dir,
                    crate::awnet::RemoteControlRequestOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::RemoteSessionTranscript,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        request: crate::awnet::RemoteControlRequest::Transcript {
                            session_id,
                            branch_id,
                        },
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::RemoteSessionCancel {
            peer,
            session_id,
            branch_id,
            run_id,
            execution_id,
            room,
            rendezvous,
            store: o,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(all(feature = "net", target_os = "linux")))]
            {
                let _ = (
                    peer,
                    session_id,
                    branch_id,
                    run_id,
                    execution_id,
                    room,
                    rendezvous,
                    o,
                    lease,
                    listen,
                    stun,
                    timeout,
                );
                return Err(Error::Config(
                    "remote Session cancellation currently requires Linux AwNet".into(),
                ));
            }
            #[cfg(all(feature = "net", target_os = "linux"))]
            {
                let dir = store_dir(&o)?;
                let (s, _) = store_and_actor(&o)?;
                let result = crate::awnet::request_remote_control(
                    &s,
                    &dir,
                    crate::awnet::RemoteControlRequestOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::RemoteSessionCancel,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        request: crate::awnet::RemoteControlRequest::Cancel {
                            session_id,
                            branch_id,
                            run_id,
                            execution_id,
                        },
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::RemoteSessionServe {
            peer,
            room,
            rendezvous,
            store: o,
            root,
            lease,
            listen,
            stun,
            timeout,
            drop_response_after_create,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer,
                    room,
                    rendezvous,
                    o,
                    root,
                    lease,
                    listen,
                    stun,
                    timeout,
                    drop_response_after_create,
                );
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                let dir = store_dir(&o)?;
                let (s, actor) = store_and_actor(&o)?;
                let result = crate::awnet::serve_remote_session(
                    &s,
                    &dir,
                    crate::awnet::RemoteSessionServeOptions {
                        link: crate::awnet::MachineLinkServeOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            allowed: BTreeSet::from([
                                crate::awnet::MachineCapability::RemoteSession,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        actor,
                        root,
                        drop_response_after_create,
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::RemoteSessionCreate {
            peer,
            room,
            rendezvous,
            store: o,
            source_workspace,
            source_session,
            source_branch,
            title,
            operation_id,
            lease,
            listen,
            stun,
            timeout,
        } => {
            #[cfg(not(feature = "net"))]
            {
                let _ = (
                    peer,
                    room,
                    rendezvous,
                    o,
                    source_workspace,
                    source_session,
                    source_branch,
                    title,
                    operation_id,
                    lease,
                    listen,
                    stun,
                    timeout,
                );
                return Err(Error::Config(
                    "this build disabled the AwNet QUIC feature".into(),
                ));
            }
            #[cfg(feature = "net")]
            {
                if source_session.is_some() != source_branch.is_some() {
                    return Err(Error::Config(
                        "--source-session and --source-branch must be supplied together".into(),
                    ));
                }
                let dir = store_dir(&o)?;
                let (s, actor) = store_and_actor(&o)?;
                let workspace = if source_workspace == "local" {
                    s.local_workspace(&actor, o.domain.as_deref())?
                } else {
                    let domain = match o.domain.as_deref() {
                        Some(selector) => Some(s.domain(&actor, selector)?.id),
                        None => None,
                    };
                    s.workspace(&actor, domain.as_deref(), &source_workspace)?
                };
                let result = crate::awnet::create_remote_session(
                    &s,
                    &dir,
                    crate::awnet::RemoteSessionCreateOptions {
                        link: crate::awnet::MachineLinkConnectOptions {
                            route: crate::awnet::PunchOptions {
                                peer_node_id: peer,
                                room,
                                rendezvous,
                                listen,
                                stun,
                                timeout: Duration::from_secs(timeout),
                            },
                            requested: BTreeSet::from([
                                crate::awnet::MachineCapability::RemoteSession,
                            ]),
                            lease: Duration::from_secs(lease),
                        },
                        actor,
                        source_workspace: workspace,
                        source_session_id: source_session,
                        source_branch_id: source_branch,
                        title,
                        operation_id,
                    },
                )?;
                print(&result, o.json)?;
            }
        }
        NetCommand::ActorPeerGrant {session_id,branch_id,peer,site,permission,ttl,store:o} => {
            let (s,owner)=store_and_actor(&o)?;
            let info=s.session(&owner,&session_id)?;
            let epoch=s.workspace_writer(&owner,None,&info.workspace_id)?.writer_epoch;
            let grants=permission.iter().map(|label| match label.as_str() {
                "query"=>Ok(aw_store::ActorPeerPermission::Query),
                "inspect"=>Ok(aw_store::ActorPeerPermission::Inspect),
                "enqueue"=>Ok(aw_store::ActorPeerPermission::Enqueue),
                "step"=>Ok(aw_store::ActorPeerPermission::Step),
                "control"=>Ok(aw_store::ActorPeerPermission::Control),
                "notice-read"=>Ok(aw_store::ActorPeerPermission::NoticeRead),
                _=>Err(Error::Denied("unknown explicit Actor peer permission")),
            }).collect::<Result<BTreeSet<_>>>()?;
            print(&s.grant_actor_peer(&owner,&session_id,&branch_id,&site,&peer,epoch,grants,ttl)?,o.json)?;
        }
        NetCommand::ActorPeerRevoke {session_id,grant_id,store:o} => {
            let (s,owner)=store_and_actor(&o)?;
            let info=s.session(&owner,&session_id)?;
            let epoch=s.workspace_writer(&owner,None,&info.workspace_id)?.writer_epoch;
            print(&s.revoke_actor_peer(&owner,&session_id,&grant_id,epoch)?,o.json)?;
        }
        NetCommand::ActorPeerList {session_id,store:o} => {
            let (s,owner)=store_and_actor(&o)?;
            print(&s.list_actor_peers(&owner,&session_id)?,o.json)?;
        }
        NetCommand::ActorRequest{peer,session_id,branch_id,room,rendezvous,request_file,
            control_file,store:o,lease,listen,stun,timeout}=>{
            #[cfg(not(all(feature="net",target_os="linux")))]
            {
                let _=(peer,session_id,branch_id,room,rendezvous,request_file,control_file,o,
                    lease,listen,stun,timeout);
                return Err(Error::Denied("signed Actor remote transport requires Linux AwNet"));
            }
            #[cfg(all(feature="net",target_os="linux"))]
            {
                let data=std::fs::read(&request_file)?;
                if data.is_empty()||data.len()>192*1024 {
                    return Err(Error::Limit("Actor remote request must fit in 192 KiB"));
                }
                let request:crate::awnet::ActorRemoteRequest=serde_json::from_slice(&data)
                    .map_err(|_|Error::Denied("invalid typed Actor remote request"))?;
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct ConcurrentControl {
                    delay_ms:u64,
                    control:crate::awnet::ActorRemoteRequest,
                }
                let control=if let Some(path)=control_file {
                    let bytes=std::fs::read(path)?;
                    if bytes.len()>4096 || bytes.is_empty() {
                        return Err(Error::Limit("Actor concurrent Control specification must fit 4KiB"));
                    }
                    let command:ConcurrentControl=serde_json::from_slice(&bytes)
                        .map_err(|_|Error::Denied("invalid strictly typed Actor concurrent Control specification"))?;
                    if !matches!(request,crate::awnet::ActorRemoteRequest::Step {}) {
                        return Err(Error::Denied("concurrent Actor Control requires a real Step"));
                    }
                    Some(command)
                }else{None};
                let mut requested=BTreeSet::from([request.capability()]);
                if control.is_some(){requested.insert(crate::awnet::MachineCapability::ActorControl);}
                let dir=store_dir(&o)?;
                let (s,_)=store_and_actor(&o)?;
                let connection=crate::awnet::ActorRemoteClient::connect(&s,&dir,
                    crate::awnet::MachineLinkConnectOptions {
                        route:crate::awnet::PunchOptions {
                            peer_node_id:peer,room,rendezvous,listen,stun,
                            timeout:Duration::from_secs(timeout),
                        },
                        requested,
                        lease:Duration::from_secs(lease),
                    },&session_id,&branch_id)?;
                if let Some(control)=control {
                    let result=connection.step_and_control(control.control,control.delay_ms);
                    connection.close();
                    let (step,signal)=result?;
                    print(&serde_json::json!({"step":step,"control":signal}),o.json)?;
                }else{
                    let result=connection.request(request);
                    connection.close();
                    print(&result?,o.json)?;
                }
            }
        }
        NetCommand::RemoteSessions {
            store: o,
            operation_id,
        } => {
            let (s, actor) = store_and_actor(&o)?;
            if let Some(operation_id) = operation_id {
                print(&s.remote_session_ref(&actor, &operation_id)?, o.json)?;
            } else {
                print(&s.remote_session_refs(&actor)?, o.json)?;
            }
        }
    }
    Ok(0)
}
