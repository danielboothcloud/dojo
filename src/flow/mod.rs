//! dojo's bookmark/remote workflows: fetch, branches, switch, push, sync,
//! new-branch, status and pr.
//!
//! Every workflow is a core function that the CLI and the TUI share. A
//! workflow computes jj commands as [`Step`]s (argument vectors, never shell
//! strings) and runs them through a [`Runner`], which shows each command
//! before it runs (the teaching aid) and records the result. In dry-run mode
//! the runner records the commands and runs nothing.

pub mod branches;
pub mod fetch;
pub mod identity;
pub mod new_branch;
pub mod pending;
pub mod pr;
pub mod push;
pub mod refusal;
pub mod status;
pub mod switch;
pub mod sync;

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::config::{self, Config};
use crate::jujutsu::{self, StepOutput, bookmarks, changes, remotes};

/// Repo context shared by all workflows.
pub struct Ctx {
    /// Root of the current workspace (where jj commands run).
    pub root: PathBuf,
    pub config: Config,
    pub remotes: remotes::RemoteRoles,
}

impl Ctx {
    /// Load the context for the workspace that contains the cwd. Snapshots
    /// the working copy first (like any jj command does), so pending file
    /// edits are part of `@` before dojo plans anything.
    pub fn load() -> Result<Self> {
        let root = jujutsu::current_workspace_root()?;
        Self::load_at(root)
    }

    pub fn load_at(root: PathBuf) -> Result<Self> {
        // Snapshot failure (for example a stale working copy) is reported by
        // the first real jj command; do not fail the load for it.
        let _ = jujutsu::snapshot_ws(&root);
        let config_root = jujutsu::workspace_root().unwrap_or_else(|_| root.clone());
        let config = config::load_config(&config_root)
            .or_else(|_| config::load_config(&root))
            .context("load dojo config")?;
        let remotes = remotes::detect(
            &root,
            config.push_remote.as_deref(),
            config.upstream_remote.as_deref(),
        )?;
        Ok(Self {
            root,
            config,
            remotes,
        })
    }

    pub fn is_protected(&self, bookmark: &str) -> bool {
        self.config.protected.iter().any(|p| p == bookmark)
    }

    pub fn bookmark_rows(&self) -> Result<Vec<bookmarks::BookmarkRow>> {
        bookmarks::bookmark_rows(&self.root, &self.config.protected)
    }

    pub fn change(&self, rev: &str) -> Result<changes::ChangeInfo> {
        changes::change(&self.root, rev)
    }

    pub fn count(&self, revset: &str) -> usize {
        changes::count(&self.root, revset).unwrap_or(0)
    }

    /// `jj git push` / `jj git fetch` remote arguments.
    pub fn fetch_remote_args(&self) -> Vec<String> {
        let mut args = vec!["git".to_string(), "fetch".to_string()];
        for r in &self.remotes.fetch {
            args.push("--remote".to_string());
            args.push(bookmarks::pattern_arg(r));
        }
        args
    }
}

/// Program a step runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Program {
    Jj,
    Gh,
    Glab,
}

impl Program {
    pub fn name(self) -> &'static str {
        match self {
            Self::Jj => "jj",
            Self::Gh => "gh",
            Self::Glab => "glab",
        }
    }
}

/// One command that dojo runs for the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub program: Program,
    /// Arguments after the program name.
    pub args: Vec<String>,
    /// The command line as the user would type it.
    pub command: String,
    /// Why dojo runs it (one short sentence).
    pub why: String,
}

impl Step {
    pub fn jj<S: AsRef<str>>(args: &[S], why: impl Into<String>) -> Self {
        Self::new(Program::Jj, args, why)
    }

    pub fn new<S: AsRef<str>>(program: Program, args: &[S], why: impl Into<String>) -> Self {
        let args: Vec<String> = args.iter().map(|a| a.as_ref().to_string()).collect();
        let command = match program {
            Program::Jj => jujutsu::display_command(&args),
            other => {
                let mut words = vec![other.name().to_string()];
                words.extend(args.iter().map(|a| jujutsu::quote_word(a)));
                words.join(" ")
            }
        };
        Self {
            program,
            args,
            command,
            why: why.into(),
        }
    }
}

