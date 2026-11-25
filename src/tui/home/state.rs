//! State and key handling of the home view. No I/O here: a key gives an
//! [`Intent`], and the event loop in `mod.rs` does the jj work.

use ansi_to_tui::IntoText;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::Text;

use super::actions::{HomeAction, Outcome, Preview, offer_action};
use super::log_pane::LogPane;
use crate::jujutsu::bookmarks::BookmarkRow;
use crate::jujutsu::changes::ChangeInfo;
use crate::tui::op_log_pane::OpLogPane;

/// Everything the home view shows, loaded by `mod.rs`.
#[derive(Debug, Clone, Default)]
pub(crate) struct HomeData {
    pub repo_name: String,
    /// Remote columns, push remote first.
    pub remotes: Vec<String>,
    pub push_remote: String,
    pub rows: Vec<BookmarkRow>,
    pub log_ansi: String,
    pub line_heads: Vec<Option<String>>,
    /// Change id of `@`.
    pub working_copy: Option<String>,
    /// The bookmark of the current work (see `flow::push`).
    pub current_bookmark: Option<String>,
    /// Set when jj has no user name or email.
    pub identity_hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pane {
    Bookmarks,
    Log,
}

/// Change detail: description, files and the coloured diff.
pub(crate) struct Detail {
    pub info: ChangeInfo,
    pub diff: Text<'static>,
    pub scroll: u16,
    /// Lines of the whole detail text (set by the view).
    pub content_lines: u16,
    /// Visible height (set by the view).
    pub page: u16,
}

impl Detail {
    pub(crate) fn new(info: ChangeInfo, diff_ansi: &str) -> Self {
        let diff = diff_ansi
            .into_text()
            .unwrap_or_else(|_| Text::from(diff_ansi.to_string()));
        Self {
            info,
            diff,
            scroll: 0,
            content_lines: 0,
            page: 0,
        }
    }

    fn max_scroll(&self) -> u16 {
        self.content_lines.saturating_sub(self.page)
    }

    fn scroll_by(&mut self, delta: i32) {
        let next = (i32::from(self.scroll) + delta).clamp(0, i32::from(self.max_scroll()));
        self.scroll = u16::try_from(next).unwrap_or(0);
    }
}

pub(crate) enum Screen {
    Home,
    Detail(Box<Detail>),
    OpLog(Box<OpLogPane>),
}

pub(crate) enum Modal {
    Preview(Preview),
    /// Shown while a command runs (the UI waits for it).
    Running(String),
    Outcome {
        outcome: Outcome,
        scroll: u16,
    },
    Help,
}

/// What the event loop must do after a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Intent {
    None,
    Quit,
    Refresh,
    /// Dry-run the action and show the command preview.
    Preview(HomeAction),
    /// Run the action (the user saw its commands).
    Run(HomeAction),
    /// Open the change detail for this revision.
    OpenDetail(String),
    OpenOpLog,
    /// Leave and open the workspace TUI (`dojo ws tui`).
    Workspaces,
}

pub(crate) struct Home {
    pub data: HomeData,
    pub log: LogPane,
    /// Index into `data.rows`.
    pub bookmark_sel: usize,
    pub focus: Pane,
    pub screen: Screen,
    pub modal: Option<Modal>,
    /// One-line message: `(text, is_error)`.
    pub status: Option<(String, bool)>,
}

/// Length of change ids in commands (unique in practice, readable).
const ID_LEN: usize = 12;

pub(crate) fn short_change(id: &str) -> &str {
    &id[..id.len().min(ID_LEN)]
}

