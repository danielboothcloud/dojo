//! Actions of the home view: what a key does, the command preview, and the
//! run. Workflow actions (fetch, switch, push, sync, pr) use the shared
//! `flow` core. The preview is a dry run of the same code, so it shows the
//! exact jj commands that the real run starts with.

use anyhow::Result;

use crate::cli_run::render_fetch;
use crate::flow::{
    self, Ctx, Offer, OfferAction, Reporter, Runner, Step, StepRecord, fetch, pr, push, switch,
    sync,
};

/// One thing the user can run from the home view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HomeAction {
    Fetch,
    Switch(String),
    /// `None`: the bookmark of the current work.
    Push(Option<String>),
    /// Push a change with a generated `push-*` bookmark.
    PushChange(String),
    /// Push a change under a new bookmark name.
    PushAs {
        rev: String,
        name: String,
    },
    Sync(String),
    Pr(String),
    /// One plain jj command. `interactive`: it needs the terminal (an
    /// editor), so the TUI hands the terminal over while it runs.
    Jj {
        label: String,
        args: Vec<String>,
        why: String,
        interactive: bool,
    },
}

impl HomeAction {
    pub(crate) fn jj(label: &str, args: &[&str], why: &str, interactive: bool) -> Self {
        Self::Jj {
            label: label.to_string(),
            args: args.iter().map(|a| (*a).to_string()).collect(),
            why: why.to_string(),
            interactive,
        }
    }

    /// Short title for the preview and output boxes.
    pub(crate) fn title(&self) -> String {
        match self {
            Self::Fetch => "Fetch from all remotes".to_string(),
            Self::Switch(b) => format!("Switch to {b}"),
            Self::Push(Some(b)) => format!("Push {b}"),
            Self::Push(None) => "Push the current work".to_string(),
            Self::PushChange(rev) => format!("Push change {rev} with a new push-* bookmark"),
            Self::PushAs { rev, name } => format!("Push change {rev} as {name}"),
            Self::Sync(b) => format!("Sync {b} with the forge"),
            Self::Pr(b) => format!("Open a pull request for {b}"),
            Self::Jj { label, .. } => label.clone(),
        }
    }

    /// The same action as a dojo command line, when there is one.
    pub(crate) fn dojo_command(&self) -> Option<String> {
        let q = flow::q;
        Some(match self {
            Self::Fetch => "dojo fetch".to_string(),
            Self::Switch(b) => format!("dojo switch {}", q(b)),
            Self::Push(Some(b)) => format!("dojo push {}", q(b)),
            Self::Push(None) => "dojo push".to_string(),
            Self::PushChange(rev) => format!("dojo push --change {}", q(rev)),
            Self::Sync(b) => format!("dojo sync {}", q(b)),
            Self::Pr(b) => format!("dojo pr {}", q(b)),
            // `dojo push --as` names the current change only.
            Self::PushAs { .. } | Self::Jj { .. } => return None,
        })
    }

    /// True when the command needs the terminal.
    pub(crate) fn interactive(&self) -> bool {
        matches!(
            self,
            Self::Jj {
                interactive: true,
                ..
            }
        )
    }

    /// Workflows that fetch first can run more commands than the preview
    /// shows (the preview cannot know what the fetch brings).
    fn fetches_first(&self) -> bool {
        matches!(self, Self::Sync(_) | Self::Switch(_))
    }
}

/// The action that runs an offer.
pub(crate) fn offer_action(offer: &Offer) -> HomeAction {
    match &offer.action {
        OfferAction::Sync { bookmark } => HomeAction::Sync(bookmark.clone()),
        OfferAction::Push { bookmark } => HomeAction::Push(bookmark.clone()),
        OfferAction::PushChange { rev } => HomeAction::PushChange(rev.clone()),
        OfferAction::PushAs { rev, name } => match name {
            Some(n) => HomeAction::PushAs {
                rev: rev.clone(),
                name: n.clone(),
            },
            None => HomeAction::PushChange(rev.clone()),
        },
        OfferAction::Pr { bookmark } => HomeAction::Pr(bookmark.clone()),
    }
}

