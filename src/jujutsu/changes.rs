//! Change (revision) details for `dojo status` and the TUI change view.

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;

use super::{DELIM_FIELD, DELIM_LIST, records, run_jj_query};

/// One file touched by a change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangedFile {
    /// jj status word: `added`, `modified`, `removed`, `copied`, `renamed`.
    pub status: String,
    pub path: String,
    /// Source path for copies and renames.
    pub source: Option<String>,
}

/// Facts about one change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangeInfo {
    pub change_id: String,
    /// Shortest unique change id prefix (at least 8 characters).
    pub short_id: String,
    pub commit_id: String,
    pub description: String,
    pub author: String,
    pub timestamp: String,
    /// No file changes.
    pub empty: bool,
    pub conflict: bool,
    pub immutable: bool,
    pub working_copy: bool,
    pub local_bookmarks: Vec<String>,
    /// `name@remote` (without `@git`).
    pub remote_bookmarks: Vec<String>,
    pub parents: Vec<String>,
    pub files: Vec<ChangedFile>,
    pub lines_added: u64,
    pub lines_removed: u64,
}

impl ChangeInfo {
    pub fn title(&self) -> &str {
        self.description.lines().next().unwrap_or("")
    }

    pub fn has_description(&self) -> bool {
        !self.description.trim().is_empty()
    }
}

/// Template for [`changes`]: 16 `\x1f` fields, `\x1e`-terminated. List
/// fields use `\x01`; a file entry is `status \x02 path \x02 source`.
/// The description is LAST so stray control bytes in it cannot shift the
/// other fields.
pub(crate) const CHANGE_TEMPLATE: &str = concat!(
    r#"change_id ++ "\x1f" ++ change_id.shortest(8) ++ "\x1f" ++ commit_id ++ "\x1f""#,
    r#" ++ author.name() ++ " <" ++ author.email() ++ ">" ++ "\x1f""#,
    r#" ++ author.timestamp().format("%Y-%m-%d %H:%M") ++ "\x1f""#,
    r#" ++ if(empty, "1", "0") ++ "\x1f" ++ if(conflict, "1", "0") ++ "\x1f""#,
    r#" ++ if(immutable, "1", "0") ++ "\x1f" ++ if(current_working_copy, "1", "0") ++ "\x1f""#,
    r#" ++ local_bookmarks.map(|b| b.name()).join("\x01") ++ "\x1f""#,
    r#" ++ remote_bookmarks.filter(|b| b.remote() != "git").map(|b| b.name() ++ "@" ++ b.remote()).join("\x01") ++ "\x1f""#,
    r#" ++ parents.map(|p| p.change_id()).join("\x01") ++ "\x1f""#,
    r#" ++ self.diff().files().map(|f| f.status() ++ "\x02" ++ f.path().display() ++ "\x02" ++ f.source().path().display()).join("\x01") ++ "\x1f""#,
    r#" ++ self.diff().stat().total_added() ++ "\x1f" ++ self.diff().stat().total_removed() ++ "\x1f""#,
    r#" ++ description ++ "\x1e""#,
);

const FILE_SEP: char = '\x02';

/// Details of every change in `revset` (in `jj log` order).
pub fn changes(repo: &Path, revset: &str) -> Result<Vec<ChangeInfo>> {
    let out = run_jj_query(
        repo,
        &[
            "log",
            "--no-graph",
            "--revisions",
            revset,
            "--template",
            CHANGE_TEMPLATE,
        ],
    )?;
    Ok(parse_changes(&out))
}

/// Details of exactly one change.
pub fn change(repo: &Path, rev: &str) -> Result<ChangeInfo> {
    let mut all = changes(repo, rev)?;
    match all.len() {
        1 => Ok(all.remove(0)),
        0 => anyhow::bail!("revision {rev} not found"),
        n => anyhow::bail!("revision {rev} resolves to {n} changes; expected one"),
    }
}

