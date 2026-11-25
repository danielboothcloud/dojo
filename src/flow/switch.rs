//! `dojo switch <bookmark>`: start working on a bookmark.
//!
//! Steps: fetch if the bookmark is unknown; `jj bookmark track` if the local
//! bookmark is missing or untracked; `jj new <bookmark>`. Edits in the old
//! working-copy change stay in that change. Never runs `git checkout`.

use anyhow::{Result, bail};
use serde::Serialize;

use super::fetch::{self, FetchReport};
use super::{Ctx, Runner, Step, StepRecord, short};
use crate::jujutsu::bookmarks::{self, BookmarkRow};

#[derive(Debug, Clone, Serialize)]
pub struct SwitchReport {
    pub bookmark: String,
    pub fetched: Option<FetchReport>,
    /// Remote that dojo started to track, if any.
    pub tracked_remote: Option<String>,
    /// The previous working-copy change, when it kept edits.
    pub kept_change: Option<KeptChange>,
    pub already_there: bool,
    pub steps: Vec<StepRecord>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeptChange {
    pub change_id: String,
    pub short_id: String,
    pub title: String,
    /// How to go back to it.
    pub back: String,
}

/// Choose the remote to track: the push remote if it has the bookmark, else
/// the only remote that has it, else the upstream (forge) remote.
pub fn track_remote(ctx: &Ctx, row: &BookmarkRow) -> Option<String> {
    if row.remote(&ctx.remotes.push).is_some() {
        return Some(ctx.remotes.push.clone());
    }
    match row.remotes.as_slice() {
        [] => None,
        [only] => Some(only.remote.clone()),
        _ => row
            .remote(&ctx.remotes.upstream)
            .map(|c| c.remote.clone())
            .or_else(|| row.remotes.first().map(|c| c.remote.clone())),
    }
}

/// Steps that make the local bookmark exist and track a remote.
pub fn track_step(ctx: &Ctx, row: &BookmarkRow) -> Option<(Step, String)> {
    let local_tracked = row.local.is_some() && row.is_tracked_anywhere();
    if local_tracked {
        return None;
    }
    let remote = track_remote(ctx, row)?;
    let cell = row.remote(&remote)?;
    if cell.tracked {
        return None;
    }
    let step = Step::jj(
        &[
            "bookmark",
            "track",
            &bookmarks::pattern_arg(&row.name),
            "--remote",
            &bookmarks::pattern_arg(&remote),
        ],
        format!(
            "make a local bookmark {} that follows {}@{remote}",
            row.name, row.name
        ),
    );
    Some((step, remote))
}

pub fn switch(ctx: &Ctx, runner: &mut Runner<'_>, name: &str) -> Result<SwitchReport> {
    let start = runner.records.len();
    let mut rows = ctx.bookmark_rows()?;
    let mut fetched = None;
    if !rows.iter().any(|r| r.name == name) {
        fetched = Some(fetch::fetch(ctx, runner)?);
        rows = ctx.bookmark_rows()?;
    }
    let Some(row) = rows.into_iter().find(|r| r.name == name) else {
        bail!("{}", not_found_message(ctx, name)?);
    };

    let at = ctx.change("@")?;
    let mut tracked_remote = None;
    if let Some((step, remote)) = track_step(ctx, &row) {
        runner.run_ok(step)?;
        tracked_remote = Some(remote);
    }

    // After tracking, a diverged local bookmark can be conflicted.
    let row = if tracked_remote.is_some() && !runner.dry_run {
        ctx.bookmark_rows()?
            .into_iter()
            .find(|r| r.name == name)
            .unwrap_or(row)
    } else {
        row
    };
    if row.conflicted() {
        bail!(
            "bookmark {name} is conflicted (local and remote moved apart).\n\
             Fix it with: dojo sync {name}"
        );
    }

    let target_commit = row
        .local
        .as_ref()
        .and_then(|l| l.commit_id.clone())
        .or_else(|| {
            tracked_remote
                .as_deref()
                .and_then(|r| row.remote(r))
                .and_then(|c| c.commit_id.clone())
        });
    let already_there = at.empty
        && !at.has_description()
        && target_commit
            .as_deref()
            .is_some_and(|c| at.parents.len() == 1 && ctx.count(&format!("{c} & @-")) == 1);

    let kept_change = (!at.empty || at.has_description()).then(|| KeptChange {
        change_id: at.change_id.clone(),
        short_id: at.short_id.clone(),
        title: at.title().to_string(),
        back: format!("jj edit {}", at.short_id),
    });

    if !already_there {
        runner.run_ok(Step::jj(
            &["new", &bookmarks::symbol(name)],
            format!("start a new change on top of {name}"),
        ))?;
    }

    Ok(SwitchReport {
        bookmark: name.to_string(),
        fetched,
        tracked_remote,
        kept_change: if already_there { None } else { kept_change },
        already_there,
        steps: runner.records[start..].to_vec(),
        dry_run: runner.dry_run,
    })
}

fn not_found_message(ctx: &Ctx, name: &str) -> Result<String> {
    let rows = ctx.bookmark_rows()?;
    let lower = name.to_ascii_lowercase();
    let similar: Vec<&str> = rows
        .iter()
        .map(|r| r.name.as_str())
        .filter(|n| {
            let n = n.to_ascii_lowercase();
            n.contains(&lower) || lower.contains(&n)
        })
        .take(5)
        .collect();
    let mut msg = format!(
        "bookmark '{name}' not found locally or on the remotes ({}).",
        ctx.remotes.fetch.join(", ")
    );
    if !similar.is_empty() {
        msg.push_str(&format!("\nDid you mean: {}?", similar.join(", ")));
    }
    msg.push_str(&format!(
        "\nTo start a new bookmark, run: dojo new-branch {name}"
    ));
    Ok(msg)
}

/// Text for the CLI.
pub fn render(report: &SwitchReport) -> String {
    let mut out = String::new();
    if report.already_there {
        out.push_str(&format!(
            "You are already on {} (an empty change on top of it).\n",
            report.bookmark
        ));
        return out;
    }
    let verb = if report.dry_run { "Would switch" } else { "Switched" };
    out.push_str(&format!(
        "{verb} to {}: your working copy is a new change on top of it.\n",
        report.bookmark
    ));
    if let Some(r) = &report.tracked_remote {
        out.push_str(&format!(
            "The local bookmark {} now tracks {}@{r}.\n",
            report.bookmark, report.bookmark
        ));
    }
    if let Some(k) = &report.kept_change {
        let title = if k.title.is_empty() {
            "(no description)".to_string()
        } else {
            format!("\"{}\"", k.title)
        };
        out.push_str(&format!(
            "Your previous edits stay in change {} {title}.\nGo back to it with: {}\n",
            short(&k.short_id),
            k.back
        ));
    }
    out
}