/// The command preview: shown before anything runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Preview {
    pub action: HomeAction,
    pub title: String,
    /// `(command, why)` pairs.
    pub commands: Vec<(String, String)>,
    pub dojo: Option<String>,
    /// Extra information (for example "the list can change after the
    /// fetch"), or why the action cannot run.
    pub note: Option<String>,
    /// False when the dry run failed; the preview then only explains.
    pub runnable: bool,
}

/// What happened when an action ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub title: String,
    /// `$ command` lines, jj's output (indented), then dojo's message.
    pub lines: Vec<String>,
    pub ok: bool,
    /// A follow-up that one key runs.
    pub offer: Option<Offer>,
}

/// Collects the steps and their output as text lines.
#[derive(Default)]
struct Collect {
    lines: Vec<String>,
}

impl Reporter for Collect {
    fn step(&mut self, step: &Step, dry_run: bool) {
        if !dry_run {
            self.lines.push(format!("$ {}", step.command));
        }
    }

    fn step_done(&mut self, record: &StepRecord) {
        if !record.ran {
            return;
        }
        for line in record
            .stderr
            .lines()
            .chain(record.stdout.lines())
            .filter(|l| !l.trim().is_empty())
        {
            self.lines.push(format!("  {}", line.trim_end()));
        }
    }
}

/// Result of one workflow run: the dojo message and an offer.
struct FlowResult {
    message: String,
    ok: bool,
    offer: Option<Offer>,
}

fn run_flow(ctx: &Ctx, runner: &mut Runner<'_>, action: &HomeAction) -> Result<FlowResult> {
    let done = |message: String, ok: bool, offer: Option<Offer>| FlowResult { message, ok, offer };
    Ok(match action {
        HomeAction::Fetch => {
            let r = fetch::fetch(ctx, runner)?;
            done(render_fetch(&r), true, None)
        }
        HomeAction::Switch(b) => {
            let r = switch::switch(ctx, runner, b)?;
            let mut msg = String::new();
            if let Some(f) = r.fetched.as_ref().filter(|f| !f.is_empty()) {
                msg.push_str(&render_fetch(f));
            }
            msg.push_str(&switch::render(&r));
            done(msg, true, None)
        }
        HomeAction::Push(_) | HomeAction::PushChange(_) | HomeAction::PushAs { .. } => {
            let opts = push_options(action);
            let r = push::push(ctx, runner, &opts)?;
            let ok = matches!(
                r.outcome,
                push::PushOutcome::Pushed | push::PushOutcome::UpToDate
            );
            done(push::render(&r), ok, r.offer)
        }
        HomeAction::Sync(b) => {
            let r = sync::sync(ctx, runner, b)?;
            let mut msg = String::new();
            if !r.fetched.is_empty() {
                msg.push_str(&render_fetch(&r.fetched));
            }
            msg.push_str(&sync::render(&r));
            let ok = !matches!(r.outcome, sync::SyncOutcome::Conflicts);
            done(msg, ok, r.offer)
        }
        HomeAction::Pr(b) => {
            let r = pr::pr(
                ctx,
                runner,
                &pr::PrOptions {
                    bookmark: Some(b.clone()),
                    ..pr::PrOptions::default()
                },
            )?;
            let ok = r.steps.iter().all(|s| s.success);
            done(r.output.trim_end().to_string(), ok, None)
        }
        HomeAction::Jj { args, why, .. } => {
            let record = runner.run(Step::jj(args, why.clone()))?;
            let msg = if record.success {
                String::new()
            } else {
                "The command failed (see the output above).".to_string()
            };
            done(msg, record.success, None)
        }
    })
}

