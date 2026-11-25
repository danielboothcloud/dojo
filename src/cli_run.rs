//! CLI front end for the `flow` workflows: prints the jj commands before
//! they run, renders reports (text or `--json`), and handles offers
//! (prompt on a terminal, `--yes` to accept, else print the next command).

use anyhow::Result;
use serde::Serialize;
use std::io::{BufRead, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::flow::{
    self, Ctx, Offer, OfferAction, Reporter, Runner, Silent, Step, StepRecord, branches, fetch,
    identity, new_branch, pr, push, status, switch, sync,
};

/// Global CLI flags.
#[derive(Debug, Clone, Copy, Default)]
pub struct Opts {
    pub json: bool,
    pub dry_run: bool,
    pub yes: bool,
}

impl Opts {
    fn interactive(self) -> bool {
        !self.json && std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
    }
}

/// Exit code for "dojo stopped and needs you" (refused push, open choice).
pub const EXIT_NEEDS_ACTION: i32 = 2;

/// Prints each command before it runs, then jj's own messages, indented.
struct Printer;

impl Reporter for Printer {
    fn step(&mut self, step: &Step, dry_run: bool) {
        if dry_run {
            println!("{}", step.command);
        } else {
            eprintln!("$ {}", step.command);
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
            eprintln!("  {}", line.trim_end());
        }
    }
}

fn with_runner<T>(
    ctx: &Ctx,
    opts: Opts,
    f: impl FnOnce(&mut Runner<'_>) -> Result<T>,
) -> Result<T> {
    if opts.json {
        let mut silent = Silent;
        let mut runner = Runner::new(&ctx.root, opts.dry_run, &mut silent);
        f(&mut runner)
    } else {
        let mut printer = Printer;
        let mut runner = Runner::new(&ctx.root, opts.dry_run, &mut printer);
        f(&mut runner)
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

static IDENTITY_CHECKED: AtomicBool = AtomicBool::new(false);

/// Before write commands: if jj has no user name/email, say so once and
/// (on a terminal) offer to set them.
fn check_identity(ctx: &Ctx, opts: Opts) {
    if IDENTITY_CHECKED.swap(true, Ordering::SeqCst) || opts.dry_run {
        return;
    }
    let id = identity::Identity::read(&ctx.root);
    let Some(hint) = id.hint() else { return };
    eprintln!("{hint}");
    if !opts.interactive() {
        return;
    }
    let name = id
        .name
        .clone()
        .or_else(|| ask("Your name for commits (empty to skip): "));
    let email = id
        .email
        
        .or_else(|| ask("Your email for commits (empty to skip): "));
    let (Some(name), Some(email)) = (name, email) else {
        return;
    };
    let mut printer = Printer;
    let mut runner = Runner::new(&ctx.root, false, &mut printer);
    for args in identity::set_commands(&name, &email) {
        let _ = runner.run(Step::jj(&args, "set your identity for all repos"));
    }
}

fn ask(prompt: &str) -> Option<String> {
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).ok()?;
    let line = line.trim().to_string();
    (!line.is_empty()).then_some(line)
}

fn confirm(prompt: &str) -> bool {
    eprint!("{prompt} [Y/n] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    let a = line.trim().to_ascii_lowercase();
    a.is_empty() || a == "y" || a == "yes"
}

/// Offer a follow-up. Returns true when it ran.
fn handle_offer(opts: Opts, offer: &Offer) -> Result<bool> {
    if opts.json {
        return Ok(false);
    }
    // Opening a PR is public; never run it implicitly.
    let auto_ok = !matches!(offer.action, OfferAction::Pr { .. });
    let preview = if offer.commands.is_empty() {
        String::new()
    } else {
        format!("\n  {}", offer.commands.join("\n  "))
    };
    if opts.dry_run || !auto_ok || (!opts.yes && !opts.interactive()) {
        eprintln!("\nNext: {}   ({}){preview}", offer.dojo, offer.label);
        return Ok(false);
    }
    if !opts.yes {
        eprintln!("\n{}: {}{preview}", offer.label, offer.dojo);
        if !confirm("Run it now?") {
            return Ok(false);
        }
    }
    run_offer(opts, &offer.action)?;
    Ok(true)
}

fn run_offer(opts: Opts, action: &OfferAction) -> Result<()> {
    match action {
        OfferAction::Sync { bookmark } => sync(&opts, Some(bookmark.clone())),
        OfferAction::Push { bookmark } => push(
            &opts,
            push::PushOptions {
                bookmark: bookmark.clone(),
                ..push::PushOptions::default()
            },
        ),
        OfferAction::PushChange { rev } => push(
            &opts,
            push::PushOptions {
                change: Some(rev.clone()),
                ..push::PushOptions::default()
            },
        ),
        OfferAction::PushAs { rev, name } => {
            let name = name.clone().or_else(|| {
                opts.interactive()
                    .then(|| ask("New bookmark name (empty for a generated push-* name): "))
                    .flatten()
            });
            let o = match name {
                Some(n) => push::PushOptions {
                    as_name: Some(n),
                    as_rev: Some(rev.clone()),
                    ..push::PushOptions::default()
                },
                None => push::PushOptions {
                    change: Some(rev.clone()),
                    ..push::PushOptions::default()
                },
            };
            push(&opts, o)
        }
        OfferAction::Pr { bookmark } => pr(
            &opts,
            pr::PrOptions {
                bookmark: Some(bookmark.clone()),
                ..pr::PrOptions::default()
            },
        ),
    }
}

fn needs_action_exit() -> ! {
    std::process::exit(EXIT_NEEDS_ACTION)
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

pub fn fetch(opts: &Opts) -> Result<()> {
    let opts = *opts;
    let ctx = Ctx::load()?;
    let report = with_runner(&ctx, opts, |r| fetch::fetch(&ctx, r))?;
    if opts.json {
        return print_json(&report);
    }
    if !opts.dry_run {
        print!("{}", render_fetch(&report));
    }
    Ok(())
}

pub fn render_fetch(report: &fetch::FetchReport) -> String {
    use fetch::ChangeKind;
    if report.is_empty() {
        return "Nothing new on the remotes.\n".to_string();
    }
    let s = |c: &Option<String>| c.as_deref().map_or("-", flow::short).to_string();
    let mut out = String::new();
    for (remote, changes) in &report.remote_changes {
        if changes.is_empty() {
            continue;
        }
        out.push_str(&format!("{remote}:\n"));
        for c in changes {
            let line = match c.kind {
                ChangeKind::New => format!("  new      {}  at {}", c.name, s(&c.new_commit)),
                ChangeKind::Moved => format!(
                    "  moved    {}  {} -> {}",
                    c.name,
                    s(&c.old_commit),
                    s(&c.new_commit)
                ),
                ChangeKind::Deleted => format!("  deleted  {}", c.name),
                ChangeKind::Conflicted => format!("  conflict {}", c.name),
            };
            out.push_str(&line);
            out.push('\n');
        }
    }
    if !report.local_changes.is_empty() {
        out.push_str("local bookmarks:\n");
        for c in &report.local_changes {
            let line = match c.kind {
                ChangeKind::Conflicted => format!(
                    "  conflicted {}  (local and remote moved apart; run: dojo sync {})",
                    c.name, c.name
                ),
                ChangeKind::Moved => format!("  updated    {}  -> {}", c.name, s(&c.new_commit)),
                ChangeKind::New => format!("  new        {}", c.name),
                ChangeKind::Deleted => format!("  deleted    {}", c.name),
            };
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

pub fn branches(opts: &Opts) -> Result<()> {
    let ctx = Ctx::load()?;
    let report = branches::branches(&ctx)?;
    if opts.json {
        return print_json(&report);
    }
    print!("{}", branches::render_table(&report));
    Ok(())
}

pub fn switch(opts: &Opts, bookmark: &str) -> Result<()> {
    let opts = *opts;
    let ctx = Ctx::load()?;
    check_identity(&ctx, opts);
    let report = with_runner(&ctx, opts, |r| switch::switch(&ctx, r, bookmark))?;
    if opts.json {
        return print_json(&report);
    }
    if let Some(f) = &report.fetched
        && !opts.dry_run
    {
        print!("{}", render_fetch(f));
    }
    print!("{}", switch::render(&report));
    Ok(())
}

pub fn push(opts: &Opts, push_opts: push::PushOptions) -> Result<()> {
    let opts = *opts;
    let ctx = Ctx::load()?;
    check_identity(&ctx, opts);
    let mut push_opts = push_opts;
    // jj refuses undescribed commits. Without a terminal, describe them from
    // their files; on a terminal, ask once (Enter accepts the generated one).
    push_opts.auto_describe |= !opts.interactive();
    let mut report = with_runner(&ctx, opts, |r| push::push(&ctx, r, &push_opts));
    if let Err(e) = &report
        && e.to_string().contains(push::NO_DESCRIPTION)
        && push_opts.message.is_none()
        && !opts.dry_run
    {
        eprintln!("{e}");
        match ask("Description for the change (Enter: \"Update <files>\"): ") {
            Some(m) => push_opts.message = Some(m),
            None => push_opts.auto_describe = true,
        }
        report = with_runner(&ctx, opts, |r| push::push(&ctx, r, &push_opts));
    }
    let report = report?;
    if opts.json {
        print_json(&report)?;
    } else {
        print!("{}", push::render(&report));
    }
    let ok = matches!(
        report.outcome,
        push::PushOutcome::Pushed | push::PushOutcome::UpToDate
    );
    let ran = match &report.offer {
        Some(offer) => handle_offer(opts, offer)?,
        None => false,
    };
    if !ok && !ran {
        needs_action_exit();
    }
    Ok(())
}

pub fn sync(opts: &Opts, bookmark: Option<String>) -> Result<()> {
    let opts = *opts;
    let ctx = Ctx::load()?;
    check_identity(&ctx, opts);
    let bookmark = match bookmark {
        Some(b) => b,
        None => push::current_bookmark(&ctx)?.ok_or_else(|| {
            anyhow::anyhow!("no bookmark on @ or @-; name one: dojo sync <bookmark>")
        })?,
    };
    let report = with_runner(&ctx, opts, |r| sync::sync(&ctx, r, &bookmark))?;
    if opts.json {
        print_json(&report)?;
    } else {
        if !opts.dry_run && !report.fetched.is_empty() {
            print!("{}", render_fetch(&report.fetched));
        }
        print!("{}", sync::render(&report));
    }
    if let Some(offer) = &report.offer {
        handle_offer(opts, offer)?;
    }
    if matches!(report.outcome, sync::SyncOutcome::Conflicts) {
        needs_action_exit();
    }
    Ok(())
}

pub fn new_branch(opts: &Opts, nb: new_branch::NewBranchOptions) -> Result<()> {
    let opts = *opts;
    let ctx = Ctx::load()?;
    check_identity(&ctx, opts);
    let report = with_runner(&ctx, opts, |r| new_branch::new_branch(&ctx, r, &nb))?;
    if opts.json {
        return print_json(&report);
    }
    println!("{}", report.message);
    Ok(())
}

pub fn status(opts: &Opts) -> Result<()> {
    let ctx = Ctx::load()?;
    let report = status::status(&ctx)?;
    if opts.json {
        return print_json(&report);
    }
    print!("{}", status::render(&report));
    Ok(())
}

pub fn pr(opts: &Opts, pr_opts: pr::PrOptions) -> Result<()> {
    let opts = *opts;
    let ctx = Ctx::load()?;
    check_identity(&ctx, opts);
    let report = with_runner(&ctx, opts, |r| pr::pr(&ctx, r, &pr_opts))?;
    if opts.json {
        return print_json(&report);
    }
    if !report.output.trim().is_empty() {
        println!("{}", report.output.trim_end());
    }
    Ok(())
}
