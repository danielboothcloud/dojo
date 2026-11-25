//! `dojo push [<bookmark>]`: push a bookmark to the push remote (Sundae).
//!
//! - Default bookmark: the one on `@`, else on `@-`, else a pending name from
//!   `dojo new-branch`. With none: offer `jj git push --remote <r> -c <rev>`.
//! - Moves the bookmark forward onto the change to push (`@`, or `@-` when
//!   `@` is empty).
//! - Always pushes with `--remote <push-remote>`.
//! - Explains Sundae refusals and offers the fix as one follow-up.

use anyhow::{Result, bail};
use serde::Serialize;

use super::refusal::{self, Refusal};
use super::{Ctx, Offer, OfferAction, Runner, Step, StepRecord, pending, q, short};
use crate::jujutsu::bookmarks::{self, BookmarkRow};
use crate::jujutsu::changes::ChangeInfo;

#[derive(Debug, Clone, Default)]
pub struct PushOptions {
    pub bookmark: Option<String>,
    /// Push this revision with a generated `push-*` bookmark (`-c`).
    pub change: Option<String>,
    /// Push under this new bookmark name instead.
    pub as_name: Option<String>,
    /// With `as_name`: the revision to name (default: the bookmark's change
    /// or the change to push).
    pub as_rev: Option<String>,
    /// Description for the change to push, when it has none.
    pub message: Option<String>,
    /// Without `message`: describe undescribed changes with a message built
    /// from their files ("Update a.txt") instead of stopping.
    pub auto_describe: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PushOutcome {
    Pushed,
    /// Nothing to do (no changes, or already on the remote).
    UpToDate,
    /// dojo needs a decision; see `offer`.
    NeedsChoice,
    Refused,
}

#[derive(Debug, Clone, Serialize)]
pub struct PushReport {
    pub outcome: PushOutcome,
    pub remote: String,
    pub bookmark: Option<String>,
    /// Change id that was (or would be) pushed.
    pub change_id: Option<String>,
    pub refusal: Option<Refusal>,
    /// Human explanation (refusals, notes).
    pub message: String,
    pub offer: Option<Offer>,
    pub steps: Vec<StepRecord>,
    pub dry_run: bool,
}

/// The change to push: `@` if it has file changes, else `@-`.
pub fn push_target(ctx: &Ctx) -> Result<(ChangeInfo, &'static str)> {
    let at = ctx.change("@")?;
    if !at.empty {
        return Ok((at, "@"));
    }
    if at.parents.len() != 1 {
        bail!("@ is an empty merge change; describe what to push with `dojo push <bookmark>`");
    }
    Ok((ctx.change("@-")?, "@-"))
}

/// The bookmark of the current work: on `@`, else on `@-`.
pub fn current_bookmark(ctx: &Ctx) -> Result<Option<String>> {
    for rev in ["@", "@-"] {
        let Ok(c) = ctx.change(rev) else { continue };
        if let Some(b) = pick_bookmark(ctx, &c.local_bookmarks) {
            return Ok(Some(b));
        }
    }
    Ok(None)
}

/// Prefer a non-protected bookmark.
fn pick_bookmark(ctx: &Ctx, names: &[String]) -> Option<String> {
    names
        .iter()
        .find(|b| !ctx.is_protected(b))
        .or_else(|| names.first())
        .cloned()
}

fn report(ctx: &Ctx, runner: &Runner<'_>, start: usize, outcome: PushOutcome) -> PushReport {
    PushReport {
        outcome,
        remote: ctx.remotes.push.clone(),
        bookmark: None,
        change_id: None,
        refusal: None,
        message: String::new(),
        offer: None,
        steps: runner.records[start..].to_vec(),
        dry_run: runner.dry_run,
    }
}

pub fn push_change_offer(ctx: &Ctx, rev: &str) -> Offer {
    Offer {
        label: "Push this change with a new push-* bookmark".to_string(),
        dojo: format!("dojo push --change {}", q(rev)),
        commands: vec![change_step(ctx, rev).command],
        action: OfferAction::PushChange {
            rev: rev.to_string(),
        },
    }
}

fn change_step(ctx: &Ctx, rev: &str) -> Step {
    Step::jj(
        &[
            "git",
            "push",
            "--remote",
            &bookmarks::pattern_arg(&ctx.remotes.push),
            "--change",
            rev,
        ],
        format!(
            "create a push-<id> bookmark for {rev} and push it to {}",
            ctx.remotes.push
        ),
    )
}

fn bookmark_push_step(ctx: &Ctx, name: &str) -> Step {
    Step::jj(
        &[
            "git",
            "push",
            "--remote",
            &bookmarks::pattern_arg(&ctx.remotes.push),
            "--bookmark",
            &bookmarks::pattern_arg(name),
        ],
        format!("send {name} to {}", ctx.remotes.push),
    )
}

/// Undescribed commits that this push would send (jj refuses them).
fn undescribed(ctx: &Ctx, head_rev: &str) -> Result<Vec<ChangeInfo>> {
    let revset = format!(
        "(::({head_rev}) ~ ::remote_bookmarks(remote=exact:{})) & description(exact:\"\") & mutable()",
        quote_revset_str(&ctx.remotes.push)
    );
    crate::jujutsu::changes::changes(&ctx.root, &revset)
}

fn quote_revset_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn push(ctx: &Ctx, runner: &mut Runner<'_>, opts: &PushOptions) -> Result<PushReport> {
    let start = runner.records.len();
    let mut report = push_inner(ctx, runner, opts)?;
    // Sundae forwarded the push to the forge; refresh our view of origin so
    // `dojo branches` / `dojo status` show it (best effort).
    if matches!(report.outcome, PushOutcome::Pushed)
        && ctx.remotes.sundae
        && let Some(forge) = ctx.remotes.forge()
    {
        let name = forge.name.clone();
        let _ = runner.run(Step::jj(
            &["git", "fetch", "--remote", &bookmarks::pattern_arg(&name)],
            format!("see the bookmark that Sundae forwarded to {name}"),
        ));
        report.steps = runner.records[start..].to_vec();
    }
    Ok(report)
}

fn push_inner(ctx: &Ctx, runner: &mut Runner<'_>, opts: &PushOptions) -> Result<PushReport> {
    let start = runner.records.len();
    let remote = ctx.remotes.push.clone();

    // --change: jj creates the bookmark name itself.
    if let Some(rev) = &opts.change {
        describe_if_needed(ctx, runner, rev, opts)?;
        let record = runner.run(change_step(ctx, rev))?;
        return Ok(finish(ctx, runner, start, None, rev, record));
    }

    let rows = ctx.bookmark_rows()?;
    let (target, target_rev) = push_target(ctx)?;

    // Which bookmark?
    let mut bookmark = opts.bookmark.clone();
    let mut from_pending = false;
    if bookmark.is_none() {
        bookmark = current_bookmark(ctx)?;
    }
    if bookmark.is_none() {
        for p in pending::load(&ctx.root) {
            if ctx.count(&format!("{} & ::{}", p.change_id, target.commit_id)) > 0 {
                bookmark = Some(p.name);
                from_pending = true;
                break;
            }
        }
    }

    // --as <name>: the same change under a new name.
    if let Some(new_name) = &opts.as_name {
        let rev = match (&opts.as_rev, &bookmark) {
            (Some(r), _) => r.clone(),
            (None, Some(b)) if opts.bookmark.is_some() => bookmarks::symbol(b),
            _ => target_rev.to_string(),
        };
        if rows.iter().any(|r| r.name == *new_name && r.local.is_some()) {
            bail!("bookmark {new_name} already exists; choose another name");
        }
        if ctx.is_protected(new_name) {
            bail!("{new_name} is protected on {remote}; choose another name");
        }
        describe_if_needed(ctx, runner, &rev, opts)?;
        runner.run_ok(Step::jj(
            &["bookmark", "create", new_name, "--revision", &rev],
            format!("name the change {new_name}"),
        ))?;
        let record = runner.run(bookmark_push_step(ctx, new_name))?;
        return Ok(finish(ctx, runner, start, Some(new_name), &rev, record));
    }

    let Some(name) = bookmark else {
        if target.immutable {
            let mut r = report(ctx, runner, start, PushOutcome::UpToDate);
            r.message = "Nothing to push: your working copy has no changes.".to_string();
            return Ok(r);
        }
        let mut r = report(ctx, runner, start, PushOutcome::NeedsChoice);
        r.change_id = Some(target.change_id.clone());
        r.message = format!(
            "Change {} has no bookmark. Push it with a generated name, or name it: dojo push --as <name>",
            target.short_id
        );
        r.offer = Some(push_change_offer(ctx, target_rev));
        return Ok(r);
    };

    if ctx.is_protected(&name) {
        let rev = rows
            .iter()
            .find(|r| r.name == name)
            .and_then(|r| r.local.as_ref())
            .and_then(|l| l.change_id.clone())
            .map_or_else(|| target_rev.to_string(), |c| short(&c).to_string());
        let mut r = report(ctx, runner, start, PushOutcome::Refused);
        r.bookmark = Some(name.clone());
        r.refusal = Some(Refusal::Protected {
            bookmark: Some(name.clone()),
        });
        r.message = protected_message(&name, &remote);
        r.offer = Some(push_as_offer(ctx, &rev));
        return Ok(r);
    }

    let row = rows.iter().find(|r| r.name == name);
    let local = row.and_then(|r| r.local.as_ref());
    if local.is_some_and(|l| l.conflict) {
        bail!(
            "bookmark {name} is conflicted. Fix it first with: dojo sync {name}"
        );
    }

    // Where the bookmark will point, and whether dojo moves it forward.
    let local_commit = local.and_then(|l| l.commit_id.clone());
    let moves_forward = local_commit.as_deref().is_some_and(|c| {
        c != target.commit_id
            && !target.immutable
            && ctx.count(&format!("{c} & ::{}", target.commit_id)) > 0
    });
    let head_rev = if local_commit.is_none() || moves_forward {
        target_rev.to_string()
    } else {
        bookmarks::symbol(&name)
    };
    // jj refuses undescribed commits: check before changing anything.
    if local_commit.is_some() || !target.immutable {
        describe_if_needed(ctx, runner, &head_rev, opts)?;
    }

    // Make the local bookmark point at the change to push.
    match local_commit {
        None => {
            if row.is_some_and(|r| !r.remotes.is_empty()) {
                bail!(
                    "{name} exists only on the remote. Start working on it with: dojo switch {name}"
                );
            }
            if target.immutable {
                bail!("nothing to push: {target_rev} is already on the remote");
            }
            runner.run_ok(Step::jj(
                &[
                    "bookmark",
                    "create",
                    &bookmarks::pattern_arg(&name),
                    "--revision",
                    target_rev,
                ],
                format!("name your change {name}"),
            ))?;
            if from_pending && !runner.dry_run {
                let _ = pending::remove(&ctx.root, &name);
            }
        }
        Some(_) if moves_forward => {
            runner.run_ok(Step::jj(
                &[
                    "bookmark",
                    "set",
                    &bookmarks::pattern_arg(&name),
                    "--revision",
                    target_rev,
                ],
                format!("move {name} forward to your latest change ({target_rev})"),
            ))?;
        }
        Some(_) => {}
    }

    // Track an existing, untracked remote bookmark so jj may update it.
    if let Some(cell) = row.and_then(|r| r.remote(&remote))
        && !cell.tracked
        && local.is_some()
    {
        runner.run_ok(Step::jj(
            &[
                "bookmark",
                "track",
                &bookmarks::pattern_arg(&name),
                "--remote",
                &bookmarks::pattern_arg(&remote),
            ],
            format!("let jj update {name}@{remote}"),
        ))?;
    }

    let sym = bookmarks::symbol(&name);
    if !runner.dry_run && up_to_date(ctx, &name, row) {
        let mut r = report(ctx, runner, start, PushOutcome::UpToDate);
        r.bookmark = Some(name.clone());
        r.message = format!("{name} is already up to date on {remote}.");
        return Ok(r);
    }
    let record = runner.run(bookmark_push_step(ctx, &name))?;
    Ok(finish(ctx, runner, start, Some(&name), &sym, record))
}

fn up_to_date(ctx: &Ctx, name: &str, row_before: Option<&BookmarkRow>) -> bool {
    // Re-read: the steps above may have moved the bookmark.
    let rows = ctx.bookmark_rows().unwrap_or_default();
    let row = rows.iter().find(|r| r.name == name).or(row_before);
    row.is_some_and(|r| {
        let local = r.local.as_ref().and_then(|l| l.commit_id.as_deref());
        let remote = r
            .remote(&ctx.remotes.push)
            .and_then(|c| c.commit_id.as_deref());
        local.is_some() && local == remote
    })
}

/// jj refuses commits without a description. With `-m`, use that message;
/// with `auto_describe`, a message built from the changed files; otherwise
/// fail with the exact fix.
fn describe_if_needed(
    ctx: &Ctx,
    runner: &mut Runner<'_>,
    rev: &str,
    opts: &PushOptions,
) -> Result<()> {
    let missing = undescribed(ctx, rev)?;
    if missing.is_empty() {
        return Ok(());
    }
    if opts.message.is_some() || opts.auto_describe {
        for c in &missing {
            let msg = opts
                .message
                .clone()
                .unwrap_or_else(|| default_message(c));
            runner.run_ok(Step::jj(
                &["describe", &c.short_id, "--message", &msg],
                "give the change a description (jj refuses to push without one)",
            ))?;
        }
        return Ok(());
    }
    let fixes: Vec<String> = missing
        .iter()
        .map(|c| format!("  jj describe -r {} -m \"what this change does\"", c.short_id))
        .collect();
    bail!(
        "jj does not push changes {NO_DESCRIPTION}. Describe them first:\n{}\nOr run: dojo push -m \"what this change does\"",
        fixes.join("\n")
    )
}

/// Marker text of the "needs a description" error (the CLI matches it).
pub const NO_DESCRIPTION: &str = "without a description";

/// "Update a.txt, b.txt" (at most three names) from a change's files.
pub fn default_message(c: &ChangeInfo) -> String {
    let names: Vec<&str> = c
        .files
        .iter()
        .map(|f| f.path.rsplit('/').next().unwrap_or(&f.path))
        .collect();
    match names.len() {
        0 => "Update".to_string(),
        1..=3 => format!("Update {}", names.join(", ")),
        n => format!("Update {} and {} more files", names[..3].join(", "), n - 3),
    }
}

fn protected_message(name: &str, remote: &str) -> String {
    format!(
        "{name} is protected: {remote} does not accept pushes to it. Push the same change as a new bookmark and open a PR from it instead."
    )
}

fn push_as_offer(ctx: &Ctx, rev: &str) -> Offer {
    Offer {
        label: "Push the same change as a new bookmark".to_string(),
        dojo: format!("dojo push --change {}", q(rev)),
        commands: vec![change_step(ctx, rev).command],
        action: OfferAction::PushAs {
            rev: rev.to_string(),
            name: None,
        },
    }
}

fn finish(
    ctx: &Ctx,
    runner: &Runner<'_>,
    start: usize,
    name: Option<&str>,
    rev: &str,
    record: StepRecord,
) -> PushReport {
    let remote = ctx.remotes.push.clone();
    let mut r = report(ctx, runner, start, PushOutcome::Pushed);
    r.bookmark = name.map(str::to_string);
    if record.success {
        r.message = match name {
            Some(n) if runner.dry_run => format!("Would push {n} to {remote}."),
            Some(n) => format!("Pushed {n} to {remote}."),
            None if runner.dry_run => format!("Would push {rev} to {remote}."),
            None => format!("Pushed {rev} to {remote}."),
        };
        if ctx.remotes.sundae && ctx.remotes.forge().is_some() {
            r.message
                .push_str(" Sundae forwards it to the forge (origin) in the same push.");
        }
        if let (Some(n), Some(_)) = (name, ctx.remotes.forge()) {
            r.offer = Some(Offer {
                label: "Open a pull request".to_string(),
                dojo: format!("dojo pr {}", q(n)),
                commands: Vec::new(),
                action: OfferAction::Pr {
                    bookmark: n.to_string(),
                },
            });
        }
        return r;
    }
    r.outcome = PushOutcome::Refused;
    let refusal = refusal::parse(&record.stderr);
    let bookmark = name.map(str::to_string);
    match &refusal {
        Refusal::ForgeAhead { bookmark: b } => {
            let b = b.clone().or_else(|| bookmark.clone()).unwrap_or_default();
            r.message = format!(
                "The forge has new commits on {b} that you do not have (someone pushed to it from outside). Fetch them, rebase your work on top, and push again."
            );
            r.offer = Some(Offer {
                label: format!("Sync {b} with the forge, then push"),
                dojo: format!("dojo sync {}", q(&b)),
                commands: sync_preview(ctx, &b),
                action: OfferAction::Sync { bookmark: b },
            });
        }
        Refusal::Protected { bookmark: b } => {
            let b = b.clone().or_else(|| bookmark.clone()).unwrap_or_default();
            r.message = protected_message(&b, &remote);
            r.offer = Some(push_as_offer(ctx, rev));
        }
        Refusal::PrivatePaths { paths } => {
            let mut msg = String::from(
                "Sundae refused the push: these commits add private files. Private files (AGENTS.md, todo.txt, .pi/, _bmad/, ...) live in refs/sundae/private and must not be committed:\n",
            );
            for p in paths {
                msg.push_str(&format!("  {}  in commit {}\n", p.path, short(&p.commit)));
            }
            msg.push_str(
                "Remove them from those commits (for example `jj file untrack <path>` after adding the path to .git/info/exclude, or edit the commit with `jj edit <commit>` and delete the file), then push again.",
            );
            r.message = msg;
        }
        Refusal::Other { message } => {
            r.message = format!("The push failed:\n{message}");
        }
    }
    r.refusal = Some(refusal);
    r
}

/// The jj commands `dojo sync <b>` runs (for offer previews).
pub fn sync_preview(ctx: &Ctx, bookmark: &str) -> Vec<String> {
    let upstream = &ctx.remotes.upstream;
    vec![
        super::fetch::step(ctx).command,
        Step::jj(
            &[
                "rebase",
                "--branch",
                &bookmarks::symbol(bookmark),
                "--destination",
                &bookmarks::remote_symbol(bookmark, upstream),
            ],
            "",
        )
        .command,
    ]
}

/// Text for the CLI.
pub fn render(report: &PushReport) -> String {
    let mut out = report.message.clone();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}
