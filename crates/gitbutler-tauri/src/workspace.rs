use crate::error::Error;
use but_core::UnifiedDiff;
use but_workspace::StackEntry;
use gitbutler_command_context::CommandContext;
use gitbutler_oxidize::OidExt;
use gitbutler_project as projects;
use gitbutler_project::ProjectId;
use gitbutler_settings::AppSettingsWithDiskSync;
use gitbutler_stack::StackId;
use gix::bstr::ByteSlice;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::hash::Hasher;
use std::path::Path;
use tauri::State;
use tracing::instrument;

#[tauri::command(async)]
#[instrument(skip(projects), err(Debug))]
pub fn stacks(
    projects: State<'_, projects::Controller>,
    project_id: ProjectId,
) -> Result<Vec<StackEntry>, Error> {
    let project = projects.get(project_id)?;
    but_workspace::stacks(&project.gb_dir()).map_err(Into::into)
}

#[tauri::command(async)]
#[instrument(skip(projects, settings), err(Debug))]
pub fn stack_branches(
    projects: State<'_, projects::Controller>,
    settings: State<'_, AppSettingsWithDiskSync>,
    project_id: ProjectId,
    stack_id: String,
) -> Result<Vec<but_workspace::Branch>, Error> {
    let project = projects.get(project_id)?;
    let ctx = CommandContext::open(&project, settings.get()?.clone())?;
    but_workspace::stack_branches(stack_id, &ctx).map_err(Into::into)
}

/// Retrieve all changes in the workspace and associate them with commits in the Workspace of `project_id`.
/// NOTE: right now there is no way to keep track of unassociated hunks.
// TODO: This probably has to change a lot once it's clear how the UI is going to use it.
//       Right now this is only a port from the V2 UI, and that data structure was never used directly.
#[tauri::command(async)]
#[instrument(skip(projects), err(Debug))]
pub fn hunk_dependencies_for_workspace_changes(
    projects: State<'_, projects::Controller>,
    project_id: ProjectId,
) -> Result<HunkDependencies, Error> {
    let project = projects.get(project_id)?;
    let dependencies =
        hunk_dependencies_for_workspace_changes_by_worktree_dir(&project.path, &project.gb_dir())?;
    Ok(dependencies)
}

pub fn hunk_dependencies_for_workspace_changes_by_worktree_dir(
    worktree_dir: &Path,
    gitbutler_dir: &Path,
) -> anyhow::Result<HunkDependencies> {
    let repo = gix::open(worktree_dir).map_err(anyhow::Error::from)?;
    let worktree_changes = but_core::diff::worktree_changes(&repo)?;
    let stacks = but_workspace::stacks(gitbutler_dir)?;
    let common_merge_base = gitbutler_stack::VirtualBranchesHandle::new(gitbutler_dir)
        .get_default_target()?
        .sha;
    let input_stacks = but_hunk_dependency::workspace_stacks_to_input_stacks(
        &repo,
        &stacks,
        common_merge_base.to_gix(),
    )?;
    let ranges = but_hunk_dependency::WorkspaceRanges::try_from_stacks(input_stacks)?;
    HunkDependencies::try_from_workspace_ranges(&repo, ranges, worktree_changes.changes)
}

/// Calculate as hash for a `universal_diff`.
// TODO: see if this should be avoided entirely here as the current impl would allow for hash collisions.
pub fn hash_lines(universal_diff: impl AsRef<[u8]>) -> HunkHash {
    let diff = universal_diff.as_ref();
    assert!(
        diff.starts_with(b"@@"),
        "BUG: input mut be a universal diff"
    );
    let mut ctx = rustc_hash::FxHasher::default();
    diff.lines_with_terminator()
        .skip(1) // skip the first line which is the diff header.
        .for_each(|line| ctx.write(line));
    ctx.finish()
}

