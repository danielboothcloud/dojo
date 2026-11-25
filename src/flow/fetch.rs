//! `dojo fetch`: fetch every remote with explicit `--remote` flags and
//! report what changed, grouped by remote.

use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;

use super::{Ctx, Runner, Step, StepRecord};
use crate::jujutsu::bookmarks::{self, RefState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    New,
    Moved,
    Deleted,
    /// Local bookmark only: it became conflicted.
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RefChange {
    pub name: String,
    pub kind: ChangeKind,
    pub old_commit: Option<String>,
    pub new_commit: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FetchReport {
    pub remotes: Vec<String>,
    /// Remote name -> changed remote bookmarks.
    pub remote_changes: BTreeMap<String, Vec<RefChange>>,
    /// Local bookmarks that the fetch moved or made conflicted.
    pub local_changes: Vec<RefChange>,
    pub steps: Vec<StepRecord>,
    pub dry_run: bool,
}

impl FetchReport {
    pub fn is_empty(&self) -> bool {
        self.local_changes.is_empty() && self.remote_changes.values().all(Vec::is_empty)
    }
}

/// The fetch step for this repo.
pub fn step(ctx: &Ctx) -> Step {
    Step::jj(
        &ctx.fetch_remote_args(),
        "download new commits and bookmarks from every remote",
    )
}

/// Fetch and diff bookmark state. Also used by switch/sync/pr.
pub fn fetch(ctx: &Ctx, runner: &mut Runner<'_>) -> Result<FetchReport> {
    let before = bookmarks::list_refs(&ctx.root)?;
    let start = runner.records.len();
    runner.run_ok(step(ctx))?;
    let after = if runner.dry_run {
        before.clone()
    } else {
        bookmarks::list_refs(&ctx.root)?
    };
    let (remote_changes, local_changes) = diff(&before, &after, &ctx.remotes.fetch);
    Ok(FetchReport {
        remotes: ctx.remotes.fetch.clone(),
        remote_changes,
        local_changes,
        steps: runner.records[start..].to_vec(),
        dry_run: runner.dry_run,
    })
}

type Key = (Option<String>, String);

fn index(refs: &[RefState]) -> BTreeMap<Key, &RefState> {
    refs.iter()
        .filter(|r| r.present)
        .map(|r| ((r.remote.clone(), r.name.clone()), r))
        .collect()
}

/// Compare two bookmark snapshots.
pub fn diff(
    before: &[RefState],
    after: &[RefState],
    remotes: &[String],
) -> (BTreeMap<String, Vec<RefChange>>, Vec<RefChange>) {
    let b = index(before);
    let a = index(after);
    let mut by_remote: BTreeMap<String, Vec<RefChange>> =
        remotes.iter().map(|r| (r.clone(), Vec::new())).collect();
    let mut local = Vec::new();

    let mut keys: Vec<&Key> = b.keys().chain(a.keys()).collect();
    keys.sort();
    keys.dedup();
    for key in keys {
        let old = b.get(key);
        let new = a.get(key);
        let change = match (old, new) {
            (None, Some(n)) => Some((ChangeKind::New, None, n.commit_id.clone())),
            (Some(o), None) => Some((ChangeKind::Deleted, o.commit_id.clone(), None)),
            (Some(o), Some(n)) if !o.conflict && n.conflict => {
                Some((ChangeKind::Conflicted, o.commit_id.clone(), None))
            }
            (Some(o), Some(n)) if o.commit_id != n.commit_id || o.conflict != n.conflict => {
                Some((ChangeKind::Moved, o.commit_id.clone(), n.commit_id.clone()))
            }
            _ => None,
        };
        let Some((kind, old_commit, new_commit)) = change else {
            continue;
        };
        let rc = RefChange {
            name: key.1.clone(),
            kind,
            old_commit,
            new_commit,
        };
        match &key.0 {
            Some(remote) => by_remote.entry(remote.clone()).or_default().push(rc),
            None => local.push(rc),
        }
    }
    (by_remote, local)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(name: &str, remote: Option<&str>, commit: &str, conflict: bool) -> RefState {
        RefState {
            name: name.into(),
            remote: remote.map(str::to_string),
            present: true,
            tracked: false,
            conflict,
            commit_id: (!commit.is_empty()).then(|| commit.to_string()),
            change_id: None,
            remote_ahead: None,
            remote_behind: None,
            added_change_ids: Vec::new(),
        }
    }

    #[test]
    fn diff_groups_new_moved_deleted() {
        let before = vec![
            r("a", Some("origin"), "1", false),
            r("gone", Some("sundae"), "2", false),
            r("a", None, "1", false),
        ];
        let after = vec![
            r("a", Some("origin"), "3", false),
            r("new", Some("sundae"), "4", false),
            r("a", None, "", true),
        ];
        let remotes = vec!["origin".to_string(), "sundae".to_string()];
        let (by_remote, local) = diff(&before, &after, &remotes);
        assert_eq!(by_remote["origin"][0].kind, ChangeKind::Moved);
        assert_eq!(by_remote["origin"][0].new_commit.as_deref(), Some("3"));
        let sundae = &by_remote["sundae"];
        assert_eq!(sundae.len(), 2);
        assert!(
            sundae
                .iter()
                .any(|c| c.kind == ChangeKind::Deleted && c.name == "gone")
        );
        assert!(
            sundae
                .iter()
                .any(|c| c.kind == ChangeKind::New && c.name == "new")
        );
        assert_eq!(local[0].kind, ChangeKind::Conflicted);
    }
}