impl Home {
    pub(crate) fn new(data: HomeData) -> Self {
        let mut log = LogPane::new(&data.log_ansi, data.line_heads.clone());
        if let Some(wc) = &data.working_copy {
            log.select_change(wc);
        }
        let bookmark_sel = data
            .current_bookmark
            .as_ref()
            .and_then(|b| data.rows.iter().position(|r| &r.name == b))
            .unwrap_or(0);
        let status = data.identity_hint.as_ref().map(|h| {
            let first = h.lines().next().unwrap_or_default();
            (format!("{first} See `dojo status`."), true)
        });
        Self {
            data,
            log,
            bookmark_sel,
            focus: Pane::Bookmarks,
            screen: Screen::Home,
            modal: None,
            status,
        }
    }

    /// Replace the data after a refresh; keep the selections where possible.
    pub(crate) fn set_data(&mut self, data: HomeData) {
        let old_bookmark = self.selected_bookmark().map(|r| r.name.clone());
        let old_change = self.log.selected_change().map(str::to_string);
        self.log = LogPane::new(&data.log_ansi, data.line_heads.clone());
        let kept = old_change.is_some_and(|c| self.log.select_change(&c));
        if !kept && let Some(wc) = &data.working_copy {
            self.log.select_change(wc);
        }
        self.bookmark_sel = old_bookmark
            .and_then(|b| data.rows.iter().position(|r| r.name == b))
            .unwrap_or(0);
        self.data = data;
    }

    pub(crate) fn selected_bookmark(&self) -> Option<&BookmarkRow> {
        self.data.rows.get(self.bookmark_sel)
    }

    /// The change the change keys (describe, new, edit, ...) act on.
    pub(crate) fn target_change(&self) -> Option<String> {
        match &self.screen {
            Screen::Detail(d) => return Some(d.info.change_id.clone()),
            Screen::OpLog(_) => return None,
            Screen::Home => {}
        }
        match self.focus {
            Pane::Log => self.log.selected_change().map(str::to_string),
            Pane::Bookmarks => self.selected_bookmark().and_then(|r| {
                r.local
                    .as_ref()
                    .and_then(|l| l.change_id.clone())
                    .or_else(|| r.remotes.iter().find_map(|c| c.change_id.clone()))
            }),
        }
    }

    /// The bookmark that sync, push and pr act on.
    fn target_bookmark(&self) -> Option<String> {
        match (self.focus, &self.screen) {
            (Pane::Bookmarks, Screen::Home) => self.selected_bookmark().map(|r| r.name.clone()),
            _ => self.data.current_bookmark.clone(),
        }
    }

