//! Prepared restore into a fresh, host-reserved Git worktree.
//! Git's worktree lock carries operation identity, not write authority.
use super::tree::{check_restore_gitlinks, committed_tree, verify_restored, PrivateIndexes, Tree};
use super::{
    access_name, binding_for_repo, descriptor_field, git_error, hash_directory, head, is_digest,
    key, materialization_parts, parse_access, parse_policy, policy_name, prepare_operation, run_os,
    verify_retained, GitSourceAnchor, GitSourceDriver, BINDING_SCHEMA, RESTORE_RECOVERY_SCHEMA,
};
use crate::Repository;
use aw_agent_core::{
    Error, ExternalEvidence, PreparedSourceOperation, RecoveryField, RecoveryFieldKind,
    RecoveryInspection, Result, SourceBinding, SourceName, SourceOperationKind,
};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

struct RestorePlan {
    anchor: GitSourceAnchor,
    origin: SourceBinding,
    target: PathBuf,
    parent_stamp: String,
}

fn target_text(path: &Path) -> Result<&str> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(Error::Denied(
            "Git restore requires an absolute fresh destination",
        ));
    }
    path.to_str()
        .ok_or(Error::Denied("Git restore requires a UTF-8 destination"))
}

impl RestorePlan {
    fn prepare(&self, prepare_id: &str) -> Result<PreparedSourceOperation> {
        use RecoveryFieldKind::{Digest, Id, Reference, Version};
        let anchor = &self.anchor;
        anchor.validate()?;
        materialization_parts(&self.origin)?;
        let target = target_text(&self.target)?;
        if self.origin.source_ref != anchor.source_ref || !is_digest(&self.parent_stamp) {
            return Err(Error::Denied("invalid Git restore identity"));
        }
        // Check the future SourceBinding's bound before materialization.
        let mut candidate = self.origin.clone();
        candidate.materialization = format!("{}:{target}", self.parent_stamp);
        materialization_parts(&candidate)?;
        let anchor_schema = anchor.anchor_schema.to_string();
        let fields = [
            ("checkpoint", Id, anchor.checkpoint_id.as_str()),
            ("name", Id, anchor.name.as_str()),
            ("source", Reference, anchor.source_ref.as_str()),
            (
                "materialization",
                Reference,
                self.origin.materialization.as_str(),
            ),
            ("anchor_schema", Version, &anchor_schema),
            ("base", Version, anchor.base_commit.as_str()),
            ("captured", Version, anchor.captured_commit.as_str()),
            ("access", Id, access_name(anchor.access)),
            (
                "checkpoint_policy",
                Id,
                policy_name(anchor.checkpoint_policy),
            ),
            ("target", Reference, target),
            ("parent", Digest, self.parent_stamp.as_str()),
        ]
        .into_iter()
        .map(|(name, kind, value)| RecoveryField::new(name, kind, value))
        .collect::<Result<Vec<_>>>()?;
        prepare_operation(
            SourceOperationKind::Restore,
            RESTORE_RECOVERY_SCHEMA,
            b"aw-git-restore-v1\0",
            prepare_id,
            fields,
        )
    }

    fn decode(prepared: &PreparedSourceOperation) -> Result<Self> {
        prepared.validate()?;
        let d = &prepared.descriptor;
        if prepared.operation != SourceOperationKind::Restore
            || d.driver != key()
            || d.recovery_schema != RESTORE_RECOVERY_SCHEMA
        {
            return Err(Error::Denied("incompatible Git restore descriptor"));
        }
        let field = |name| descriptor_field(d, name);
        let anchor = GitSourceAnchor {
            driver: key(),
            anchor_schema: field("anchor_schema")?
                .parse()
                .map_err(|_| Error::Storage("invalid Git restore anchor schema"))?,
            source_ref: field("source")?.into(),
            name: SourceName::new(field("name")?)?,
            access: parse_access(field("access")?)?,
            checkpoint_policy: parse_policy(field("checkpoint_policy")?)?,
            checkpoint_id: field("checkpoint")?.into(),
            base_commit: field("base")?.into(),
            captured_commit: field("captured")?.into(),
        };
        let origin = SourceBinding {
            name: anchor.name.clone(),
            driver: key(),
            binding_schema: BINDING_SCHEMA,
            source_ref: anchor.source_ref.clone(),
            materialization: field("materialization")?.into(),
            access: anchor.access,
            checkpoint_policy: anchor.checkpoint_policy,
        };
        let plan = Self {
            anchor,
            origin,
            target: PathBuf::from(field("target")?),
            parent_stamp: field("parent")?.into(),
        };
        if plan.prepare(&d.prepare_id)? != *prepared {
            return Err(Error::Storage(
                "Git restore descriptor does not match its identity",
            ));
        }
        Ok(plan)
    }

