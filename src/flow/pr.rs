//! `dojo pr [<bookmark>]`: open the pull request (GitHub, `gh`) or merge
//! request (GitLab, `glab`) for a bookmark, after making sure it is pushed.
//!
//! jj leaves git's HEAD detached, so `gh` gets `--head <bookmark>` and
//! `glab` gets `--source-branch <bookmark>`.

use anyhow::{Result, bail};
use serde::Serialize;

use super::push::{self, PushOptions, PushOutcome, PushReport};
use super::{Ctx, Program, Runner, Step, StepRecord, find_on_path};
use crate::jujutsu::changes;
use crate::jujutsu::remotes::{self, Forge};

#[derive(Debug, Clone, Default)]
pub struct PrOptions {
    pub bookmark: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub base: Option<String>,
    pub draft: bool,
    /// Show CI status instead of creating the PR.
    pub checks: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrReport {
    pub bookmark: String,
    pub forge: Forge,
    pub repo: String,
    pub push: Option<PushReport>,
    pub output: String,
    pub steps: Vec<StepRecord>,
    pub dry_run: bool,
}

/// `(forge, tool, --repo value)` for the origin remote.
pub fn forge_target(ctx: &Ctx) -> Result<(Forge, Program, String)> {
    let Some(origin) = ctx.remotes.forge() else {
        bail!("no 'origin' remote, so dojo does not know the forge for a pull request");
    };
    let Some((forge, host, path)) = remotes::forge_of_url(&origin.fetch_url) else {
        bail!(
            "origin ({}) is not on github.com or a GitLab host; open the pull request in the web UI",
            origin.fetch_url
        );
    };
    let (program, default_host) = match forge {
        Forge::GitHub => (Program::Gh, "github.com"),
        Forge::GitLab => (Program::Glab, "gitlab.com"),
    };
    let repo = if host.eq_ignore_ascii_case(default_host) {
        path
    } else {
        format!("{host}/{path}")
    };
    Ok((forge, program, repo))
}

/// The create (or checks) command for a bookmark.
pub fn pr_step(
    program: Program,
    repo: &str,
    bookmark: &str,
    title: &str,
    body: &str,
    opts: &PrOptions,
) -> Step {
    let mut args: Vec<String> = Vec::new();
    match (program, opts.checks) {
        (Program::Gh, false) => {
            args.extend(
                ["pr", "create", "--repo", repo, "--head", bookmark, "--title", title, "--body", body]
                    .map(String::from),
            );
            if let Some(b) = &opts.base {
                args.extend(["--base".to_string(), b.clone()]);
            }
            if opts.draft {
                args.push("--draft".into());
            }
        }
        (Program::Gh, true) => {
            args.extend(["pr", "checks", bookmark, "--repo", repo].map(String::from));
        }
        (_, false) => {
            args.extend(
                [
                    "mr",
                    "create",
                    "--repo",
                    repo,
                    "--source-branch",
                    bookmark,
                    "--title",
                    title,
                    "--description",
                    body,
                    "--yes",
                ]
                .map(String::from),
            );
            if let Some(b) = &opts.base {
                args.extend(["--target-branch".to_string(), b.clone()]);
            }
            if opts.draft {
                args.push("--draft".into());
            }
        }
        (_, true) => {
            args.extend(["ci", "status", "--branch", bookmark, "--repo", repo].map(String::from));
        }
    }
    let why = if opts.checks {
        format!("show CI status for {bookmark}")
    } else {
        format!("open a pull request from {bookmark}")
    };
    Step::new(program, &args, why)
}

/// Title and body from the commits on the bookmark that the forge's main
/// line does not have.
fn title_body(ctx: &Ctx, bookmark: &str) -> (String, String) {
    let sym = crate::jujutsu::bookmarks::symbol(bookmark);
    let commits = changes::changes(&ctx.root, &format!("trunk()..{sym}")).unwrap_or_default();
    match commits.as_slice() {
        [] => (bookmark.to_string(), String::new()),
        [one] => {
            let mut lines = one.description.lines();
            let title = lines.next().unwrap_or(bookmark).to_string();
            let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
            (title, body)
        }
        many => {
            // jj log order is newest first; list oldest first.
            let body = many
                .iter()
                .rev()
                .map(|c| format!("- {}", c.title()))
                .collect::<Vec<_>>()
                .join("\n");
            (bookmark.to_string(), body)
        }
    }
}

pub fn pr(ctx: &Ctx, runner: &mut Runner<'_>, opts: &PrOptions) -> Result<PrReport> {
    let start = runner.records.len();
    let (forge, program, repo) = forge_target(ctx)?;
    let bookmark = match &opts.bookmark {
        Some(b) => b.clone(),
        None => push::current_bookmark(ctx)?
            .ok_or_else(|| anyhow::anyhow!("no bookmark on @ or @-. Push your work first: dojo push"))?,
    };
    if find_on_path(program.name()).is_none() && !runner.dry_run {
        bail!(
            "{} is not installed. Install it, or open the pull request for {bookmark} in the web UI",
            program.name()
        );
    }

    // Make sure the bookmark is pushed (not needed for checks).
    let mut push_report = None;
    if !opts.checks {
        let p = push::push(
            ctx,
            runner,
            &PushOptions {
                bookmark: Some(bookmark.clone()),
                ..PushOptions::default()
            },
        )?;
        let ok = matches!(p.outcome, PushOutcome::Pushed | PushOutcome::UpToDate);
        let msg = p.message.clone();
        push_report = Some(p);
        if !ok && !runner.dry_run {
            bail!("cannot open a pull request: the push did not succeed.\n{msg}");
        }
    }

    let (title, body) = title_body(ctx, &bookmark);
    let title = opts.title.clone().unwrap_or(title);
    let body = opts.body.clone().unwrap_or(body);
    let record = runner.run(pr_step(program, &repo, &bookmark, &title, &body, opts))?;
    if !record.success {
        bail!(
            "`{}` failed:\n{}",
            record.step.command,
            record.stderr.trim_end()
        );
    }
    Ok(PrReport {
        bookmark,
        forge,
        repo,
        push: push_report,
        output: format!("{}{}", record.stdout, record.stderr),
        steps: runner.records[start..].to_vec(),
        dry_run: runner.dry_run,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gh_create_uses_head() {
        let s = pr_step(
            Program::Gh,
            "o/r",
            "fix/goals",
            "Fix goals",
            "",
            &PrOptions::default(),
        );
        assert_eq!(
            s.command,
            "gh pr create --repo o/r --head fix/goals --title 'Fix goals' --body ''"
        );
    }

    #[test]
    fn glab_create_uses_source_branch() {
        let s = pr_step(
            Program::Glab,
            "g/p",
            "fix/goals",
            "T",
            "B",
            &PrOptions {
                base: Some("main".into()),
                ..PrOptions::default()
            },
        );
        assert!(s.command.starts_with("glab mr create --repo g/p --source-branch fix/goals"));
        assert!(s.command.ends_with("--yes --target-branch main"));
    }

    #[test]
    fn checks_commands() {
        let opts = PrOptions {
            checks: true,
            ..PrOptions::default()
        };
        assert_eq!(
            pr_step(Program::Gh, "o/r", "b", "", "", &opts).command,
            "gh pr checks b --repo o/r"
        );
        assert_eq!(
            pr_step(Program::Glab, "g/p", "b", "", "", &opts).command,
            "glab ci status --branch b --repo g/p"
        );
    }
}
