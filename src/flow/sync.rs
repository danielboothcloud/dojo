//! `dojo sync <bookmark>`: bring the forge's new commits into your work.
//!
//! Steps: fetch; `jj rebase -b <b> -d <b>@origin` (or `@sundae` without an
//! origin); fix the bookmark if the fetch left it conflicted; report
//! conflicts; offer `dojo push <b>`.

use anyhow::{Result, bail};
use serde::Serialize;

use super::fetch::{self, FetchReport};
use super::{Ctx, Offer, OfferAction, Runner, Step, StepRecord, q};
use crate::jujutsu::bookmarks;
use crate::jujutsu::changes::{self, ChangeInfo};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SyncOutcome {
    /// The bookmark already contains the forge's commits.
    UpToDate,
    /// The bookmark moved forward to the forge's commit (no own commits).
    FastForwarded,
    Rebased,
    /// Rebased, but some changes now have conflicts.
    Conflicts,
    /// Nothing on the forge to sync with.
    NoUpstream,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    pub bookmark: String,
    pub upstream: String,
    pub outcome: SyncOutcome,
    pub fetched: FetchReport,
    pub conflicts: Vec<ChangeInfo>,
    pub message: String,
    pub offer: Option<Offer>,
    pub steps: Vec<StepRecord>,
    pub dry_run: bool,
}

