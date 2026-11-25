//! `dojo branches`: one row per bookmark with its local and remote state.

use anyhow::Result;
use serde::Serialize;

use super::Ctx;
use crate::jujutsu::bookmarks::{BookmarkRow, RemoteCell};

#[derive(Debug, Clone, Serialize)]
pub struct BranchesReport {
    /// Remote columns, push remote first.
    pub remotes: Vec<String>,
    pub push_remote: String,
    pub rows: Vec<BookmarkRow>,
}

pub fn branches(ctx: &Ctx) -> Result<BranchesReport> {
    Ok(BranchesReport {
        remotes: ctx.remotes.display_order(),
        push_remote: ctx.remotes.push.clone(),
        rows: ctx.bookmark_rows()?,
    })
}

/// Ahead/behind text of the local bookmark vs one remote:
/// `=` same, `+2` local ahead, `-1` local behind, `+2-1` diverged,
/// `?` unknown, empty when one side is missing.
pub fn ahead_behind(cell: Option<&RemoteCell>, has_local: bool) -> String {
    let Some(cell) = cell else {
        return String::new();
    };
    if !has_local {
        return String::new();
    }
    match (cell.local_ahead, cell.local_behind) {
        (Some(0), Some(0)) => "=".to_string(),
        (Some(a), Some(0)) => format!("+{a}"),
        (Some(0), Some(b)) => format!("-{b}"),
        (Some(a), Some(b)) => format!("+{a}-{b}"),
        _ => "?".to_string(),
    }
}

/// Plain-text table for the CLI.
pub fn render_table(report: &BranchesReport) -> String {
    let mut header = vec!["BOOKMARK".to_string(), "LOCAL".to_string()];
    for r in &report.remotes {
        header.push(format!("@{r}"));
    }
    header.push("TRACKED".to_string());
    for r in &report.remotes {
        header.push(format!("±{r}"));
    }
    header.push("NOTES".to_string());

    let mut table: Vec<Vec<String>> = vec![header];
    for row in &report.rows {
        let mut line = vec![row.name.clone()];
        line.push(match &row.local {
            Some(l) if l.conflict => "CONFLICT".to_string(),
            Some(l) => short_id(l.change_id.as_deref()),
            None => "-".to_string(),
        });
        for r in &report.remotes {
            line.push(
                row.remote(r)
                    .map_or("-".to_string(), |c| short_id(c.change_id.as_deref())),
            );
        }
        let tracked = row.tracked_remotes();
        line.push(if tracked.is_empty() {
            "no".to_string()
        } else {
            tracked.join(",")
        });
        for r in &report.remotes {
            line.push(ahead_behind(row.remote(r), row.local.is_some()));
        }
        let mut notes = Vec::new();
        if row.protected {
            notes.push("protected");
        }
        if row.conflicted() {
            notes.push("conflicted");
        }
        if row.local.is_none() {
            notes.push("remote only");
        }
        line.push(notes.join(", "));
        table.push(line);
    }

    let cols = table[0].len();
    let widths: Vec<usize> = (0..cols)
        .map(|c| {
            table
                .iter()
                .map(|r| unicode_width::UnicodeWidthStr::width(r[c].as_str()))
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut out = String::new();
    for row in &table {
        let mut line = String::new();
        for (c, cell) in row.iter().enumerate() {
            line.push_str(cell);
            if c + 1 < cols {
                let w = unicode_width::UnicodeWidthStr::width(cell.as_str());
                line.push_str(&" ".repeat(widths[c] - w + 2));
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    if report.rows.is_empty() {
        out.push_str("(no bookmarks; run `dojo fetch`)\n");
    }
    out
}

fn short_id(id: Option<&str>) -> String {
    id.map_or("-".to_string(), |s| s[..s.len().min(8)].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(a: Option<u64>, b: Option<u64>) -> RemoteCell {
        RemoteCell {
            remote: "origin".into(),
            commit_id: None,
            change_id: None,
            tracked: true,
            local_ahead: a,
            local_behind: b,
        }
    }

    #[test]
    fn ahead_behind_text() {
        assert_eq!(ahead_behind(Some(&cell(Some(0), Some(0))), true), "=");
        assert_eq!(ahead_behind(Some(&cell(Some(2), Some(0))), true), "+2");
        assert_eq!(ahead_behind(Some(&cell(Some(0), Some(1))), true), "-1");
        assert_eq!(ahead_behind(Some(&cell(Some(2), Some(1))), true), "+2-1");
        assert_eq!(ahead_behind(Some(&cell(None, None)), true), "?");
        assert_eq!(ahead_behind(None, true), "");
        assert_eq!(ahead_behind(Some(&cell(Some(0), Some(0))), false), "");
    }
}
