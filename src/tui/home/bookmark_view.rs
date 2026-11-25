//! Bookmark view: the state of one bookmark on each remote, and the log of
//! the changes on it (from `trunk()` to the bookmark, its remote copies,
//! and your working copy when it is on top of the bookmark).

use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Paragraph};

use super::log_pane::LogPane;
use super::view::{border_style, dim, key_style};
use crate::jujutsu::bookmarks::BookmarkRow;

pub(crate) struct BookmarkView {
    pub row: BookmarkRow,
    pub log: LogPane,
    /// Remote columns, push remote first.
    pub remotes: Vec<String>,
    pub push_remote: String,
    pub upstream_remote: String,
    /// The bookmark has no commits after `trunk()`: the log shows its
    /// recent history instead.
    pub history_only: bool,
}

/// A jj string literal for a revset (`"name"`, escaped).
fn literal(name: &str) -> String {
    format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The local bookmark and all its remote copies.
pub(crate) fn tips(name: &str) -> String {
    let q = literal(name);
    format!("(bookmarks(exact:{q}) | remote_bookmarks(exact:{q}))")
}

/// Revsets of the view: the bookmark's own changes (with `trunk()` as the
/// base and the path to `@`), and its recent history (when it has no own
/// changes, for example `main`).
pub(crate) fn revsets(name: &str) -> (String, String) {
    let t = tips(name);
    (
        format!("trunk() | (trunk()..({t} | ({t}:: & ::@)))"),
        format!("ancestors({t}, 15) | ({t}:: & ::@)"),
    )
}

/// Revset whose size tells whether the bookmark has own changes.
pub(crate) fn own_changes(name: &str) -> String {
    format!("trunk()..{}", tips(name))
}

/// One line about the bookmark on one remote, and the key that fixes it.
fn remote_status(view: &BookmarkView, remote: &str) -> (String, Option<(&'static str, String)>) {
    let row = &view.row;
    let Some(cell) = row.remote(remote) else {
        return if row.local.is_some() && remote == view.push_remote {
            (
                format!("not on {remote} yet"),
                Some(("p", "push".to_string())),
            )
        } else {
            (format!("not on {remote}"), None)
        };
    };
    let tracked = if cell.tracked { "" } else { " (not tracked)" };
    if row.local.is_none() {
        let id = cell.change_id.as_deref().unwrap_or("?");
        return (
            format!("at {}{tracked}", &id[..id.len().min(8)]),
            Some(("s", "switch: start work on it".to_string())),
        );
    }
    let plural = |n: u64| if n == 1 { "" } else { "s" };
    match (cell.local_ahead, cell.local_behind) {
        (Some(0), Some(0)) => (format!("same as local{tracked}"), None),
        (Some(a), Some(0)) => {
            let fix = (remote == view.push_remote).then(|| ("p", "push".to_string()));
            (
                format!("{a} local commit{} not on {remote}{tracked}", plural(a)),
                fix,
            )
        }
        (Some(0), Some(b)) => (
            format!(
                "{b} commit{} on {remote} that you do not have{tracked}",
                plural(b)
            ),
            Some(("y", "sync".to_string())),
        ),
        (Some(a), Some(b)) => (
            format!("diverged: {a} local, {b} on {remote}{tracked}"),
            Some(("y", "sync".to_string())),
        ),
        _ => (format!("unknown{tracked}"), None),
    }
}

fn summary_lines(view: &BookmarkView) -> Vec<Line<'static>> {
    let row = &view.row;
    let name_style = if row.protected {
        Style::default().fg(Color::Yellow).bold()
    } else {
        Style::default().fg(Color::Green).bold()
    };
    let mut first = vec![Span::styled(row.name.clone(), name_style)];
    match &row.local {
        Some(l) if l.conflict => first.push(Span::styled(
            "  local: CONFLICT (press y to sync)",
            Style::default().fg(Color::Red),
        )),
        Some(l) => {
            let id = l.change_id.as_deref().unwrap_or("?");
            first.push(Span::styled("  local ", dim()));
            first.push(Span::styled(
                id[..id.len().min(8)].to_string(),
                Style::default().fg(Color::Magenta),
            ));
        }
        None => first.push(Span::styled("  remote only", dim())),
    }
    if row.protected {
        first.push(Span::styled(
            "  protected: push it as a new bookmark",
            Style::default().fg(Color::Yellow),
        ));
    }
    let mut lines = vec![Line::from(first)];
    let width = view.remotes.iter().map(String::len).max().unwrap_or(6) + 2;
    for remote in &view.remotes {
        let (text, fix) = remote_status(view, remote);
        let mut spans = vec![
            Span::styled(format!("@{remote:<width$}"), dim()),
            Span::raw(text),
        ];
        if remote == &view.upstream_remote && remote != &view.push_remote {
            spans.push(Span::styled("  (forge)", dim()));
        }
        if let Some((key, label)) = fix {
            spans.push(Span::raw("   "));
            spans.push(Span::styled(key, key_style()));
            spans.push(Span::styled(format!(" {label}"), dim()));
        }
        lines.push(Line::from(spans));
    }
    lines
}

pub(crate) fn draw(frame: &mut Frame, area: Rect, view: &mut BookmarkView) {
    let summary = summary_lines(view);
    let h = u16::try_from(summary.len() + 2)
        .unwrap_or(u16::MAX)
        .min(area.height / 2);
    let [top, body] = Layout::vertical([Constraint::Length(h), Constraint::Min(3)]).areas(area);
    let block = Block::bordered()
        .title(" Bookmark ")
        .border_type(BorderType::Rounded)
        .border_style(border_style(false));
    frame.render_widget(Paragraph::new(summary).block(block), top);
    let title = if view.history_only {
        format!(
            " History of {} (no own changes after trunk) ",
            view.row.name
        )
    } else {
        format!(" Changes on {} ", view.row.name)
    };
    view.log.draw(frame, body, true, &title);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revsets_quote_the_name() {
        assert_eq!(
            tips("fix/goals"),
            r#"(bookmarks(exact:"fix/goals") | remote_bookmarks(exact:"fix/goals"))"#
        );
        let (own, history) = revsets("a\"b");
        assert!(own.starts_with("trunk() | (trunk()..((bookmarks(exact:\"a\\\"b\")"));
        assert!(history.starts_with("ancestors((bookmarks(exact:\"a\\\"b\")"));
    }
}