pub fn sync(ctx: &Ctx, runner: &mut Runner<'_>, name: &str) -> Result<SyncReport> {
    let start = runner.records.len();
    let rows = ctx.bookmark_rows()?;
    let Some(row) = rows.iter().find(|r| r.name == name) else {
        bail!("bookmark '{name}' not found. See `dojo branches`.");
    };
    let Some(local) = &row.local else {
        bail!("{name} exists only on the remote. Start working on it with: dojo switch {name}");
    };
    // Our side of the bookmark, as a change id (stable across rebases).
    let ours = if local.conflict {
        let remote_changes: Vec<&str> = row
            .remotes
            .iter()
            .filter_map(|c| c.change_id.as_deref())
            .collect();
        local
            .conflict_change_ids
            .iter()
            .find(|c| !remote_changes.contains(&c.as_str()))
            .cloned()
    } else {
        local.change_id.clone()
    };
    let Some(ours) = ours else {
        bail!(
            "cannot tell which side of the conflicted bookmark {name} is yours.\n\
             Pick one with: jj bookmark set {name} -r <change id> --allow-backwards"
        );
    };

    let fetched = fetch::fetch(ctx, runner)?;

    let rows = if runner.dry_run {
        rows
    } else {
        ctx.bookmark_rows()?
    };
    let row = rows.iter().find(|r| r.name == name);
    let upstream = [ctx.remotes.upstream.as_str(), ctx.remotes.push.as_str()]
        .into_iter()
        .find(|r| row.and_then(|row| row.remote(r)).is_some())
        .map(str::to_string);
    let mut report = SyncReport {
        bookmark: name.to_string(),
        upstream: upstream.clone().unwrap_or_default(),
        outcome: SyncOutcome::NoUpstream,
        fetched,
        conflicts: Vec::new(),
        message: String::new(),
        offer: None,
        steps: Vec::new(),
        dry_run: runner.dry_run,
    };
    let Some(upstream) = upstream else {
        report.message = format!(
            "{name} is not on {} yet, so there is nothing to sync. Push it with: dojo push {name}",
            ctx.remotes.upstream
        );
        report.steps = runner.records[start..].to_vec();
        return Ok(report);
    };
    let up_sym = bookmarks::remote_symbol(name, &upstream);
    let up_commit = row
        .and_then(|r| r.remote(&upstream))
        .and_then(|c| c.commit_id.clone())
        .unwrap_or_default();
    let local_conflicted = row.is_some_and(|r| r.conflicted());

    let contains_upstream = ctx.count(&format!("{up_commit} & ::{ours}")) > 0;
    let behind_only = ctx.count(&format!("{ours} & ::{up_commit}")) > 0;

    // Your work on top of the bookmark (the working copy and its ancestors
    // after the bookmark) must move too, so rebase from @ when @ is on it.
    let work_on_top = ctx.count(&format!(
        "({ours}::@) ~ {ours} ~ (empty() & description(exact:\"\"))"
    )) > 0;
    let at_on_bookmark = ctx.count(&format!("{ours} & ::@")) > 0;
    let wc_contains_upstream = ctx.count(&format!("{up_commit} & ::@")) > 0;

    if contains_upstream && !local_conflicted && (!work_on_top || wc_contains_upstream) {
        report.outcome = SyncOutcome::UpToDate;
        report.message = format!("{name} already has every commit of {up_sym}.");
    } else if behind_only && !local_conflicted {
        runner.run_ok(Step::jj(
            &[
                "bookmark",
                "set",
                &bookmarks::pattern_arg(name),
                "--revision",
                &up_sym,
            ],
            format!("move {name} forward to {up_sym} (it has no commits of your own)"),
        ))?;
        if work_on_top && at_on_bookmark {
            runner.run_ok(Step::jj(
                &["rebase", "--branch", "@", "--destination", &up_sym],
                format!("put your working-copy change on top of {up_sym}"),
            ))?;
            report.outcome = SyncOutcome::Rebased;
            report.message =
                format!("{name} now points at {up_sym}, and your change is on top of it.");
        } else {
            report.outcome = SyncOutcome::FastForwarded;
            report.message = format!("{name} now points at {up_sym}.");
        }
    } else {
        let branch = if work_on_top && at_on_bookmark {
            "@".to_string()
        } else if local_conflicted {
            ours.clone()
        } else {
            bookmarks::symbol(name)
        };
        runner.run_ok(Step::jj(
            &["rebase", "--branch", &branch, "--destination", &up_sym],
            format!("put your commits on {name} on top of the forge's {up_sym}"),
        ))?;
        let still_conflicted = !runner.dry_run
            && ctx
                .bookmark_rows()?
                .iter()
                .find(|r| r.name == name)
                .is_some_and(|r| r.conflicted());
        if local_conflicted && (still_conflicted || runner.dry_run) {
            runner.run_ok(Step::jj(
                &[
                    "bookmark",
                    "set",
                    &bookmarks::pattern_arg(name),
                    "--revision",
                    &ours,
                    "--allow-backwards",
                ],
                format!("point {name} at your rebased change"),
            ))?;
        }
        report.outcome = SyncOutcome::Rebased;
        report.message = format!("Rebased your work on {name} onto {up_sym}.");
    }

    if !runner.dry_run {
        let conflict_revset = format!("({up_commit}::({ours} | @)) & conflicts()");
        report.conflicts = changes::changes(&ctx.root, &conflict_revset).unwrap_or_default();
    }
    if report.conflicts.is_empty() {
        if !matches!(report.outcome, SyncOutcome::UpToDate) || needs_push(ctx, name) {
            report.offer = Some(Offer {
                label: format!("Push {name}"),
                dojo: format!("dojo push {}", q(name)),
                commands: vec![
                    Step::jj(
                        &[
                            "git",
                            "push",
                            "--remote",
                            &bookmarks::pattern_arg(&ctx.remotes.push),
                            "--bookmark",
                            &bookmarks::pattern_arg(name),
                        ],
                        "",
                    )
                    .command,
                ],
                action: OfferAction::Push {
                    bookmark: Some(name.to_string()),
                },
            });
        }
    } else {
        report.outcome = SyncOutcome::Conflicts;
        let ids: Vec<String> = report
            .conflicts
            .iter()
            .map(|c| format!("  {} {}", c.short_id, c.title()))
            .collect();
        report.message.push_str(&format!(
            "\nThese changes now have conflicts:\n{}\nResolve them: `jj new <id>`, edit the files (remove the conflict markers), then `jj squash`. `jj resolve` opens a merge tool. Then run: dojo push {name}",
            ids.join("\n")
        ));
    }
    report.steps = runner.records[start..].to_vec();
    Ok(report)
}

fn needs_push(ctx: &Ctx, name: &str) -> bool {
    ctx.bookmark_rows()
        .ok()
        .and_then(|rows| rows.into_iter().find(|r| r.name == name))
        .is_some_and(|r| {
            let local = r.local.as_ref().and_then(|l| l.commit_id.clone());
            let pushed = r
                .remote(&ctx.remotes.push)
                .and_then(|c| c.commit_id.clone());
            local.is_some() && local != pushed
        })
}

pub fn render(report: &SyncReport) -> String {
    let mut out = report.message.clone();
    out.push('\n');
    out
}
