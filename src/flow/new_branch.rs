//! `dojo new-branch <name> [--from <rev>] [--now]`: start new work.
//!
//! Runs `jj new <rev>`. With `--now`, also `jj bookmark create <name> -r @`.
//! Without `--now`, dojo remembers the name and `dojo push` creates the
//! bookmark once the change has content (a bookmark on an empty change has
//! nothing to push).

use anyhow::{Result, bail};
use serde::Serialize;

use super::{Ctx, Runner, Step, StepRecord, pending};
use crate::jujutsu::bookmarks;

#[derive(Debug, Clone, Default)]
pub struct NewBranchOptions {
    pub name: String,
    pub from: Option<String>,
    pub now: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NewBranchReport {
    pub bookmark: String,
    pub from: String,
    pub created_now: bool,
    pub change_id: Option<String>,
    pub message: String,
    pub steps: Vec<StepRecord>,
    pub dry_run: bool,
}

/// Default base for new work: jj's `trunk()` (the forge's main branch).
pub const DEFAULT_FROM: &str = "trunk()";

pub fn new_branch(
    ctx: &Ctx,
    runner: &mut Runner<'_>,
    opts: &NewBranchOptions,
) -> Result<NewBranchReport> {
    let start = runner.records.len();
    let name = opts.name.trim();
    if name.is_empty() || name.chars().any(char::is_whitespace) {
        bail!("invalid bookmark name '{}': use no spaces", opts.name);
    }
    if ctx.is_protected(name) {
        bail!("{name} is protected on {}; choose another name", ctx.remotes.push);
    }
    let rows = ctx.bookmark_rows()?;
    if let Some(row) = rows.iter().find(|r| r.name == name) {
        if row.local.is_some() {
            bail!("bookmark {name} already exists. Work on it with: dojo switch {name}");
        }
        bail!("{name} already exists on the remote. Work on it with: dojo switch {name}");
    }
    let from = opts.from.clone().unwrap_or_else(|| DEFAULT_FROM.to_string());

    let mut args: Vec<String> = vec!["new".into(), from.clone()];
    if let Some(m) = &opts.message {
        args.push("--message".into());
        args.push(m.clone());
    }
    runner.run_ok(Step::jj(&args, format!("start a new change on top of {from}")))?;

    let change_id = if runner.dry_run {
        None
    } else {
        Some(ctx.change("@")?.change_id)
    };
    let message = if opts.now {
        runner.run_ok(Step::jj(
            &[
                "bookmark",
                "create",
                &bookmarks::pattern_arg(name),
                "--revision",
                "@",
            ],
            format!("name the new change {name}"),
        ))?;
        format!("Created bookmark {name} on a new change on top of {from}. Edit files, then run: dojo push")
    } else {
        if let Some(id) = &change_id {
            pending::add(&ctx.root, name, id)?;
        }
        format!(
            "Started a new change on top of {from}. Edit files, then run `dojo push`: it creates bookmark {name} (jj bookmark create {name} -r @) and pushes it."
        )
    };
    Ok(NewBranchReport {
        bookmark: name.to_string(),
        from,
        created_now: opts.now,
        change_id,
        message,
        steps: runner.records[start..].to_vec(),
        dry_run: runner.dry_run,
    })
}
