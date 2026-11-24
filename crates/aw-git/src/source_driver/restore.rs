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

/// Independently approved physical locations for one named restore. These
/// identities are observations, not authority: the host must hold the live
/// repository and destination fence across planning, effects and publication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitRestoreLocation {
    pub name: SourceName,
    pub repository: PathBuf,
    pub repository_identity: (u64, u64),
    pub target: PathBuf,
    pub parent_identity: (u64, u64),
}

impl GitRestoreLocation {
    fn check_directories(&self) -> Result<()> {
        target_text(&self.target)?;
        let parent = self
            .target
            .parent()
            .ok_or(Error::Denied("Git restore destination has no parent"))?;
        for (path, expected) in [
            (self.repository.as_path(), self.repository_identity),
            (parent, self.parent_identity),
        ] {
            if !path.is_absolute() || path.canonicalize()? != path {
                return Err(Error::Denied(
                    "Git restore approval requires canonical directories",
                ));
            }
            let pin = aw_platform::DirectoryIdentity::open(path)?;
            if pin.identity()? != expected || !pin.is_current(path)? {
                return Err(Error::Conflict("approved Git restore directory changed"));
            }
        }
        // An approved checkout root is not permission to discover a
        // repository by ascending into its unapproved ancestors.
        let dotgit = self.repository.join(".git");
        let metadata = fs::symlink_metadata(&dotgit)?;
        if !(metadata.is_file() || metadata.is_dir()) || aw_platform::path_is_link_like(&dotgit)? {
            return Err(Error::Denied(
                "Git restore approval requires an exact checkout root",
            ));
        }
        Ok(())
    }
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

    fn require_binding(&self, binding: &SourceBinding) -> Result<()> {
        let (_, path) = materialization_parts(binding)?;
        let mut expected = self.origin.clone();
        expected
            .materialization
            .clone_from(&binding.materialization);
        if path != self.target || expected != *binding {
            return Err(Error::Conflict(
                "Git retirement differs from original restore binding",
            ));
        }
        Ok(())
    }

    fn verify_location(
        &self,
        driver: &GitSourceDriver,
        location: &GitRestoreLocation,
    ) -> Result<()> {
        let (_, origin) = materialization_parts(&self.origin)?;
        if location.name != self.anchor.name
            || origin != location.repository
            || self.target != location.target
        {
            return Err(Error::Denied(
                "Git restore descriptor differs from approved locations",
            ));
        }
        location.check_directories()?;
        self.check_parent()?;
        driver.verify_materialization(
            &self.origin,
            &location.repository,
            location.repository_identity,
        )?;
        Ok(())
    }

    fn origin_repository(&self, driver: &GitSourceDriver) -> Result<Repository> {
        self.check_parent()?;
        let repo = driver.binding_repository(&self.origin)?;
        repo.guard_managed_checkout().map_err(git_error)?;
        Ok(repo)
    }

