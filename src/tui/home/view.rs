//! Drawing of the home view. Pure functions of [`Home`]; the snapshot tests
//! render them on a `TestBackend`.

use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use super::actions::{Outcome, Preview};
use super::state::{Home, Modal, Pane, Screen};
use super::{bookmark_view, detail};
use crate::flow::branches::ahead_behind;
use crate::jujutsu::bookmarks::BookmarkRow;
use crate::text_utils::truncate_end;

/// From this width on, the bookmarks and the log are side by side.
const WIDE: u16 = 100;

pub(crate) fn border_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::White).bold()
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

pub(crate) fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub(crate) fn key_style() -> Style {
    Style::default().fg(Color::Cyan).bold()
}

pub(crate) fn draw(frame: &mut Frame, home: &mut Home) {
    let area = frame.area();
    let [header, body, status, hints] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_header(frame, header, home);
    match &mut home.screen {
        Screen::Home => draw_home(frame, body, home),
        Screen::Bookmark(v) => bookmark_view::draw(frame, body, v),
        Screen::Detail(d) => detail::draw(frame, body, d),
        Screen::OpLog(pane) => pane.draw(frame, body, true, true, false),
    }
    if let Some((text, error)) = &home.status {
        let style = if *error {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Yellow)
        };
        frame.render_widget(
            Paragraph::new(truncate_end(text, status.width as usize)).style(style),
            status,
        );
    }
    draw_hints(frame, hints, home);
    if let Some(modal) = &home.modal {
        draw_modal(frame, area, modal);
    }
}