    fn check_parent(&self) -> Result<()> {
        if parent_stamp(&self.target)? != self.parent_stamp {
            return Err(Error::Conflict("Git restore destination parent changed"));
        }
        Ok(())
    }

    fn repository(&self, driver: &GitSourceDriver) -> Result<Repository> {
        self.check_parent()?;
        let repo = driver.binding_repository(&self.origin)?;
        verify_retained(&repo, &self.anchor)?;
        repo.guard_managed_checkout().map_err(git_error)?;
        Ok(repo)
    }

    fn trees(&self, repo: &Repository) -> Result<RestoreTrees> {
        let base = committed_tree(repo, &self.anchor.base_commit)?;
        let captured = self
            .anchor
            .is_dirty()
            .then(|| committed_tree(repo, &self.anchor.captured_commit))
            .transpose()?;
        check_restore_gitlinks(&base, captured.as_ref().unwrap_or(&base))?;
        Ok(RestoreTrees { base, captured })
    }

    fn binding(&self, repo: &Repository) -> Result<SourceBinding> {
        let binding = binding_for_repo(
            repo,
            self.anchor.name.clone(),
            self.anchor.access,
            self.anchor.checkpoint_policy,
        )?;
        if binding.source_ref != self.anchor.source_ref {
            return Err(Error::Conflict("restored Git object database changed"));
        }
        Ok(binding)
    }
}

struct RestoreTrees {
    base: Tree,
    captured: Option<Tree>,
}
impl RestoreTrees {
    fn captured(&self) -> &Tree {
        self.captured.as_ref().unwrap_or(&self.base)
    }
}

// Host-selected destination must be an absent direct child of a private,
// existing canonical directory. Bind the parent's identity across restart.
fn parent_stamp(target: &Path) -> Result<String> {
    target_text(target)?;
    let parent = target
        .parent()
        .ok_or(Error::Denied("Git restore destination has no parent"))?;
    let canonical = parent.canonicalize()?;
    if canonical.join(
        target
            .file_name()
            .ok_or(Error::Denied("Git restore destination has no name"))?,
    ) != target
    {
        return Err(Error::Denied("Git restore destination must be canonical"));
    }
    aw_platform::validate_private_dir(&canonical)?;
    let identity = aw_platform::DirectoryIdentity::open(&canonical)?;
    let mut hash = Sha256::new();
    hash.update(b"aw-git-restore-parent-v1\0");
    hash_directory(&mut hash, &canonical, &identity)?;
    Ok(format!("{:x}", hash.finalize()))
}

fn absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(_) => Err(Error::Conflict(
            "Git restore destination already exists; inspect the Prepared operation",
        )),
    }
}

fn overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