    fn set_status(&mut self, text: impl Into<String>, error: bool) {
        self.status = Some((text.into(), error));
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Intent {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Intent::Quit;
        }
        if self.modal.is_some() {
            return self.handle_modal_key(key);
        }
        match &mut self.screen {
            Screen::Detail(d) => {
                let page = i32::from(d.page.max(2));
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => {
                        self.screen = Screen::Home;
                        return Intent::None;
                    }
                    KeyCode::Down | KeyCode::Char('j') => d.scroll_by(1),
                    KeyCode::Up | KeyCode::Char('k') => d.scroll_by(-1),
                    KeyCode::PageDown | KeyCode::Char(' ') => d.scroll_by(page - 1),
                    KeyCode::PageUp => d.scroll_by(1 - page),
                    KeyCode::Char('g') | KeyCode::Home => d.scroll = 0,
                    KeyCode::Char('G') | KeyCode::End => d.scroll = d.max_scroll(),
                    _ => return self.change_key(key.code),
                }
                Intent::None
            }
            Screen::OpLog(pane) => match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('o') => {
                    self.screen = Screen::Home;
                    Intent::None
                }
                KeyCode::Char('u') => self.change_key(KeyCode::Char('u')),
                code => {
                    pane.handle_key(code);
                    Intent::None
                }
            },
            Screen::Home => self.home_key(key.code),
        }
    }

    fn handle_modal_key(&mut self, key: KeyEvent) -> Intent {
        let Some(modal) = &mut self.modal else {
            return Intent::None;
        };
        match modal {
            Modal::Running(_) => Intent::None,
            Modal::Help => {
                self.modal = None;
                Intent::None
            }
            Modal::Preview(p) => match key.code {
                KeyCode::Enter | KeyCode::Char('y') if p.runnable => {
                    let action = p.action.clone();
                    self.modal = None;
                    Intent::Run(action)
                }
                KeyCode::Esc | KeyCode::Char('n' | 'q') | KeyCode::Enter => {
                    self.modal = None;
                    Intent::None
                }
                _ => Intent::None,
            },
            Modal::Outcome { outcome, scroll } => match key.code {
                // The offer's commands are on screen: one key runs it.
                KeyCode::Enter if outcome.offer.is_some() => {
                    let action = outcome.offer.as_ref().map(offer_action);
                    self.modal = None;
                    action.map_or(Intent::None, Intent::Run)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    *scroll = scroll.saturating_add(1);
                    Intent::None
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    *scroll = scroll.saturating_sub(1);
                    Intent::None
                }
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => {
                    self.modal = None;
                    Intent::None
                }
                _ => Intent::None,
            },
        }
    }

    fn home_key(&mut self, code: KeyCode) -> Intent {
        match code {
            KeyCode::Char('q') => Intent::Quit,
            KeyCode::Char('?') => {
                self.modal = Some(Modal::Help);
                Intent::None
            }
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
                self.focus = match self.focus {
                    Pane::Bookmarks => Pane::Log,
                    Pane::Log => Pane::Bookmarks,
                };
                Intent::None
            }
            KeyCode::Char('h') => {
                self.focus = Pane::Bookmarks;
                Intent::None
            }
            KeyCode::Char('l') => {
                self.focus = Pane::Log;
                Intent::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                Intent::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                Intent::None
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.move_selection(i32::MIN);
                Intent::None
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.move_selection(i32::MAX);
                Intent::None
            }
            KeyCode::Enter => match self.target_change() {
                Some(c) => Intent::OpenDetail(c),
                None => Intent::None,
            },
            KeyCode::Char('r') => Intent::Refresh,
            KeyCode::Char('o') => Intent::OpenOpLog,
            KeyCode::Char('w') => Intent::Workspaces,
            KeyCode::Char('f') => Intent::Preview(HomeAction::Fetch),
            KeyCode::Char('s') => match (self.focus, self.selected_bookmark()) {
                (Pane::Bookmarks, Some(r)) => Intent::Preview(HomeAction::Switch(r.name.clone())),
                _ => {
                    self.set_status(
                        "Select a bookmark (Tab) to switch to it, or press n for a new change here.",
                        false,
                    );
                    Intent::None
                }
            },
            KeyCode::Char('p') => {
                let bookmark = match self.focus {
                    Pane::Bookmarks => self.selected_bookmark().map(|r| r.name.clone()),
                    Pane::Log => None,
                };
                Intent::Preview(HomeAction::Push(bookmark))
            }
            KeyCode::Char('y') => self.with_bookmark("sync", HomeAction::Sync),
            KeyCode::Char('P') => self.with_bookmark("open a pull request for", HomeAction::Pr),
            code => self.change_key(code),
        }
    }

    fn with_bookmark(&mut self, verb: &str, make: fn(String) -> HomeAction) -> Intent {
        match self.target_bookmark() {
            Some(b) => Intent::Preview(make(b)),
            None => {
                self.set_status(
                    format!("No bookmark to {verb}. Select one in the bookmarks pane."),
                    true,
                );
                Intent::None
            }
        }
    }

    /// Keys that act on one change (in the log, the detail, or the
    /// selected bookmark's change).
    fn change_key(&mut self, code: KeyCode) -> Intent {
        if code == KeyCode::Char('u') {
            return Intent::Preview(HomeAction::jj(
                "Undo the last jj operation",
                &["undo"],
                "undo the last operation (jj op log shows it)",
                false,
            ));
        }
        let make: fn(&str) -> HomeAction = match code {
            KeyCode::Char('d') => |c| {
                HomeAction::jj(
                    &format!("Describe change {c}"),
                    &["describe", c],
                    "edit the description in your editor",
                    true,
                )
            },
            KeyCode::Char('n') => |c| {
                HomeAction::jj(
                    &format!("New change on top of {c}"),
                    &["new", c],
                    "start a new empty change on top of it",
                    false,
                )
            },
            KeyCode::Char('e') => |c| {
                HomeAction::jj(
                    &format!("Edit change {c}"),
                    &["edit", c],
                    "make it the working-copy change",
                    false,
                )
            },
            KeyCode::Char('S') => |c| {
                HomeAction::jj(
                    &format!("Squash change {c} into its parent"),
                    &["squash", "--revision", c],
                    "move its changes into the parent (jj asks for a description when both have one)",
                    true,
                )
            },
            KeyCode::Char('a') => |c| {
                HomeAction::jj(
                    &format!("Abandon change {c}"),
                    &["abandon", c],
                    "remove the change; its children move to its parent (jj undo brings it back)",
                    false,
                )
            },
            KeyCode::Char('x') => |c| {
                HomeAction::jj(
                    &format!("Split change {c}"),
                    &["split", "--revision", c],
                    "choose the part for the first change in the diff editor",
                    true,
                )
            },
            _ => return Intent::None,
        };
        match self.target_change() {
            Some(c) => Intent::Preview(make(short_change(&c))),
            None => {
                self.set_status("No change selected.", true);
                Intent::None
            }
        }
    }

    fn move_selection(&mut self, delta: i32) {
        match self.focus {
            Pane::Bookmarks => {
                let len = self.data.rows.len();
                if len == 0 {
                    return;
                }
                self.bookmark_sel = match delta {
                    i32::MIN => 0,
                    i32::MAX => len - 1,
                    d if d < 0 => self.bookmark_sel.saturating_sub(1),
                    _ => (self.bookmark_sel + 1).min(len - 1),
                };
            }
            Pane::Log => match delta {
                i32::MIN => self.log.top(),
                i32::MAX => self.log.bottom(),
                d if d < 0 => self.log.up(),
                _ => self.log.down(),
            },
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::jujutsu::bookmarks::{LocalCell, RemoteCell};

    pub(crate) fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    pub(crate) fn code(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    fn remote(name: &str, change: &str, tracked: bool, ahead: u64) -> RemoteCell {
        RemoteCell {
            remote: name.into(),
            commit_id: Some(format!("{change}c0ffee")),
            change_id: Some(change.into()),
            tracked,
            local_ahead: Some(ahead),
            local_behind: Some(0),
        }
    }

    /// Fixed data for the state and snapshot tests.
    pub(crate) fn sample_data() -> HomeData {
        let wc = "rtssmotxlmvtwkqnzzpyoqklrzvnulnx";
        let fix = "qsspzlznyywrkmnvmqulnoyvqtzswotq";
        let main = "npppruztlspuknmrxvnszvyvlqtuuzrs";
        let rows = vec![
            BookmarkRow {
                name: "fix/goals".into(),
                local: Some(LocalCell {
                    commit_id: Some("72fc598a61f3".into()),
                    change_id: Some(fix.into()),
                    conflict: false,
                    conflict_change_ids: Vec::new(),
                }),
                remotes: vec![
                    remote("sundae", fix, true, 0),
                    remote("origin", "tpkvspyskppl", false, 1),
                ],
                protected: false,
            },
            BookmarkRow {
                name: "main".into(),
                local: Some(LocalCell {
                    commit_id: Some("0963c234283f".into()),
                    change_id: Some(main.into()),
                    conflict: false,
                    conflict_change_ids: Vec::new(),
                }),
                remotes: vec![
                    remote("sundae", main, false, 0),
                    remote("origin", main, true, 0),
                ],
                protected: true,
            },
            BookmarkRow {
                name: "from-forge".into(),
                local: None,
                remotes: vec![remote("origin", "zzwwkkppqqrr", false, 0)],
                protected: false,
            },
        ];
        let log_ansi = concat!(
            "@  rtssmotx test@example.com 2026-11-25 15:00 3d015f14\n",
            "│  (empty) (no description set)\n",
            "○  qsspzlzn forge@example.com 2026-11-25 14:00 fix/goals 72fc598a\n",
            "│  fix goals\n",
            "◆  npppruzt forge@example.com 2026-11-25 13:00 main 0963c234\n",
            "│  add goals\n",
            "~\n",
        )
        .to_string();
        let line_heads = [wc, wc, fix, fix, main, main]
            .iter()
            .map(|s| Some((*s).to_string()))
            .chain(std::iter::once(None))
            .collect();
        HomeData {
            repo_name: "dojo".into(),
            remotes: vec!["sundae".into(), "origin".into()],
            push_remote: "sundae".into(),
            rows,
            log_ansi,
            line_heads,
            working_copy: Some(wc.into()),
            current_bookmark: Some("fix/goals".into()),
            identity_hint: None,
        }
    }

    #[test]
    fn starts_on_the_current_bookmark_and_working_copy() {
        let home = Home::new(sample_data());
        assert_eq!(home.selected_bookmark().unwrap().name, "fix/goals");
        assert_eq!(
            home.log.selected_change(),
            Some("rtssmotxlmvtwkqnzzpyoqklrzvnulnx")
        );
    }

    #[test]
    fn keys_give_previews_for_the_selected_item() {
        let mut home = Home::new(sample_data());
        assert_eq!(
            home.handle_key(key('s')),
            Intent::Preview(HomeAction::Switch("fix/goals".into()))
        );
        assert_eq!(
            home.handle_key(key('y')),
            Intent::Preview(HomeAction::Sync("fix/goals".into()))
        );
        home.handle_key(key('j'));
        assert_eq!(
            home.handle_key(key('p')),
            Intent::Preview(HomeAction::Push(Some("main".into())))
        );
        // Log pane: change keys act on the selected change.
        home.handle_key(code(KeyCode::Tab));
        home.handle_key(key('j'));
        match home.handle_key(key('n')) {
            Intent::Preview(HomeAction::Jj { args, .. }) => {
                assert_eq!(args, vec!["new", "qsspzlznyywr"]);
            }
            other => panic!("unexpected {other:?}"),
        }
        match home.handle_key(key('d')) {
            Intent::Preview(a) => assert!(a.interactive()),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(
            home.handle_key(code(KeyCode::Enter)),
            Intent::OpenDetail("qsspzlznyywrkmnvmqulnoyvqtzswotq".into())
        );
        assert_eq!(home.handle_key(key('q')), Intent::Quit);
    }

    #[test]
    fn preview_enter_runs_and_outcome_enter_runs_the_offer() {
        let mut home = Home::new(sample_data());
        home.modal = Some(Modal::Preview(Preview {
            action: HomeAction::Fetch,
            title: String::new(),
            commands: Vec::new(),
            dojo: None,
            note: None,
            runnable: true,
        }));
        assert_eq!(
            home.handle_key(code(KeyCode::Enter)),
            Intent::Run(HomeAction::Fetch)
        );
        assert!(home.modal.is_none());
        home.modal = Some(Modal::Outcome {
            outcome: Outcome {
                title: String::new(),
                lines: Vec::new(),
                ok: false,
                offer: Some(crate::flow::Offer {
                    label: "Sync".into(),
                    dojo: "dojo sync fix/goals".into(),
                    commands: Vec::new(),
                    action: crate::flow::OfferAction::Sync {
                        bookmark: "fix/goals".into(),
                    },
                }),
            },
            scroll: 0,
        });
        assert_eq!(
            home.handle_key(code(KeyCode::Enter)),
            Intent::Run(HomeAction::Sync("fix/goals".into()))
        );
    }
}
