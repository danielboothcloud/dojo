//! Home view of `dojo` (no subcommand): bookmarks and log side by side (one
//! at a time on narrow terminals), the change detail, and keys that run jj
//! commands. Every command is shown in a preview before it runs.
//!
//! - `state.rs`: data, selections, key handling (no I/O).
//! - `view.rs`: drawing (pure; snapshot tests).
//! - `actions.rs`: previews (dry runs of the shared `flow` code) and runs.
//! - `log_pane.rs`: the `jj log` graph with a selected change.
//!
//! This file has the I/O: loading data, the event loop, and the terminal
//! handoff for commands that open an editor.

mod actions;
mod log_pane;
mod state;
mod view;

use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::config;
use crate::flow::{self, Ctx, identity::Identity};
use crate::jujutsu::{self, changes};
use actions::{HomeAction, Outcome};
use state::{Detail, Home, HomeData, Intent, Modal, Screen};

use super::op_log_pane::OpLogPane;

/// Why the event loop ended.
enum Exit {
    Quit,
    /// Open the workspace TUI (`dojo ws tui`).
    Workspaces,
}

pub fn run() -> Result<()> {
    let root = jujutsu::current_workspace_root()?;
    let mut ctx = match Ctx::load_at(root) {
        Ok(ctx) => ctx,
        Err(e) => {
            // For example a repo without remotes: the workspace view still
            // works there.
            eprintln!("dojo: {e:#}\nOpening the workspace view (dojo ws tui) instead.");
            return super::run();
        }
    };
    let mut data = load_data(&ctx)?;
    data.identity_hint = Identity::read(&ctx.root).hint();
    let mut home = Home::new(data);

    let mut session = Session::enter()?;
    let result = event_loop(&mut session, &mut home, &mut ctx);
    session.leave();
    match result? {
        Exit::Quit => Ok(()),
        Exit::Workspaces => super::run(),
    }
}

/// Load everything the home view shows. Called at start and after each
/// action (the `Ctx` load snapshots the working copy first).
fn load_data(ctx: &Ctx) -> Result<HomeData> {
    let rows = ctx.bookmark_rows()?;
    let (log_ansi, line_heads) =
        jujutsu::log_graph_with_heads(&ctx.root, ctx.config.log_template.as_deref())
            .unwrap_or_else(|e| (format!("jj log failed: {e:#}"), Vec::new()));
    let repo_root = jujutsu::workspace_root().unwrap_or_else(|_| ctx.root.clone());
    Ok(HomeData {
        repo_name: config::resolve_repo_name(&ctx.config, &repo_root),
        remotes: ctx.remotes.display_order(),
        push_remote: ctx.remotes.push.clone(),
        rows,
        log_ansi,
        line_heads,
        working_copy: ctx.change("@").ok().map(|c| c.change_id),
        current_bookmark: flow::push::current_bookmark(ctx).ok().flatten(),
        identity_hint: None,
    })
}

fn load_detail(ctx: &Ctx, rev: &str) -> Result<Detail> {
    let info = ctx.change(rev)?;
    let diff = changes::diff_colored(&ctx.root, &info.commit_id).unwrap_or_default();
    Ok(Detail::new(info, &diff))
}

/// Reload the context (snapshot) and the data; keep the screen.
fn refresh(ctx: &mut Ctx, home: &mut Home) {
    match Ctx::load_at(ctx.root.clone()).and_then(|c| {
        let data = load_data(&c)?;
        Ok((c, data))
    }) {
        Ok((c, data)) => {
            *ctx = c;
            let hint = home.data.identity_hint.take();
            home.set_data(HomeData {
                identity_hint: hint,
                ..data
            });
        }
        Err(e) => home.status = Some((format!("refresh failed: {e:#}"), true)),
    }
    // The change in the detail can be gone (abandon) or rewritten.
    if let Screen::Detail(d) = &home.screen {
        let id = d.info.change_id.clone();
        let scroll = d.scroll;
        home.screen = match load_detail(ctx, &id) {
            Ok(mut fresh) => {
                fresh.scroll = scroll;
                Screen::Detail(Box::new(fresh))
            }
            Err(_) => Screen::Home,
        };
    }
}