fn draw_header(frame: &mut Frame, area: Rect, home: &Home) {
    let d = &home.data;
    let mut spans = vec![
        Span::styled(
            " dojo ",
            Style::default().fg(Color::Black).bg(Color::Cyan).bold(),
        ),
        Span::raw(" "),
        Span::styled(d.repo_name.clone(), Style::default().bold()),
    ];
    if let Some(wc) = &d.working_copy {
        spans.push(Span::styled("  @ ", dim()));
        spans.push(Span::styled(
            wc[..wc.len().min(8)].to_string(),
            Style::default().fg(Color::Magenta),
        ));
    }
    if let Some(b) = &d.current_bookmark {
        spans.push(Span::styled(" on ", dim()));
        spans.push(Span::styled(b.clone(), Style::default().fg(Color::Green)));
    }
    if !d.push_remote.is_empty() {
        spans.push(Span::styled("  push → ", dim()));
        spans.push(Span::raw(d.push_remote.clone()));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_home(frame: &mut Frame, area: Rect, home: &mut Home) {
    if area.width >= WIDE {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
                .areas(area);
        let focused = home.focus == Pane::Log;
        home.log.draw(frame, left, focused, " Log ");
        draw_bookmarks(frame, right, home, false);
    } else {
        match home.focus {
            Pane::Log => home.log.draw(frame, area, true, " Log · Tab: bookmarks "),
            Pane::Bookmarks => draw_bookmarks(frame, area, home, true),
        }
    }
}

/// Cell text of one remote column: ahead/behind vs the local bookmark, or
/// the remote's change id when there is no local bookmark.
fn remote_text(row: &BookmarkRow, remote: &str) -> String {
    let Some(cell) = row.remote(remote) else {
        return "-".to_string();
    };
    if row.local.is_none() {
        return cell
            .change_id
            .as_deref()
            .map_or("?".to_string(), |c| c[..c.len().min(8)].to_string());
    }
    ahead_behind(Some(cell), true)
}

fn notes(row: &BookmarkRow) -> String {
    let mut n = Vec::new();
    if row.protected {
        n.push("protected");
    }
    if row.conflicted() {
        n.push("conflicted");
    }
    if row.local.is_none() {
        n.push("remote only");
    } else if !row.is_tracked_anywhere() && !row.remotes.is_empty() {
        n.push("untracked");
    }
    n.join(", ")
}

fn pad(s: &str, width: usize) -> String {
    let s = truncate_end(s, width);
    let w = UnicodeWidthStr::width(s.as_str());
    format!("{s}{}", " ".repeat(width.saturating_sub(w)))
}

fn draw_bookmarks(frame: &mut Frame, area: Rect, home: &Home, narrow: bool) {
    let focused = home.focus == Pane::Bookmarks;
    let title = if narrow {
        " Bookmarks · Tab: log · Enter: open "
    } else {
        " Bookmarks "
    };
    let block = Block::bordered()
        .title(title)
        .border_type(BorderType::Rounded)
        .border_style(border_style(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = inner.width as usize;
    let rows = &home.data.rows;
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new("No bookmarks. Press f to fetch.").style(dim()),
            inner,
        );
        return;
    }

    // Columns: marker, name, local, one per remote, notes. Drop the local
    // column, then the notes, when the pane is narrow.
    let name_w = rows
        .iter()
        .map(|r| UnicodeWidthStr::width(r.name.as_str()))
        .max()
        .unwrap_or(4)
        .clamp(6, 24);
    let remote_w: Vec<usize> = home
        .data
        .remotes
        .iter()
        .map(|r| (r.len() + 1).max(6))
        .collect();
    let fixed = 2 + name_w + 1 + remote_w.iter().map(|w| w + 1).sum::<usize>();
    let show_local = fixed + 9 + 6 <= width;
    let used = fixed + if show_local { 9 } else { 0 };
    let notes_w = width.saturating_sub(used);
    let show_notes = notes_w >= 6;

    let mut header = format!("  {} ", pad("BOOKMARK", name_w));
    if show_local {
        header.push_str(&pad("LOCAL", 8));
        header.push(' ');
    }
    for (r, w) in home.data.remotes.iter().zip(&remote_w) {
        header.push_str(&pad(&format!("@{r}"), *w));
        header.push(' ');
    }
    if show_notes {
        header.push_str("NOTES");
    }
    let mut lines = vec![Line::styled(truncate_end(&header, width), dim())];

    let visible = (inner.height as usize).saturating_sub(1);
    let first = home.bookmark_sel.saturating_sub(visible.saturating_sub(1));
    for (i, row) in rows.iter().enumerate().skip(first).take(visible) {
        let selected = i == home.bookmark_sel;
        let marker = if selected { "▶ " } else { "  " };
        let name_style = if row.protected {
            Style::default().fg(Color::Yellow)
        } else if row.local.is_none() {
            dim()
        } else {
            Style::default().fg(Color::Green)
        };
        let mut spans = vec![
            Span::styled(marker, key_style()),
            Span::styled(pad(&row.name, name_w), name_style),
            Span::raw(" "),
        ];
        if show_local {
            let local = match &row.local {
                Some(l) if l.conflict => "CONFLICT".to_string(),
                Some(l) => l
                    .change_id
                    .as_deref()
                    .map_or("-".to_string(), |c| c[..c.len().min(8)].to_string()),
                None => "-".to_string(),
            };
            spans.push(Span::styled(
                pad(&local, 8),
                Style::default().fg(Color::Magenta),
            ));
            spans.push(Span::raw(" "));
        }
        for (r, w) in home.data.remotes.iter().zip(&remote_w) {
            let text = remote_text(row, r);
            let tracked = row.remote(r).is_some_and(|c| c.tracked);
            let style = if text.starts_with('+') || text.contains('-') && text != "-" {
                Style::default().fg(Color::Yellow)
            } else if tracked {
                Style::default()
            } else {
                dim()
            };
            spans.push(Span::styled(pad(&text, *w), style));
            spans.push(Span::raw(" "));
        }
        if show_notes {
            spans.push(Span::styled(truncate_end(&notes(row), notes_w), dim()));
        }
        let mut line = Line::from(spans);
        if selected && focused {
            line = line.style(Style::default().bg(Color::Rgb(40, 44, 52)));
        }
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn hint_spans(pairs: &[(&str, &str)], width: usize) -> Line<'static> {
    let mut spans = Vec::new();
    let mut used = 0;
    for (k, label) in pairs {
        let w = UnicodeWidthStr::width(*k) + UnicodeWidthStr::width(*label) + 3;
        if used + w > width {
            break;
        }
        used += w;
        spans.push(Span::styled(format!(" {k}"), key_style()));
        spans.push(Span::styled(format!(" {label} "), dim()));
    }
    Line::from(spans)
}

fn draw_hints(frame: &mut Frame, area: Rect, home: &Home) {
    let pairs: &[(&str, &str)] = match (&home.modal, &home.screen) {
        (Some(Modal::Preview(p)), _) if p.runnable => &[("Enter", "run"), ("Esc", "cancel")],
        (Some(Modal::Outcome { outcome, .. }), _) if outcome.offer.is_some() => &[
            ("Enter", "run next step"),
            ("Esc", "close"),
            ("↑↓", "scroll"),
        ],
        (Some(_), _) => &[("Esc", "close")],
        (None, Screen::Detail(_)) => &[
            ("Esc", "back"),
            ("Tab", "files/diff"),
            ("J K", "file"),
            ("] [", "hunk"),
            ("Space", "page"),
            ("← →", "scroll"),
            ("d", "describe"),
            ("e", "edit"),
            ("?", "help"),
        ],
        (None, Screen::Bookmark(_)) => &[
            ("Esc", "back"),
            ("Enter", "change"),
            ("s", "switch"),
            ("p", "push"),
            ("y", "sync"),
            ("P", "pr"),
            ("n", "new"),
            ("?", "help"),
        ],
        (None, Screen::OpLog(_)) => &[("Esc", "back"), ("↑↓", "move"), ("u", "undo")],
        (None, Screen::Home) => &[
            ("?", "help"),
            ("Tab", "pane"),
            ("Enter", "open"),
            ("f", "fetch"),
            ("s", "switch"),
            ("p", "push"),
            ("y", "sync"),
            ("P", "pr"),
            ("d", "describe"),
            ("n", "new"),
            ("u", "undo"),
            ("q", "quit"),
        ],
    };
    frame.render_widget(Paragraph::new(hint_spans(pairs, area.width as usize)), area);
}

/// Keys for the help box.
const HELP: &[(&str, &str)] = &[
    ("Tab h l", "switch between the log and the bookmarks"),
    ("j k ↑ ↓", "move the selection"),
    (
        "Enter",
        "log: show the change; bookmarks: show the bookmark",
    ),
    ("Esc q", "go back (on the first screen, q quits)"),
    ("f", "fetch from all remotes"),
    ("s", "switch to the bookmark"),
    ("p", "push the bookmark (log: the current work)"),
    ("y", "sync the bookmark with the forge"),
    ("P", "open a pull request"),
    ("d", "describe the change (editor)"),
    ("n", "new change on top of the change"),
    ("e", "edit the change"),
    ("S", "squash the change into its parent"),
    ("a", "abandon the change"),
    ("x", "split the change (diff editor)"),
    ("u", "undo the last jj operation"),
    ("o", "operation log"),
    ("w", "workspaces (dojo ws tui)"),
    ("r", "refresh"),
    ("", ""),
    ("Change:", ""),
    ("Tab", "move between the files and the diff"),
    ("j k", "files: select a file; diff: scroll"),
    ("J K", "next / previous file"),
    ("] [", "next / previous hunk"),
    ("Space PgUp", "page down / up"),
    ("← →", "scroll long lines"),
];

fn modal_lines(modal: &Modal) -> (String, Vec<Line<'static>>, Style) {
    match modal {
        Modal::Help => {
            let lines = HELP
                .iter()
                .map(|(k, v)| {
                    Line::from(vec![
                        Span::styled(format!("{k:<9}"), key_style()),
                        Span::raw((*v).to_string()),
                    ])
                })
                .collect();
            (
                " Keys ".to_string(),
                lines,
                Style::default().fg(Color::Cyan),
            )
        }
        Modal::Running(text) => (
            " Running ".to_string(),
            vec![Line::raw(text.clone())],
            Style::default().fg(Color::Yellow),
        ),
        Modal::Preview(p) => preview_lines(p),
        Modal::Outcome { outcome, .. } => outcome_lines(outcome),
    }
}

fn preview_lines(p: &Preview) -> (String, Vec<Line<'static>>, Style) {
    let mut lines = Vec::new();
    if !p.commands.is_empty() {
        lines.push(Line::styled("dojo runs:", dim()));
        for (cmd, why) in &p.commands {
            lines.push(Line::from(vec![
                Span::styled("$ ", dim()),
                Span::styled(cmd.clone(), Style::default().bold()),
            ]));
            if !why.is_empty() {
                lines.push(Line::styled(format!("    {why}"), dim()));
            }
        }
    }
    if let Some(note) = &p.note {
        lines.push(Line::raw(""));
        for l in note.lines() {
            lines.push(Line::raw(l.to_string()));
        }
    }
    if let Some(d) = &p.dojo {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled("CLI: ", dim()),
            Span::raw(d.clone()),
        ]));
    }
    lines.push(Line::raw(""));
    if p.runnable {
        lines.push(Line::from(vec![
            Span::styled("Enter", key_style()),
            Span::raw(" run   "),
            Span::styled("Esc", key_style()),
            Span::raw(" cancel"),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled("Esc", key_style()),
            Span::raw(" close"),
        ]));
    }
    let color = if p.runnable {
        Color::Cyan
    } else {
        Color::Yellow
    };
    (format!(" {} ", p.title), lines, Style::default().fg(color))
}

