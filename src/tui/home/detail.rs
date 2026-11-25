//! Change detail: a header (ids, author, bookmarks, description), the list
//! of changed files, and the diff of the selected file with line numbers,
//! coloured added/removed lines, and the changed words in a stronger colour.

use crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::{
    Block, BorderType, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use unicode_width::UnicodeWidthStr;

use super::view::{border_style, dim, key_style};
use crate::jujutsu::changes::ChangeInfo;
use crate::jujutsu::diff::{DiffLine, FileDiff, LineKind};
use crate::text_utils::{truncate_end, truncate_start};

/// From this width on, the file list is left of the diff.
const WIDE: u16 = 100;
const TAB: &str = "    ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Focus {
    Files,
    Diff,
}

pub(crate) struct Detail {
    pub info: ChangeInfo,
    pub files: Vec<FileDiff>,
    pub file_sel: usize,
    pub focus: Focus,
    /// First visible row of the diff.
    pub scroll: usize,
    /// Columns scrolled to the right in the diff.
    pub hscroll: usize,
    /// Visible diff rows (set by `draw`).
    pub page: usize,
    /// Show the whole description (the header can take most of the screen).
    pub expanded: bool,
}

/// One row of the diff pane.
enum Row<'a> {
    Hunk {
        old: u32,
        new: u32,
        context: &'a str,
    },
    Line(&'a DiffLine),
    Note(String),
    Blank,
}

fn rows(file: &FileDiff) -> Vec<Row<'_>> {
    let mut rows = Vec::new();
    if file.binary {
        rows.push(Row::Note(
            "Binary file: dojo does not show its content.".into(),
        ));
    } else if file.hunks.is_empty() {
        rows.push(Row::Note(match file.status {
            'R' => "Renamed. The content did not change.".into(),
            'C' => "Copied. The content did not change.".into(),
            'A' => "New empty file.".into(),
            'D' => "Deleted an empty file.".into(),
            _ => "Only the file mode changed.".into(),
        }));
    }
    for (i, h) in file.hunks.iter().enumerate() {
        if i > 0 {
            rows.push(Row::Blank);
        }
        rows.push(Row::Hunk {
            old: h.old_start,
            new: h.new_start,
            context: &h.context,
        });
        rows.extend(h.lines.iter().map(Row::Line));
    }
    rows
}

fn hunk_offsets(file: &FileDiff) -> Vec<usize> {
    rows(file)
        .iter()
        .enumerate()
        .filter_map(|(i, r)| matches!(r, Row::Hunk { .. }).then_some(i))
        .collect()
}

impl Detail {
    pub(crate) fn new(info: ChangeInfo, files: Vec<FileDiff>) -> Self {
        let focus = if files.len() > 1 {
            Focus::Files
        } else {
            Focus::Diff
        };
        Self {
            info,
            files,
            file_sel: 0,
            focus,
            scroll: 0,
            hscroll: 0,
            page: 0,
            expanded: false,
        }
    }

    pub(crate) fn selected_file(&self) -> Option<&FileDiff> {
        self.files.get(self.file_sel)
    }

    /// Select the file with this path (after a reload).
    pub(crate) fn select_path(&mut self, path: &str) {
        if let Some(i) = self.files.iter().position(|f| f.path == path) {
            self.file_sel = i;
        }
    }

    fn row_count(&self) -> usize {
        self.selected_file().map_or(0, |f| rows(f).len())
    }

    fn max_scroll(&self) -> usize {
        self.row_count().saturating_sub(self.page.max(1))
    }

    fn scroll_by(&mut self, delta: isize) {
        self.scroll = self
            .scroll
            .saturating_add_signed(delta)
            .min(self.max_scroll());
    }

    fn select_file(&mut self, index: usize) {
        if index < self.files.len() && index != self.file_sel {
            self.file_sel = index;
            self.scroll = 0;
            self.hscroll = 0;
        }
    }

    fn jump_hunk(&mut self, forward: bool) {
        let Some(file) = self.selected_file() else {
            return;
        };
        let offsets = hunk_offsets(file);
        let target = if forward {
            offsets.into_iter().find(|&o| o > self.scroll)
        } else {
            offsets.into_iter().rev().find(|&o| o < self.scroll)
        };
        if let Some(t) = target {
            self.scroll = t.min(self.max_scroll());
        }
    }