fn check_fresh(repo: &Repository, target: &Path) -> Result<()> {
    absent(target)?;
    if overlaps(target, &repo.common) || overlaps(target, &repo.git_dir) {
        return Err(Error::Denied(
            "Git restore destination overlaps repository administration",
        ));
    }
    let admin = repo.common.join("worktrees");
    match fs::symlink_metadata(&admin) {
        Ok(meta) if !meta.is_dir() || aw_platform::path_is_link_like(&admin)? => {
            return Err(Error::Denied(
                "Git worktree administration is not a real directory",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    for worktree in repo.worktrees().map_err(git_error)? {
        if overlaps(target, &worktree.path) {
            return Err(Error::Denied(
                "Git restore destination overlaps a registered worktree",
            ));
        }
        match worktree.path.canonicalize() {
            Ok(path) if overlaps(target, &path) => {
                return Err(Error::Denied(
                    "Git restore destination overlaps a registered worktree",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn lock_reason(prepared: &PreparedSourceOperation) -> String {
    format!("aw-restore-v1:{}", prepared.descriptor.prepared_identity)
}

fn check_target(plan: &RestorePlan) -> Result<()> {
    plan.check_parent()?;
    let target = &plan.target;
    let meta = fs::symlink_metadata(target)?;
    if !meta.is_dir()
        || aw_platform::path_is_link_like(target)?
        || target.canonicalize()? != *target
    {
        return Err(Error::Conflict("Git restore destination identity changed"));
    }
    let dotgit = target.join(".git");
    if !fs::symlink_metadata(&dotgit)?.is_file() || aw_platform::path_is_link_like(&dotgit)? {
        return Err(Error::Conflict("Git restore requires a linked worktree"));
    }
    Ok(())
}

// Only the exact registered, detached, operation-marked worktree can be
// inspected or materialized. Neither the path nor the lock alone is sufficient.
fn validate_restored_repo(
    origin: &Repository,
    repo: &Repository,
    plan: &RestorePlan,
    prepared: &PreparedSourceOperation,
) -> Result<()> {
    check_target(plan)?;
    repo.validate_binding().map_err(git_error)?;
    let target = &plan.target;
    let mut registered = origin
        .worktrees()
        .map_err(git_error)?
        .into_iter()
        .filter(|w| w.path == *target);
    let entry = registered.next().ok_or(Error::Conflict(
        "Git restored worktree registration unavailable",
    ))?;
    if registered.next().is_some()
        || entry.bare
        || entry.prunable
        || !entry.locked
        || !entry.detached
        || entry.head.as_deref() != Some(&plan.anchor.base_commit)
    {
        return Err(Error::Conflict(
            "Git restored worktree registration changed",
        ));
    }
    if repo.root() != target
        || repo.common != origin.common
        || repo.git_dir.parent() != Some(origin.common.join("worktrees").as_path())
        || head(repo)? != plan.anchor.base_commit
    {
        return Err(Error::Conflict("Git restored worktree binding changed"));
    }
    let expected = format!("{}\n", lock_reason(prepared));
    let file = aw_platform::open_nofollow_read(&repo.git_dir.join("locked"))?;
    if !file.metadata()?.is_file() {
        return Err(Error::Conflict(
            "Git restore operation marker is not a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take((expected.len() + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes != expected.as_bytes() {
        return Err(Error::Conflict(
            "Git restored worktree belongs to another operation",
        ));
    }
    Ok(())
}

fn restored_repo(
    origin: &Repository,
    plan: &RestorePlan,
    prepared: &PreparedSourceOperation,
) -> Result<Repository> {
    check_target(plan)?;
    let repo = Repository::discover(origin.git(), &plan.target).map_err(git_error)?;
    validate_restored_repo(origin, &repo, plan, prepared)?;
    Ok(repo)
}

fn materialize(
    repo: &Repository,
    plan: &RestorePlan,
    prepared: &PreparedSourceOperation,
) -> Result<()> {
    repo.guard_managed_checkout().map_err(git_error)?;
    // A newly registered --no-checkout worktree contains only its .git file.
    // Never overwrite materialization or an index that is already present.
    for entry in fs::read_dir(repo.root())? {
        if entry?.file_name() != ".git" {
            return Err(Error::Conflict("Git restore destination is not empty"));
        }
    }
    let index = repo.git_dir.join("index");
    absent(&index)?;
    let args = [
        "read-tree",
        "--no-sparse-checkout",
        &plan.anchor.base_commit,
    ]
    .map(OsString::from);
    run_os(repo, &args, None, Some(&index))?;
    let checkout = |index: &Path| {
        run_os(
            repo,
            &["checkout-index".into(), "--all".into()],
            None,
            Some(index),
        )
    };
    if plan.anchor.is_dirty() {
        let private = PrivateIndexes::create(repo, &prepared.descriptor.prepared_identity)?;
        let captured_index = private.index(1);
        let args = [
            "read-tree",
            "--no-sparse-checkout",
            &plan.anchor.captured_commit,
        ]
        .map(OsString::from);
        run_os(repo, &args, None, Some(&captured_index))?;
        checkout(&captured_index)?;
    } else {
        checkout(&index)?;
    }
    Ok(())
}

fn inspect_binding(
    origin: &Repository,
    repo: &Repository,
    plan: &RestorePlan,
    prepared: &PreparedSourceOperation,
    trees: &RestoreTrees,
) -> Result<SourceBinding> {
    verify_restored(repo, &trees.base, trees.captured())?;
    let binding = plan.binding(repo)?;
    // Keep the observed checkout identity, rather than rediscovering a path
    // that may now identify another worktree.
    validate_restored_repo(origin, repo, plan, prepared)?;
    verify_retained(origin, &plan.anchor)?;
    Ok(binding)
}

impl GitSourceDriver {
    /// Read-only restore planning. The trusted host chooses a private fresh
    /// destination and authorizes both source retention and materialization.
    /// Persist the returned descriptor before any external effect.
    pub fn plan_restore(
        &self,
        repository: &Path,
        anchor: &GitSourceAnchor,
        target: &Path,
        prepare_id: &str,
    ) -> Result<PreparedSourceOperation> {
        let repo = self.repository(repository)?;
        verify_retained(&repo, anchor)?;
        let origin = binding_for_repo(
            &repo,
            anchor.name.clone(),
            anchor.access,
            anchor.checkpoint_policy,
        )?;
        let plan = RestorePlan {
            anchor: anchor.clone(),
            origin,
            target: target.to_owned(),
            parent_stamp: parent_stamp(target)?,
        };
        check_fresh(&repo, target)?;
        plan.trees(&repo)?;
        plan.prepare(prepare_id)
    }

    /// First attempt only, under the caller's live source/destination barrier
    /// and durable Prepared custody. An uncertain result is inspected, not
    /// blindly retried or removed. No synthetic commit becomes visible HEAD.
    pub fn execute_restore(&self, prepared: &PreparedSourceOperation) -> Result<SourceBinding> {
        let plan = RestorePlan::decode(prepared)?;
        let repo = plan.repository(self)?;
        let trees = plan.trees(&repo)?;
        check_fresh(&repo, &plan.target)?;
        plan.check_parent()?;
        let args = [
            "worktree",
            "add",
            "--detach",
            "--no-checkout",
            "--lock",
            "--reason",
            &lock_reason(prepared),
            target_text(&plan.target)?,
            &plan.anchor.base_commit,
        ]
        .map(OsString::from);
        // No force, no checkout hooks, no submodule initialization or fetch.
        run_os(&repo, &args, None, None).map_err(|_| {
            Error::Conflict("Git restore outcome uncertain; inspect the Prepared operation")
        })?;
        let child = restored_repo(&repo, &plan, prepared)?;
        materialize(&child, &plan, prepared)?;
        inspect_binding(&repo, &child, &plan, prepared, &trees)
    }

    /// Read-only recovery. Absence, partial checkout, changed contents or
    /// mismatched identity are Unknown, never permission to create again.
    /// Completed reports observed materialization, not authority to publish.
    pub fn inspect_restore(
        &self,
        prepared: &PreparedSourceOperation,
    ) -> Result<RecoveryInspection<SourceBinding>> {
        let plan = RestorePlan::decode(prepared)?;
        let result = (|| {
            let repo = plan.repository(self)?;
            let trees = plan.trees(&repo)?;
            let child = restored_repo(&repo, &plan, prepared)?;
            inspect_binding(&repo, &child, &plan, prepared, &trees)
        })();
        let Ok(binding) = result else {
            return Ok(RecoveryInspection::Unknown);
        };
        // Mutable worktree evidence requires the live AW-owned local source
        // and destination fence through commit. The handle and Git lock are
        // identities only; neither is a lease or a write barrier.
        let evidence = ExternalEvidence::held_fence(
            "aw_local_git_restore",
            &prepared.descriptor.prepared_identity,
        )?;
        RecoveryInspection::completed(binding, evidence)
    }

    /// Revalidate exact materialization under the live host barrier before
    /// publication. The caller must keep that barrier held through AW commit.
    pub fn revalidate_restore(
        &self,
        prepared: &PreparedSourceOperation,
        binding: &SourceBinding,
    ) -> Result<()> {
        let plan = RestorePlan::decode(prepared)?;
        let repo = plan.repository(self)?;
        let trees = plan.trees(&repo)?;
        let child = restored_repo(&repo, &plan, prepared)?;
        if inspect_binding(&repo, &child, &plan, prepared, &trees)? != *binding {
            return Err(Error::Conflict(
                "Git restored binding differs from Prepared operation",
            ));
        }
        Ok(())
    }
}
