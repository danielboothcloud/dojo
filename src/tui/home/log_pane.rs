//! Log pane: the `jj log` graph (with jj's colours) and one selected change.
//!
//! The graph text and the per-line change ids come from
//! [`crate::jujutsu::log_graph_with_heads`]. Each line of a change's block
//! carries the change id, so a change starts where the id changes.

use ansi_to_tui::IntoText;
use ratatui::prelude::*;
use ratatui::widgets::{
    Block, BorderType, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

pub(crate) struct LogPane {
    text: Text<'static>,
    line_heads: Vec<Option<String>>,
    /// First line of each change, in graph order.
    starts: Vec<usize>,
    /// Index into `starts`.
    selected: usize,
    /// First visible line.
    scroll: usize,
}

impl LogPane {
    pub(crate) fn new(ansi: &str, line_heads: Vec<Option<String>>) -> Self {
        let text = ansi
            .into_text()
            .unwrap_or_else(|_| Text::from("failed to read the jj log output"));
        let mut starts = Vec::new();
        let mut prev: Option<&str> = None;
        for (i, head) in line_heads.iter().enumerate() {
            match head.as_deref() {
                Some(id) if prev != Some(id) => {
                    starts.push(i);
                    prev = Some(id);
                }
                Some(_) => {}
                None => prev = None,
            }
        }
        Self {
            text,
            line_heads,
            starts,
            selected: 0,
            scroll: 0,
        }
    }

    /// Change id of the selected change.
    pub(crate) fn selected_change(&self) -> Option<&str> {
        let line = *self.starts.get(self.selected)?;
        self.line_heads.get(line)?.as_deref()
    }

    /// Select the change with this id. Returns false when it is not shown.
    pub(crate) fn select_change(&mut self, change_id: &str) -> bool {
        let found = self
            .starts
            .iter()
            .position(|&l| self.line_heads[l].as_deref() == Some(change_id));
        if let Some(i) = found {
            self.selected = i;
        }
        found.is_some()
    }

    pub(crate) fn up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub(crate) fn down(&mut self) {
        if self.selected + 1 < self.starts.len() {
            self.selected += 1;
        }
    }

    pub(crate) fn top(&mut self) {
        self.selected = 0;
    }

    pub(crate) fn bottom(&mut self) {
        self.selected = self.starts.len().saturating_sub(1);
    }

    /// Line range `[start, end)` of the selected change.
    fn selected_lines(&self) -> Option<(usize, usize)> {
        let start = *self.starts.get(self.selected)?;
        let id = self.line_heads[start].as_deref();
        let len = self.line_heads[start..]
            .iter()
            .take_while(|h| h.as_deref() == id)
            .count();
        Some((start, start + len))
    }

    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect, focused: bool, title: &str) {
        let block = Block::bordered()
            .title(title.to_string())
            .border_type(BorderType::Rounded)
            .border_style(super::view::border_style(focused));
        let inner = block.inner(area);
        let height = inner.height as usize;

        // Keep the selected change visible.
        if let Some((start, end)) = self.selected_lines() {
            if start < self.scroll {
                self.scroll = start;
            } else if end > self.scroll + height && height > 0 {
                self.scroll = end.saturating_sub(height).min(start);
            }
        }
        let selected = self.selected_lines();
        let lines: Vec<Line<'static>> = self
            .text
            .lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let is_sel = selected.is_some_and(|(s, e)| i >= s && i < e);
                let marker = if selected.is_some_and(|(s, _)| s == i) {
                    Span::styled("▶", Style::default().fg(Color::Cyan).bold())
                } else {
                    Span::raw(" ")
                };
                let mut spans = vec![marker];
                spans.extend(line.spans.iter().cloned());
                let mut l = Line::from(spans);
                if is_sel && focused {
                    l = l.style(Style::default().bg(Color::Rgb(40, 44, 52)));
                }
                l
            })
            .collect();
        let total = lines.len();
        let paragraph = Paragraph::new(lines)
            .block(block)
            .scroll((u16::try_from(self.scroll).unwrap_or(u16::MAX), 0));
        frame.render_widget(paragraph, area);
        if total > height {
            let mut state = ScrollbarState::default()
                .content_length(total.saturating_sub(height))
                .position(self.scroll);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight),
                area,
                &mut state,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heads(ids: &[Option<&str>]) -> Vec<Option<String>> {
        ids.iter().map(|i| i.map(str::to_string)).collect()
    }

    #[test]
    fn changes_start_where_the_id_changes() {
        let mut pane = LogPane::new(
            "@ a\n│ a desc\n○ b\n│ b desc\n~\n",
            heads(&[Some("a"), Some("a"), Some("b"), Some("b"), None]),
        );
        assert_eq!(pane.selected_change(), Some("a"));
        pane.down();
        assert_eq!(pane.selected_change(), Some("b"));
        pane.down();
        assert_eq!(pane.selected_change(), Some("b"));
        assert_eq!(pane.selected_lines(), Some((2, 4)));
        assert!(pane.select_change("a"));
        assert!(!pane.select_change("zzz"));
        assert_eq!(pane.selected_change(), Some("a"));
    }
}