fn push_options(action: &HomeAction) -> push::PushOptions {
    // The preview shows the `jj describe` command, so an undescribed change
    // gets the generated "Update <files>" description only after the user
    // saw it and confirmed.
    let base = push::PushOptions {
        auto_describe: true,
        ..push::PushOptions::default()
    };
    match action {
        HomeAction::Push(b) => push::PushOptions {
            bookmark: b.clone(),
            ..base
        },
        HomeAction::PushChange(rev) => push::PushOptions {
            change: Some(rev.clone()),
            ..base
        },
        HomeAction::PushAs { rev, name } => push::PushOptions {
            as_name: Some(name.clone()),
            as_rev: Some(rev.clone()),
            ..base
        },
        _ => base,
    }
}

/// Dry-run the action to show its commands.
pub(crate) fn preview(ctx: &Ctx, action: &HomeAction) -> Preview {
    let mut collect = Collect::default();
    let mut runner = Runner::new(&ctx.root, true, &mut collect);
    let result = match action {
        // A dry run of a plain jj command is the command itself.
        HomeAction::Jj { args, why, .. } => {
            runner.run(Step::jj(args, why.clone())).map(|_| FlowResult {
                message: String::new(),
                ok: true,
                offer: None,
            })
        }
        _ => run_flow(ctx, &mut runner, action),
    };
    let commands = runner
        .records
        .iter()
        .map(|r| (r.step.command.clone(), r.step.why.clone()))
        .collect();
    let mut p = Preview {
        action: action.clone(),
        title: action.title(),
        commands,
        dojo: action.dojo_command(),
        note: None,
        runnable: true,
    };
    match result {
        Err(e) => {
            p.runnable = false;
            p.note = Some(format!("dojo cannot run this now:\n{e:#}"));
        }
        Ok(r) => {
            if action.fetches_first() {
                p.note = Some("dojo can run more commands after the fetch.".to_string());
            }
            if p.commands.is_empty() {
                p.runnable = false;
                let msg = r.message.trim();
                p.note = Some(if msg.is_empty() {
                    "Nothing to do.".to_string()
                } else {
                    msg.to_string()
                });
            }
            if action.interactive() {
                p.note = Some("jj opens your editor. The TUI comes back when it closes.".into());
            }
        }
    }
    p
}

/// Run the action (not for interactive ones; see [`run_interactive_args`]).
pub(crate) fn execute(ctx: &Ctx, action: &HomeAction) -> Outcome {
    let mut collect = Collect::default();
    let mut runner = Runner::new(&ctx.root, false, &mut collect);
    let result = run_flow(ctx, &mut runner, action);
    drop(runner);
    let mut lines = collect.lines;
    let (ok, offer) = match result {
        Ok(r) => {
            lines.extend(r.message.lines().map(str::to_string));
            (r.ok, r.offer)
        }
        Err(e) => {
            lines.extend(format!("{e:#}").lines().map(str::to_string));
            (false, None)
        }
    };
    Outcome {
        title: action.title(),
        lines,
        ok,
        offer,
    }
}

/// The jj arguments of an interactive action.
pub(crate) fn run_interactive_args(action: &HomeAction) -> Option<&[String]> {
    match action {
        HomeAction::Jj {
            args,
            interactive: true,
            ..
        } => Some(args),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_map_to_actions() {
        let offer = |action| Offer {
            label: String::new(),
            dojo: String::new(),
            commands: Vec::new(),
            action,
        };
        assert_eq!(
            offer_action(&offer(OfferAction::Sync {
                bookmark: "b".into()
            })),
            HomeAction::Sync("b".into())
        );
        assert_eq!(
            offer_action(&offer(OfferAction::PushAs {
                rev: "abc".into(),
                name: None
            })),
            HomeAction::PushChange("abc".into())
        );
        assert_eq!(
            offer_action(&offer(OfferAction::Pr {
                bookmark: "b".into()
            })),
            HomeAction::Pr("b".into())
        );
    }

    #[test]
    fn dojo_command_quotes_names() {
        assert_eq!(
            HomeAction::Sync("fix/goals".into()).dojo_command().unwrap(),
            "dojo sync fix/goals"
        );
        assert!(
            HomeAction::jj("Undo", &["undo"], "", false)
                .dojo_command()
                .is_none()
        );
    }
}