/// A step and what happened when it ran.
#[derive(Debug, Clone, Serialize)]
pub struct StepRecord {
    #[serde(flatten)]
    pub step: Step,
    /// False in dry-run mode.
    pub ran: bool,
    pub success: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub stdout: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub stderr: String,
}

/// Receives progress while a workflow runs.
pub trait Reporter {
    /// Called before a step runs (or would run, in dry-run mode).
    fn step(&mut self, step: &Step, dry_run: bool);
    /// Called after a step ran.
    fn step_done(&mut self, _record: &StepRecord) {}
}

/// Reporter that ignores everything (JSON mode, tests).
pub struct Silent;

impl Reporter for Silent {
    fn step(&mut self, _step: &Step, _dry_run: bool) {}
}

/// Runs steps, shows them first, records the results.
pub struct Runner<'a> {
    pub root: &'a Path,
    pub dry_run: bool,
    reporter: &'a mut dyn Reporter,
    pub records: Vec<StepRecord>,
}

impl<'a> Runner<'a> {
    pub fn new(root: &'a Path, dry_run: bool, reporter: &'a mut dyn Reporter) -> Self {
        Self {
            root,
            dry_run,
            reporter,
            records: Vec::new(),
        }
    }

    /// Run one step. A failed command is returned as `Ok` with
    /// `success == false`; only spawn failures are `Err`.
    pub fn run(&mut self, step: Step) -> Result<StepRecord> {
        self.reporter.step(&step, self.dry_run);
        let record = if self.dry_run {
            StepRecord {
                step,
                ran: false,
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            }
        } else {
            let out = match step.program {
                Program::Jj => jujutsu::run_jj_step(self.root, &step.args)?,
                other => run_external(self.root, other.name(), &step.args)?,
            };
            StepRecord {
                step,
                ran: true,
                success: out.success,
                stdout: out.stdout,
                stderr: out.stderr,
            }
        };
        self.reporter.step_done(&record);
        self.records.push(record.clone());
        Ok(record)
    }

    /// Run a step that must succeed; a failure becomes an error that quotes
    /// the command and jj's message.
    pub fn run_ok(&mut self, step: Step) -> Result<StepRecord> {
        let record = self.run(step)?;
        if !record.success {
            anyhow::bail!(
                "`{}` failed:\n{}",
                record.step.command,
                record.stderr.trim_end()
            );
        }
        Ok(record)
    }
}

/// Run a non-jj tool (gh, glab) with an argument vector (no shell).
fn run_external(root: &Path, program: &str, args: &[String]) -> Result<StepOutput> {
    let output = std::process::Command::new(program)
        .args(args)
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .output()
        .with_context(|| format!("failed to run {program}"))?;
    Ok(StepOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Find an executable on `PATH` (no `which` needed).
pub fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// A follow-up that dojo offers after a workflow (one key in the TUI, one
/// command in the CLI).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Offer {
    /// Short label, for example "Rebase onto the forge and push again".
    pub label: String,
    /// The dojo command line that does it.
    pub dojo: String,
    /// The jj (or gh/glab) commands it will run, for the preview.
    pub commands: Vec<String>,
    pub action: OfferAction,
}

/// What an [`Offer`] runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum OfferAction {
    Sync { bookmark: String },
    Push { bookmark: Option<String> },
    /// `jj git push -c <rev>` (creates a `push-*` bookmark).
    PushChange { rev: String },
    /// Push the same change under a new bookmark name.
    PushAs { rev: String, name: Option<String> },
    Pr { bookmark: String },
}

/// Short form of a full id for messages.
pub fn short(id: &str) -> &str {
    &id[..id.len().min(8)]
}

/// Quote a revset or name for display in a suggested command.
pub fn q(word: &str) -> String {
    jujutsu::quote_word(word)
}