fn outcome_lines(o: &Outcome) -> (String, Vec<Line<'static>>, Style) {
    let mut lines: Vec<Line<'static>> = o
        .lines
        .iter()
        .map(|l| {
            if l.starts_with("$ ") {
                Line::styled(l.clone(), Style::default().bold())
            } else if l.starts_with("  ") {
                Line::styled(l.clone(), dim())
            } else {
                Line::raw(l.clone())
            }
        })
        .collect();
    if let Some(offer) = &o.offer {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled("Next: ", Style::default().fg(Color::Cyan).bold()),
            Span::raw(offer.label.clone()),
        ]));
        for c in &offer.commands {
            lines.push(Line::styled(format!("$ {c}"), Style::default().bold()));
        }
        lines.push(Line::from(vec![
            Span::styled("Enter", key_style()),
            Span::raw(" run it   "),
            Span::styled("Esc", key_style()),
            Span::raw(" close"),
        ]));
    }
    let (mark, color) = if o.ok {
        ("✓", Color::Green)
    } else {
        ("✗", Color::Red)
    };
    (
        format!(" {mark} {} ", o.title),
        lines,
        Style::default().fg(color),
    )
}

fn draw_modal(frame: &mut Frame, area: Rect, modal: &Modal) {
    let (title, lines, border) = modal_lines(modal);
    let scroll = match modal {
        Modal::Outcome { scroll, .. } => *scroll,
        _ => 0,
    };
    let width = area.width.saturating_sub(4).min(84).max(20.min(area.width));
    let inner_w = width.saturating_sub(2).max(1) as usize;
    // Wrapped height estimate (each line takes ceil(width / inner_w) rows).
    let rows: usize = lines
        .iter()
        .map(|l| l.width().max(1).div_ceil(inner_w))
        .sum();
    let height = u16::try_from(rows + 2)
        .unwrap_or(u16::MAX)
        .min(area.height.saturating_sub(2))
        .max(3.min(area.height));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let rect = Rect::new(x, y, width, height);
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .title(truncate_end(&title, width.saturating_sub(4) as usize))
        .border_type(BorderType::Rounded)
        .border_style(border);
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        rect,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::home::actions::HomeAction;
    use crate::tui::home::detail::tests::sample_detail;
    use crate::tui::home::state::tests::{code, key, sample_bookmark_view, sample_data};
    use crossterm::event::KeyCode;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    /// Render the home view and return the screen as text (one line per
    /// row, trailing spaces removed).
    fn render(home: &mut Home, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, home)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buf.area.height {
            let mut line = String::new();
            for x in 0..buf.area.width {
                line.push_str(buf[(x, y)].symbol());
            }
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }

    /// Compare with `src/tui/home/snapshots/<name>.txt`. Set
    /// `DOJO_UPDATE_SNAPSHOTS=1` to write the files.
    fn assert_snapshot(name: &str, actual: &str) {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/tui/home/snapshots")
            .join(format!("{name}.txt"));
        if std::env::var_os("DOJO_UPDATE_SNAPSHOTS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, actual).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!(
                "missing snapshot {}; run with DOJO_UPDATE_SNAPSHOTS=1\n{actual}",
                path.display()
            )
        });
        assert_eq!(
            expected, actual,
            "snapshot {name} differs; run with DOJO_UPDATE_SNAPSHOTS=1 to update"
        );
    }

    #[test]
    fn snapshot_branches_view() {
        for width in [80, 50] {
            let mut home = Home::new(sample_data());
            home.handle_key(code(KeyCode::Tab));
            let screen = render(&mut home, width, 16);
            assert_snapshot(&format!("branches_{width}"), &screen);
        }
    }

    #[test]
    fn snapshot_log_view() {
        for width in [80, 50] {
            let mut home = Home::new(sample_data());
            home.handle_key(key('j'));
            let screen = render(&mut home, width, 16);
            assert_snapshot(&format!("log_{width}"), &screen);
        }
    }

    #[test]
    fn snapshot_change_detail() {
        for width in [80, 50] {
            let mut home = Home::new(sample_data());
            home.open(Screen::Detail(Box::new(sample_detail())));
            let screen = render(&mut home, width, 24);
            assert_snapshot(&format!("detail_{width}"), &screen);
        }
        // Wide: the file list is left of the diff; the second hunk.
        let mut home = Home::new(sample_data());
        let mut d = sample_detail();
        d.handle_key(KeyCode::Tab);
        home.open(Screen::Detail(Box::new(d)));
        render(&mut home, 120, 24);
        home.handle_key(key(']'));
        assert_snapshot("detail_120", &render(&mut home, 120, 24));
    }

    #[test]
    fn snapshot_bookmark_view() {
        for width in [80, 50] {
            let mut home = Home::new(sample_data());
            let view = sample_bookmark_view(&home);
            home.open(Screen::Bookmark(Box::new(view)));
            let screen = render(&mut home, width, 18);
            assert_snapshot(&format!("bookmark_{width}"), &screen);
        }
    }

    #[test]
    fn snapshot_preview_and_outcome() {
        let mut home = Home::new(sample_data());
        home.modal = Some(Modal::Preview(Preview {
            action: HomeAction::Sync("fix/goals".into()),
            title: "Sync fix/goals with the forge".into(),
            commands: vec![
                (
                    "jj git fetch --remote origin --remote sundae".into(),
                    "get new commits from the remotes".into(),
                ),
                (
                    "jj rebase --branch fix/goals --destination fix/goals@origin".into(),
                    "put your commits on top of the forge's commits".into(),
                ),
            ],
            dojo: Some("dojo sync fix/goals".into()),
            note: Some("dojo can run more commands after the fetch.".into()),
            runnable: true,
        }));
        assert_snapshot("preview_80", &render(&mut home, 80, 20));
        home.modal = Some(Modal::Outcome {
            outcome: Outcome {
                title: "Push fix/goals".into(),
                lines: vec![
                    "$ jj git push --remote sundae --bookmark fix/goals".into(),
                    "  remote: sundae: forge fix/goals has commits you lack".into(),
                    "The forge has new commits on fix/goals that you do not have.".into(),
                ],
                ok: false,
                offer: Some(crate::flow::Offer {
                    label: "Sync fix/goals with the forge, then push".into(),
                    dojo: "dojo sync fix/goals".into(),
                    commands: vec![
                        "jj rebase --branch fix/goals --destination fix/goals@origin".into(),
                    ],
                    action: crate::flow::OfferAction::Sync {
                        bookmark: "fix/goals".into(),
                    },
                }),
            },
            scroll: 0,
        });
        assert_snapshot("outcome_50", &render(&mut home, 50, 20));
    }

    #[test]
    fn wide_layout_shows_both_panes() {
        let mut home = Home::new(sample_data());
        let screen = render(&mut home, 120, 12);
        assert!(screen.contains("Bookmarks"), "{screen}");
        assert!(screen.contains("Log"), "{screen}");
        assert!(screen.contains("fix goals"), "{screen}");
    }
}