/// A way to represent all hunk dependencies that would make it possible to know what can be applied, and were.
///
/// Note that the [`errors`](Self::errors) field may contain information about specific failures, while other paths
/// may have succeeded computing.
#[derive(Debug, Clone, Serialize)]
pub struct HunkDependencies {
    /// A map from diffs to branch and commit dependencies.
    // TODO: could this be a specific type? Is the mapping truly required?
    //       Is this because `commit_dependent_diffs` use `HunkHash`?
    // TODO: the frontend actually has no way of associating the hunks it gets with this hash as it's made
    //       on the patch lines without any context lines, while it has context lines.
    //       Hash must then skip the context lines if there are any.
    pub diffs: Vec<(HunkHash, Vec<HunkLock>)>,
    /// A map from stack id to commit dependencies.
    /// Commit dependencies map commit id to commits it depends on.
    // TODO: have to use strings as keys for serialization (`gix::ObjectId`)
    #[serde(skip)]
    pub commit_dependencies: HashMap<StackId, HashMap<gix::ObjectId, HashSet<gix::ObjectId>>>,
    /// A map from stack id to inverse commit dependencies.
    /// Inverse commit dependencies map commit id to commits that depend on it.
    // TODO: have to use strings as keys for serialization (`gix::ObjectId`)
    #[serde(skip)]
    pub inverse_commit_dependencies:
        HashMap<StackId, HashMap<gix::ObjectId, HashSet<gix::ObjectId>>>,
    /// A map from stack id to dependent commit dependent diffs.
    /// Commit dependent diffs map commit id to diffs that depend on it.
    // TODO: have to use strings as keys for serialization (`gix::ObjectId`)
    #[serde(skip)]
    pub commit_dependent_diffs: HashMap<StackId, HashMap<gix::ObjectId, HashSet<HunkHash>>>,
    /// Errors that occurred during the calculation that should be presented in some way.
    // TODO: Does the UI really use whatever partial result that there may be? Should this be a real error?
    pub errors: Vec<but_hunk_dependency::CalculationError>,
}

impl HunkDependencies {
    /// Calculate all hunk dependencies using a preparepd [`but_hunk_dependency::WorkspaceRanges`].
    fn try_from_workspace_ranges(
        repo: &gix::Repository,
        ranges: but_hunk_dependency::WorkspaceRanges,
        worktree_changes: Vec<but_core::TreeChange>,
    ) -> anyhow::Result<HunkDependencies> {
        let mut diffs = Vec::<(HunkHash, Vec<HunkLock>)>::new();
        for change in worktree_changes {
            let unidiff = change.unified_diff(repo, 0 /* zero context lines */)?;
            let UnifiedDiff::Patch { hunks } = unidiff else {
                continue;
            };
            for hunk in hunks {
                if let Some(intersections) =
                    ranges.intersection(&change.path, hunk.old_start, hunk.old_lines)
                {
                    let locks: Vec<_> = intersections
                        .into_iter()
                        .map(|dependency| HunkLock {
                            commit_id: dependency.commit_id,
                            stack_id: dependency.stack_id,
                        })
                        .collect();
                    diffs.push((hash_lines(&hunk.diff), locks));
                }
            }
        }

        let mut commit_dependent_diffs =
            HashMap::<StackId, HashMap<gix::ObjectId, HashSet<HunkHash>>>::new();
        for (hash, locks) in &diffs {
            for lock in locks {
                commit_dependent_diffs
                    .entry(lock.stack_id)
                    .or_default()
                    .entry(lock.commit_id)
                    .or_default()
                    .insert(*hash);
            }
        }

        let (commit_dependencies, inverse_commit_dependencies) =
            ranges.commit_dependencies_and_inverse_commit_dependencies();
        let errors = ranges.errors;

        Ok(HunkDependencies {
            diffs,
            commit_dependencies,
            inverse_commit_dependencies,
            commit_dependent_diffs,
            errors,
        })
    }
}

/// A hash over the universal diff of a hunk.
// TODO: using the hash directly like we do can collide, would have to use actual Hunk to prevent this issue.
pub type HunkHash = u64;

/// A commit that owns this lock, along with the stack that owns it.
/// A hunk is locked when it depends on changes in commits that are in your workspace. A hunk can
/// be locked to more than one branch if it overlaps with more than one committed hunk.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HunkLock {
    /// The ID of the stack that contains [`commit_id`](Self::commit_id).
    pub stack_id: StackId,
    /// The commit the hunk applies to.
    #[serde(serialize_with = "gitbutler_serde::object_id::serialize")]
    pub commit_id: gix::ObjectId,
}