    /// Handle a key. Returns false when the key is not a detail key.
    pub(crate) fn handle_key(&mut self, code: KeyCode) -> bool {
        let page = isize::try_from(self.page.max(2)).unwrap_or(2);
        let next_file = self.file_sel + 1;
        let prev_file = self.file_sel.saturating_sub(1);
        match (code, self.focus) {
            (KeyCode::Tab | KeyCode::BackTab, _) => {
                self.focus = match self.focus {
                    Focus::Files => Focus::Diff,
                    Focus::Diff => Focus::Files,
                };
            }
            (KeyCode::Enter | KeyCode::Right, Focus::Files) => self.focus = Focus::Diff,
            (KeyCode::Down | KeyCode::Char('j'), Focus::Files) => self.select_file(next_file),
            (KeyCode::Up | KeyCode::Char('k'), Focus::Files) => self.select_file(prev_file),
            (KeyCode::Down | KeyCode::Char('j'), Focus::Diff) => self.scroll_by(1),
            (KeyCode::Up | KeyCode::Char('k'), Focus::Diff) => self.scroll_by(-1),
            (KeyCode::Right, Focus::Diff) => self.hscroll += 4,
            (KeyCode::Left, Focus::Diff) => {
                if self.hscroll == 0 {
                    self.focus = Focus::Files;
                }
                self.hscroll = self.hscroll.saturating_sub(4);
            }
            (KeyCode::Char('J'), _) => self.select_file(next_file),
            (KeyCode::Char('K'), _) => self.select_file(prev_file),
            (KeyCode::Char(']'), _) => self.jump_hunk(true),
            (KeyCode::Char('['), _) => self.jump_hunk(false),
            (KeyCode::PageDown | KeyCode::Char(' '), _) => self.scroll_by(page - 1),
            (KeyCode::PageUp, _) => self.scroll_by(1 - page),
            (KeyCode::Char('v'), _) => self.expanded = !self.expanded,
            (KeyCode::Char('g') | KeyCode::Home, _) => self.scroll = 0,
            (KeyCode::Char('G') | KeyCode::End, _) => self.scroll = self.max_scroll(),
            _ => return false,
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

fn header_lines(c: &ChangeInfo) -> Vec<Line<'static>> {
    let mut desc = c.description.trim_end().lines();
    let title = desc.next().filter(|t| !t.trim().is_empty());
    let mut lines = vec![match title {
        Some(t) => Line::styled(t.to_string(), Style::default().bold()),
        None => Line::styled("(no description)", dim()),
    }];
    let mut meta = vec![
        Span::styled(
            c.change_id[..c.change_id.len().min(12)].to_string(),
            Style::default().fg(Color::Magenta).bold(),
        ),
        Span::styled("  commit ", dim()),
        Span::styled(
            c.commit_id[..c.commit_id.len().min(12)].to_string(),
            Style::default().fg(Color::Blue),
        ),
        Span::styled("  ", dim()),
        Span::raw(c.author.clone()),
        Span::styled(format!("  {}", c.timestamp), dim()),
    ];
    let mut tags: Vec<&str> = Vec::new();
    if c.working_copy {
        tags.push("working copy");
    }
    if c.empty {
        tags.push("empty");
    }
    if c.conflict {
        tags.push("conflict");
    }
    if c.immutable {
        tags.push("immutable");
    }
    if !tags.is_empty() {
        meta.push(Span::styled(
            format!("  [{}]", tags.join(", ")),
            Style::default().fg(Color::Yellow),
        ));
    }
    lines.push(Line::from(meta));
    let mut marks: Vec<String> = c.local_bookmarks.clone();
    marks.extend(c.remote_bookmarks.iter().cloned());
    if !marks.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("bookmarks ", dim()),
            Span::styled(marks.join("  "), Style::default().fg(Color::Green)),
        ]));
    }
    let body: Vec<&str> = desc.collect();
    let body = body
        .iter()
        .position(|l| !l.trim().is_empty())
        .map_or(&body[..0], |i| &body[i..]);
    if !body.is_empty() {
        lines.push(Line::raw(""));
        lines.extend(body.iter().map(|l| Line::raw((*l).to_string())));
    }
    lines
}