fn event_loop(session: &mut Session, home: &mut Home, ctx: &mut Ctx) -> Result<Exit> {
    let mut op_head = jujutsu::current_op_head(&ctx.root);
    loop {
        session.terminal.draw(|f| view::draw(f, home))?;

        if let Screen::OpLog(pane) = &mut home.screen
            && let Some(id) = pane.needs_detail_fetch().map(str::to_string)
        {
            let text = jujutsu::op_show(&ctx.root, &id)
                .unwrap_or_else(|e| format!("jj op show failed: {e:#}"));
            pane.set_detail(&id, text);
            continue;
        }

        if !event::poll(Duration::from_millis(700))? {
            // Another jj command changed the repo: show it.
            let now = jujutsu::current_op_head(&ctx.root);
            if now != op_head && home.modal.is_none() {
                refresh(ctx, home);
                op_head = jujutsu::current_op_head(&ctx.root);
            }
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if home.modal.is_none() {
            home.status = None;
        }
        let intent = home.handle_key(key);
        if let Some(exit) = handle_intent(session, home, ctx, intent)? {
            return Ok(exit);
        }
        op_head = jujutsu::current_op_head(&ctx.root);
    }
}

/// Do what a key asked for. `Some` ends the event loop.
fn handle_intent(
    session: &mut Session,
    home: &mut Home,
    ctx: &mut Ctx,
    intent: Intent,
) -> Result<Option<Exit>> {
    match intent {
        Intent::None => {}
        Intent::Quit => return Ok(Some(Exit::Quit)),
        Intent::Workspaces => return Ok(Some(Exit::Workspaces)),
        Intent::Refresh => {
            refresh(ctx, home);
            home.status = Some(("Refreshed.".to_string(), false));
        }
        Intent::Preview(action) => {
            home.modal = Some(Modal::Preview(actions::preview(ctx, &action)));
        }
        Intent::Run(action) => run_action(session, home, ctx, &action)?,
        Intent::OpenDetail(rev) => match load_detail(ctx, &rev) {
            Ok(d) => home.screen = Screen::Detail(Box::new(d)),
            Err(e) => home.status = Some((format!("cannot show {rev}: {e:#}"), true)),
        },
        Intent::OpenOpLog => match jujutsu::op_log(&ctx.root, 200) {
            Ok(ops) => home.screen = Screen::OpLog(Box::new(OpLogPane::new(ops, &[]))),
            Err(e) => home.status = Some((format!("jj op log failed: {e:#}"), true)),
        },
    }
    Ok(None)
}

fn run_action(
    session: &mut Session,
    home: &mut Home,
    ctx: &mut Ctx,
    action: &HomeAction,
) -> Result<()> {
    let outcome = if let Some(args) = actions::run_interactive_args(action) {
        let command = jujutsu::display_command(args);
        session.suspend()?;
        eprintln!("$ {command}");
        let result = jujutsu::run_jj_step_interactive(&ctx.root, args);
        session.resume()?;
        let ok = matches!(result, Ok(true));
        let mut lines = vec![format!("$ {command}")];
        match result {
            Ok(true) => {}
            Ok(false) => lines.push("jj stopped with an error (see the terminal output).".into()),
            Err(e) => lines.extend(format!("{e:#}").lines().map(str::to_string)),
        }
        Outcome {
            title: action.title(),
            lines,
            ok,
            offer: None,
        }
    } else {
        home.modal = Some(Modal::Running(action.title()));
        session.terminal.draw(|f| view::draw(f, home))?;
        actions::execute(ctx, action)
    };
    refresh(ctx, home);
    if outcome.ok && outcome.offer.is_none() && action.interactive() {
        // The user saw the editor; one line is enough.
        home.modal = None;
        home.status = Some((format!("Done: {}", outcome.lines[0]), false));
    } else {
        home.modal = Some(Modal::Outcome { outcome, scroll: 0 });
    }
    Ok(())
}

/// Raw mode + alternate screen, restored on drop.
struct Session {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    active: bool,
}

impl Session {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode().context("failed to enable raw mode")?;
        let mut stdout = io::stdout();
        if let Err(e) = crossterm::execute!(stdout, EnterAlternateScreen) {
            let _ = terminal::disable_raw_mode();
            return Err(e).context("failed to enter the alternate screen");
        }
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(Self {
            terminal,
            active: true,
        })
    }

    /// Give the terminal to a child process (an editor).
    fn suspend(&mut self) -> Result<()> {
        terminal::disable_raw_mode()?;
        crossterm::execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;
        Ok(())
    }

    fn resume(&mut self) -> Result<()> {
        crossterm::execute!(
            self.terminal.backend_mut(),
            EnterAlternateScreen,
            terminal::Clear(terminal::ClearType::All)
        )?;
        terminal::enable_raw_mode()?;
        // A new Terminal has empty buffers, so the next draw repaints
        // everything. (`Terminal::clear` asks the terminal for the cursor
        // position, which can time out.)
        self.terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        Ok(())
    }

    fn leave(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        let _ = terminal::disable_raw_mode();
        let _ = crossterm::execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.leave();
    }
}