pub(crate) fn parse_changes(out: &str) -> Vec<ChangeInfo> {
    // Records are split on \x1e; the description may contain newlines but
    // jj never emits \x1e from a description in practice. Trailing newline
    // trimming in `records` is fine: descriptions are re-trimmed below.
    records(out)
        .filter_map(|rec| {
            let f: Vec<&str> = rec.splitn(16, DELIM_FIELD).collect();
            if f.len() != 16 {
                return None;
            }
            let list = |s: &str| -> Vec<String> {
                s.split(DELIM_LIST)
                    .filter(|x| !x.is_empty())
                    .map(str::to_string)
                    .collect()
            };
            let files = f[12]
                .split(DELIM_LIST)
                .filter(|x| !x.is_empty())
                .filter_map(|entry| {
                    let mut parts = entry.splitn(3, FILE_SEP);
                    let status = parts.next()?.to_string();
                    let path = parts.next()?.to_string();
                    let source = parts
                        .next()
                        .filter(|s| !s.is_empty() && *s != path)
                        .map(str::to_string);
                    Some(ChangedFile {
                        status,
                        path,
                        source,
                    })
                })
                .collect();
            Some(ChangeInfo {
                change_id: f[0].to_string(),
                short_id: f[1].to_string(),
                commit_id: f[2].to_string(),
                author: f[3].to_string(),
                timestamp: f[4].to_string(),
                empty: f[5] == "1",
                conflict: f[6] == "1",
                immutable: f[7] == "1",
                working_copy: f[8] == "1",
                local_bookmarks: list(f[9]),
                remote_bookmarks: list(f[10]),
                parents: list(f[11]),
                files,
                lines_added: f[13].parse().unwrap_or(0),
                lines_removed: f[14].parse().unwrap_or(0),
                description: f[15].trim_end().to_string(),
            })
        })
        .collect()
}

/// Full diff of a change, with ANSI colours (`jj diff --git --color=always`),
/// for display through `ansi-to-tui`. Not parsed.
pub fn diff_colored(repo: &Path, rev: &str) -> Result<String> {
    super::run_jj(
        repo,
        &[
            "--color=always",
            "--no-pager",
            "diff",
            "--git",
            "--revision",
            rev,
        ],
    )
    .context("jj diff")
}

/// Plain `jj diff --git` text (for `--json` and tests).
pub fn diff_plain(repo: &Path, rev: &str) -> Result<String> {
    run_jj_query(repo, &["diff", "--git", "--revision", rev]).context("jj diff")
}

/// Whether the revset matches any commit.
pub fn exists(repo: &Path, revset: &str) -> bool {
    count(repo, revset).is_ok_and(|n| n > 0)
}

/// Number of commits in `revset`.
pub fn count(repo: &Path, revset: &str) -> Result<usize> {
    let out = run_jj_query(
        repo,
        &[
            "log",
            "--no-graph",
            "--revisions",
            revset,
            "--template",
            r#"commit_id.short() ++ "\x1e""#,
        ],
    )?;
    Ok(records(&out).count())
}

/// `jj config get <key>`; `None` when the key is unset.
pub fn config_get(repo: &Path, key: &str) -> Option<String> {
    run_jj_query(repo, &["config", "get", key])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_shape() {
        assert_eq!(CHANGE_TEMPLATE.matches(r#""\x1f""#).count(), 15);
        assert!(CHANGE_TEMPLATE.ends_with(r#"description ++ "\x1e""#));
    }

    #[test]
    fn parses_a_change() {
        let fields = [
            "abcdefghijklmnopqrstuvwxyzabcdef",
            "abcdefgh",
            "0123",
            "A <a@x>",
            "2026-01-01 10:00",
            "0",
            "1",
            "0",
            "1",
            "fix/goals",
            "fix/goals@sundae\x01fix/goals@origin",
            "p1",
            "modified\x02goals.txt\x02goals.txt\x01renamed\x02new.txt\x02old.txt",
            "3",
            "1",
            "fix goals\n\nbody line\n",
        ];
        let out = format!("{}\x1e", fields.join("\x1f"));
        let c = &parse_changes(&out)[0];
        assert_eq!(c.short_id, "abcdefgh");
        assert!(c.conflict && c.working_copy && !c.empty);
        assert_eq!(c.remote_bookmarks.len(), 2);
        assert_eq!(c.files.len(), 2);
        assert_eq!(c.files[0].source, None);
        assert_eq!(c.files[1].source.as_deref(), Some("old.txt"));
        assert_eq!(c.title(), "fix goals");
        assert_eq!(c.description, "fix goals\n\nbody line");
        assert_eq!((c.lines_added, c.lines_removed), (3, 1));
    }
}