    fn repository(&self, driver: &GitSourceDriver) -> Result<Repository> {
        let repo = self.origin_repository(driver)?;
        verify_retained(&repo, &self.anchor)?;
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
    if !lock_matches(&repo.git_dir, &expected)? {
        return Err(Error::Conflict(
            "Git restored worktree belongs to another operation",
        ));
    }
    Ok(())
}

fn lock_matches(admin: &Path, expected: &str) -> Result<bool> {
    let file = match aw_platform::open_nofollow_read(&admin.join("locked")) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if !file.metadata()?.is_file() {
        return Err(Error::Conflict(
            "Git restore operation marker is not a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take((expected.len() + 1) as u64)
        .read_to_end(&mut bytes)?;
    Ok(bytes == expected.as_bytes())
}

/// The old pathname may be absent while the original operation-marked
/// worktree survives under another name (or with a broken registration).
fn require_released(
    origin: &Repository,
    plan: &RestorePlan,
    prepared: &PreparedSourceOperation,
) -> Result<()> {
    check_fresh(origin, &plan.target)?;
    let admin = origin.common.join("worktrees");
    let entries = match fs::read_dir(&admin) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return plan.check_parent(),
        Err(error) => return Err(error.into()),
    };
    let expected = format!("{}\n", lock_reason(prepared));
    for (index, entry) in entries.enumerate() {
        if index >= 1024 {
            return Err(Error::Limit("Git worktree administration count"));
        }
        let path = entry?.path();
        if !fs::symlink_metadata(&path)?.is_dir() || aw_platform::path_is_link_like(&path)? {
            return Err(Error::Conflict("Git worktree administration changed"));
        }
        if lock_matches(&path, &expected)? {
            return Err(Error::Conflict(
                "original Git restore registration still exists",
            ));
        }
    }
    plan.check_parent()
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

fn remove_verified_restore(
    origin: &Repository,
    child: &Repository,
    plan: &RestorePlan,
    prepared: &PreparedSourceOperation,
) -> Result<()> {
    let admin = child.git_dir.clone();
    // Keep the original lock marker until Git removes the worktree.
    // Git requires two force flags for a locked checkout (and a dirty
    // captured tree). The exact tree/index/marker checks above and the
    // independent live physical fence are prerequisites, not a generic
    // discard switch. No recursive caller-side deletion or broad prune.
    let args = [
        "worktree",
        "remove",
        "--force",
        "--force",
        "--",
        target_text(&plan.target)?,
    ]
    .map(OsString::from);
    run_os(origin, &args, None, None).map_err(|_| {
        Error::Conflict("Git restore retirement uncertain; inspect original materialization")
    })?;
    plan.check_parent()?;
    absent(&admin)?;
    require_released(origin, plan, prepared)
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
        Self::plan_restore_in(&repo, anchor, target, prepare_id)
    }

    /// Plan only at the exact independently approved checkout and private
    /// destination parent. Never resolve a persisted locator as authority.
    pub fn plan_restore_approved(
        &self,
        location: &GitRestoreLocation,
        anchor: &GitSourceAnchor,
        prepare_id: &str,
    ) -> Result<PreparedSourceOperation> {
        if location.name != anchor.name {
            return Err(Error::Denied("Git restore approval names another source"));
        }
        location.check_directories()?;
        let repo = self.repository(&location.repository)?;
        if repo.root() != location.repository
            || repo.root_identity().map_err(git_error)? != location.repository_identity
        {
            return Err(Error::Conflict(
                "Git restore approval differs from repository root",
            ));
        }
        Self::plan_restore_in(&repo, anchor, &location.target, prepare_id)
    }

    /// Before executing or inspecting any original descriptor, require the
    /// current host-approved locations to match it exactly. No Git probe
    /// at an unapproved persisted origin or destination.
    pub fn verify_restore_location(
        &self,
        location: &GitRestoreLocation,
        anchor: &GitSourceAnchor,
        prepared: &PreparedSourceOperation,
    ) -> Result<()> {
        let plan = RestorePlan::decode(prepared)?;
        if plan.anchor != *anchor {
            return Err(Error::Denied(
                "Git restore descriptor differs from checkpoint anchor",
            ));
        }
        plan.verify_location(self, location)
    }

    /// Verify the exact independently approved origin/target of a published
    /// restore before retirement. The original Prepared descriptor carries
    /// the anchor identity even after checkpoint retention has been released.
    /// This is read-only preflight, not proof that the materialization was
    /// released or authority to remove it.
    pub fn verify_restored_materialization_location(
        &self,
        location: &GitRestoreLocation,
        prepared: &PreparedSourceOperation,
        binding: &SourceBinding,
    ) -> Result<()> {
        let plan = RestorePlan::decode(prepared)?;
        plan.require_binding(binding)?;
        plan.verify_location(self, location)
    }

    fn plan_restore_in(
        repo: &Repository,
        anchor: &GitSourceAnchor,
        target: &Path,
        prepare_id: &str,
    ) -> Result<PreparedSourceOperation> {
        verify_retained(repo, anchor)?;
        let origin = binding_for_repo(
            repo,
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
        check_fresh(repo, target)?;
        plan.trees(repo)?;
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

    /// Reconcile an already-published, retired restore materialization.
    /// Trusted caller holds the source, repository and destination fences,
    /// independently authorizes disposal of this exact managed checkout,
    /// and excludes other live backing dependencies through settlement.
    /// The original Prepared operation and published binding are identities,
    /// not grants. New work, changed index/HEAD/lock or foreign content
    /// requires explicit reconciliation; never discard it implicitly.
    pub fn release_restored_materialization(
        &self,
        prepared: &PreparedSourceOperation,
        binding: &SourceBinding,
    ) -> Result<()> {
        let plan = RestorePlan::decode(prepared)?;
        plan.require_binding(binding)?;

        // The checkpoint may already be retired. The original descriptor
        // and checkout binding suffice for ownership; retained object
        // content is needed only when there is still a worktree to remove.
        let origin = plan.origin_repository(self)?;
        match fs::symlink_metadata(&plan.target) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // Both the target and its registration must be absent.
                // Partial removal is Unknown; never prune or unlock it.
                return require_released(&origin, &plan, prepared);
            }
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }

        let child = restored_repo(&origin, &plan, prepared)?;
        if plan.binding(&child)? != *binding {
            return Err(Error::Conflict("Git retirement checkout identity changed"));
        }
        let trees = plan.trees(&origin)?;
        verify_restored(&child, &trees.base, trees.captured())?;
        validate_restored_repo(&origin, &child, &plan, prepared)?;
        remove_verified_restore(&origin, &child, &plan, prepared)
    }

    /// Reconcile only the original unpublished candidate, under separately
    /// authorized repository/destination fences. Exact completed restores,
    /// untouched registered --no-checkout candidates, and proven absence
    /// can be settled. Partial/modified content stays Unknown.
    pub fn release_prepared_restore(&self, prepared: &PreparedSourceOperation) -> Result<()> {
        let plan = RestorePlan::decode(prepared)?;
        let origin = plan.origin_repository(self)?;
        match fs::symlink_metadata(&plan.target) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return require_released(&origin, &plan, prepared)
            }
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let child = restored_repo(&origin, &plan, prepared)?;
        child.guard_managed_checkout().map_err(git_error)?;
        PrivateIndexes::require_absent(&child, &prepared.descriptor.prepared_identity)?;
        let untouched = match fs::symlink_metadata(child.git_dir.join("index")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut untouched = true;
                for entry in fs::read_dir(child.root())? {
                    if entry?.file_name() != ".git" {
                        untouched = false;
                        break;
                    }
                }
                untouched
            }
            Err(error) => return Err(error.into()),
            Ok(_) => false,
        };
        if !untouched {
            let trees = plan.trees(&origin)?;
            verify_restored(&child, &trees.base, trees.captured())?;
        }
        validate_restored_repo(&origin, &child, &plan, prepared)?;
        remove_verified_restore(&origin, &child, &plan, prepared)
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