fn status_style(status: char) -> Style {
    Style::default().fg(match status {
        'A' => Color::Green,
        'D' => Color::Red,
        'R' | 'C' => Color::Cyan,
        _ => Color::Yellow,
    })
}

fn file_label(f: &FileDiff) -> String {
    match &f.old_path {
        Some(old) => format!("{old} → {}", f.path),
        None => f.path.clone(),
    }
}

fn draw_files(frame: &mut Frame, area: Rect, d: &Detail) {
    let (added, removed) = d
        .files
        .iter()
        .fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));
    let title = Line::from(vec![
        Span::raw(format!(" Files ({}) ", d.files.len())),
        Span::styled(format!("+{added}"), Style::default().fg(Color::Green)),
        Span::raw(" "),
        Span::styled(format!("-{removed} "), Style::default().fg(Color::Red)),
    ]);
    let block = Block::bordered()
        .title(title)
        .border_type(BorderType::Rounded)
        .border_style(border_style(d.focus == Focus::Files));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if d.files.is_empty() {
        frame.render_widget(Paragraph::new("No changed files.").style(dim()), inner);
        return;
    }
    let height = inner.height as usize;
    let first = d.file_sel.saturating_sub(height.saturating_sub(1));
    let width = inner.width as usize;
    let lines: Vec<Line> = d
        .files
        .iter()
        .enumerate()
        .skip(first)
        .take(height)
        .map(|(i, f)| {
            let selected = i == d.file_sel;
            let counts = format!(" +{} -{}", f.added, f.removed);
            let path_w = width.saturating_sub(4 + counts.len());
            let path = truncate_start(&file_label(f), path_w);
            let pad = path_w.saturating_sub(UnicodeWidthStr::width(path.as_str()));
            let mut line = Line::from(vec![
                Span::styled(if selected { "▶" } else { " " }, key_style()),
                Span::styled(format!("{} ", f.status), status_style(f.status)),
                Span::raw(format!(" {path}{}", " ".repeat(pad))),
                Span::styled(format!(" +{}", f.added), Style::default().fg(Color::Green)),
                Span::styled(format!(" -{}", f.removed), Style::default().fg(Color::Red)),
            ]);
            if selected {
                line = line.style(Style::default().bg(Color::Rgb(40, 44, 52)));
            }
            line
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

const ADD_BG: Color = Color::Rgb(22, 52, 32);
const ADD_EMPH: Color = Color::Rgb(38, 110, 58);
const DEL_BG: Color = Color::Rgb(62, 26, 30);
const DEL_EMPH: Color = Color::Rgb(135, 40, 50);

fn expand(s: &str) -> String {
    s.replace('\t', TAB)
}

fn content_line(row: &Row<'_>) -> Line<'static> {
    match row {
        Row::Blank => Line::raw(""),
        Row::Note(text) => Line::styled(text.clone(), dim().italic()),
        Row::Hunk { old, new, context } => Line::from(vec![
            Span::styled(
                format!("@@ -{old} +{new} @@"),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(format!(" {}", expand(context)), dim()),
        ]),
        Row::Line(l) => {
            let (sign, fg, bg, emph) = match l.kind {
                LineKind::Added => ("+", Color::Green, ADD_BG, ADD_EMPH),
                LineKind::Removed => ("-", Color::Red, DEL_BG, DEL_EMPH),
                LineKind::NoNewline => {
                    return Line::styled("\\ No newline at end of file", dim().italic());
                }
                LineKind::Context => (" ", Color::Reset, Color::Reset, Color::Reset),
            };
            let base = if l.kind == LineKind::Context {
                Style::default()
            } else {
                Style::default().fg(fg).bg(bg)
            };
            let mut spans = vec![Span::styled(sign.to_string(), base.bold())];
            match l.emph {
                Some((s, e)) if s <= e && e <= l.text.len() => {
                    spans.push(Span::styled(expand(&l.text[..s]), base));
                    spans.push(Span::styled(
                        expand(&l.text[s..e]),
                        base.bg(emph).fg(Color::White).bold(),
                    ));
                    spans.push(Span::styled(expand(&l.text[e..]), base));
                }
                _ => spans.push(Span::styled(expand(&l.text), base)),
            }
            Line::from(spans).style(base)
        }
    }
}

fn gutter_line(row: &Row<'_>, digits: usize) -> Line<'static> {
    let num = |n: Option<u32>| n.map_or(" ".repeat(digits), |n| format!("{n:>digits$}"));
    match row {
        Row::Line(l) if l.kind != LineKind::NoNewline => {
            let style = match l.kind {
                LineKind::Added => Style::default().fg(Color::Green),
                LineKind::Removed => Style::default().fg(Color::Red),
                _ => dim(),
            };
            Line::styled(format!("{} {} │", num(l.old_no), num(l.new_no)), style)
        }
        Row::Hunk { .. } => Line::styled(format!("{:>w$} │", "⋯", w = digits * 2 + 1), dim()),
        _ => Line::styled(format!("{} │", " ".repeat(digits * 2 + 1)), dim()),
    }
}

fn draw_diff(frame: &mut Frame, area: Rect, d: &mut Detail) {
    let Some(file) = d.files.get(d.file_sel) else {
        let block = Block::bordered()
            .title(" Diff ")
            .border_type(BorderType::Rounded)
            .border_style(border_style(false));
        frame.render_widget(
            Paragraph::new("This change has no file changes.")
                .style(dim())
                .block(block),
            area,
        );
        return;
    };
    let title = Line::from(vec![
        Span::styled(
            format!(" {} ", file.status),
            status_style(file.status).bold(),
        ),
        Span::raw(truncate_start(
            &file_label(file),
            area.width.saturating_sub(20) as usize,
        )),
        Span::styled(
            format!("  +{}", file.added),
            Style::default().fg(Color::Green),
        ),
        Span::styled(
            format!(" -{} ", file.removed),
            Style::default().fg(Color::Red),
        ),
    ]);
    let block = Block::bordered()
        .title(title)
        .border_type(BorderType::Rounded)
        .border_style(border_style(d.focus == Focus::Diff));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = rows(file);
    d.page = inner.height as usize;
    d.scroll = d.scroll.min(rows.len().saturating_sub(d.page.max(1)));
    let max_no = file
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .filter_map(|l| l.old_no.max(l.new_no))
        .max()
        .unwrap_or(1);
    let digits = max_no.to_string().len().max(2);
    let gutter_w = u16::try_from(digits * 2 + 3)
        .unwrap_or(9)
        .min(inner.width / 2);
    let [gutter, content] =
        Layout::horizontal([Constraint::Length(gutter_w), Constraint::Min(1)]).areas(inner);
    let visible = rows.iter().skip(d.scroll).take(d.page);
    let (g, c): (Vec<_>, Vec<_>) = visible
        .map(|r| (gutter_line(r, digits), content_line(r)))
        .unzip();
    frame.render_widget(Paragraph::new(g), gutter);
    let hscroll = u16::try_from(d.hscroll).unwrap_or(u16::MAX);
    frame.render_widget(Paragraph::new(c).scroll((0, hscroll)), content);
    if rows.len() > d.page {
        let mut state = ScrollbarState::default()
            .content_length(rows.len().saturating_sub(d.page))
            .position(d.scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut state,
        );
    }
}

pub(crate) fn draw(frame: &mut Frame, area: Rect, d: &mut Detail) {
    let mut header = header_lines(&d.info);
    let limit = if d.expanded {
        area.height * 2 / 3
    } else {
        (area.height / 4).max(5)
    };
    let header_h = u16::try_from(header.len() + 2)
        .unwrap_or(u16::MAX)
        .min(limit)
        .max(3.min(area.height));
    let room = header_h.saturating_sub(2) as usize;
    if header.len() > room && room > 0 {
        let hidden = header.len() - (room - 1);
        header.truncate(room - 1);
        header.push(Line::styled(
            format!("… {hidden} more lines (v: show all)"),
            dim().italic(),
        ));
    }
    let [top, body] =
        Layout::vertical([Constraint::Length(header_h), Constraint::Min(3)]).areas(area);
    let title = if d.expanded {
        format!(" Change {} · v: less ", d.info.short_id)
    } else {
        format!(" Change {} ", d.info.short_id)
    };
    let block = Block::bordered()
        .title(title)
        .border_type(BorderType::Rounded)
        .border_style(border_style(false));
    let width = top.width.saturating_sub(2) as usize;
    let header: Vec<Line> = header
        .into_iter()
        .map(|l| {
            if l.width() > width {
                let text: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
                Line::styled(
                    truncate_end(&text, width),
                    l.spans.first().map_or(Style::default(), |s| s.style),
                )
            } else {
                l
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(header).block(block), top);

    if body.width >= WIDE {
        let longest = d
            .files
            .iter()
            .map(|f| UnicodeWidthStr::width(file_label(f).as_str()))
            .max()
            .unwrap_or(10);
        let files_w = u16::try_from(longest + 16)
            .unwrap_or(u16::MAX)
            .clamp(28, body.width * 2 / 5);
        let [files, diff] =
            Layout::horizontal([Constraint::Length(files_w), Constraint::Min(20)]).areas(body);
        draw_files(frame, files, d);
        draw_diff(frame, diff, d);
    } else {
        let files_h = u16::try_from(d.files.len() + 2)
            .unwrap_or(u16::MAX)
            .clamp(3, 7);
        let [files, diff] =
            Layout::vertical([Constraint::Length(files_h), Constraint::Min(3)]).areas(body);
        draw_files(frame, files, d);
        draw_diff(frame, diff, d);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::jujutsu::diff;

    pub(crate) fn sample_info() -> ChangeInfo {
        ChangeInfo {
            change_id: "qsspzlznyywrkmnvmqulnoyvqtzswotq".into(),
            short_id: "qsspzlzn".into(),
            commit_id: "72fc598a61f3aa00".into(),
            description: "fix goals\n\nShip dojo this week.\n".into(),
            author: "Forge <forge@example.com>".into(),
            timestamp: "2026-11-25 14:00".into(),
            empty: false,
            conflict: false,
            immutable: false,
            working_copy: false,
            local_bookmarks: vec!["fix/goals".into()],
            remote_bookmarks: vec!["fix/goals@origin".into()],
            parents: vec!["npppruztlspuknmrxvnszvyvlqtuuzrs".into()],
            files: Vec::new(),
            lines_added: 3,
            lines_removed: 2,
        }
    }

    pub(crate) const SAMPLE_DIFF: &str = "\
diff --git a/goals.txt b/goals.txt
index 1111111..2222222 100644
--- a/goals.txt
+++ b/goals.txt
@@ -1,3 +1,3 @@ goals
 # Goals
-goals: none
+goals: ship dojo
 owner: team
@@ -20,2 +20,3 @@ notes
 keep
+added line
 end
diff --git a/src/new.rs b/src/new.rs
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/src/new.rs
@@ -0,0 +1,1 @@
+fn main() {}
diff --git a/old.txt b/old.txt
deleted file mode 100644
index 4444444..0000000
--- a/old.txt
+++ /dev/null
@@ -1,1 +0,0 @@
-gone
";

    pub(crate) fn sample_detail() -> Detail {
        Detail::new(sample_info(), diff::parse(SAMPLE_DIFF))
    }

    #[test]
    fn keys_select_files_and_jump_between_hunks() {
        let mut d = sample_detail();
        d.page = 3;
        assert_eq!(d.focus, Focus::Files);
        assert!(d.handle_key(KeyCode::Char('j')));
        assert_eq!(d.selected_file().unwrap().path, "src/new.rs");
        assert!(d.handle_key(KeyCode::Char('K')));
        assert_eq!(d.file_sel, 0);
        // goals.txt rows: hunk(0) 4 lines(1-4) blank(5) hunk(6) 3 lines(7-9)
        assert_eq!(hunk_offsets(&d.files[0]), vec![0, 6]);
        d.handle_key(KeyCode::Char(']'));
        assert_eq!(d.scroll, 6);
        d.handle_key(KeyCode::Char('['));
        assert_eq!(d.scroll, 0);
        d.handle_key(KeyCode::Tab);
        assert_eq!(d.focus, Focus::Diff);
        d.handle_key(KeyCode::Char('j'));
        assert_eq!(d.scroll, 1);
        d.handle_key(KeyCode::Right);
        assert_eq!(d.hscroll, 4);
        assert!(!d.handle_key(KeyCode::Char('d')));
    }
}
