//! Bookmark state across the local repo and every remote, read with a
//! record-framed `jj bookmark list --all-remotes` template.

use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

use super::{DELIM_FIELD, DELIM_LIST, records, run_jj_query};

/// One `jj bookmark list --all-remotes` entry (local or `name@remote`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RefState {
    pub name: String,
    /// `None` for the local bookmark.
    pub remote: Option<String>,
    pub present: bool,
    pub tracked: bool,
    pub conflict: bool,
    pub commit_id: Option<String>,
    pub change_id: Option<String>,
    /// Remote refs only, when tracked: commits on the remote that the local
    /// bookmark lacks (`tracking_ahead_count`).
    pub remote_ahead: Option<u64>,
    /// Remote refs only, when tracked: local commits the remote lacks.
    pub remote_behind: Option<u64>,
    /// Change ids of the conflicting targets (`added_targets`).
    pub added_change_ids: Vec<String>,
}

/// Template for [`list_refs`]. 10 fields, `\x1f`-separated, `\x1e`-terminated;
/// the last field is a `\x01`-separated list.
pub(crate) const REF_TEMPLATE: &str = concat!(
    r#"name ++ "\x1f" ++ remote ++ "\x1f" ++ if(present, "1", "0") ++ "\x1f""#,
    r#" ++ if(tracked, "1", "0") ++ "\x1f" ++ if(conflict, "1", "0") ++ "\x1f""#,
    r#" ++ if(normal_target, normal_target.commit_id(), "") ++ "\x1f""#,
    r#" ++ if(normal_target, normal_target.change_id(), "") ++ "\x1f""#,
    r#" ++ if(remote && tracked, tracking_ahead_count.lower(), "") ++ "\x1f""#,
    r#" ++ if(remote && tracked, tracking_behind_count.lower(), "") ++ "\x1f""#,
    r#" ++ added_targets.map(|c| c.change_id()).join("\x01") ++ "\x1e""#,
);

/// All local and remote bookmark refs, except the internal `@git` remote.
pub fn list_refs(repo: &Path) -> Result<Vec<RefState>> {
    let out = run_jj_query(
        repo,
        &[
            "bookmark",
            "list",
            "--all-remotes",
            "--template",
            REF_TEMPLATE,
        ],
    )?;
    Ok(parse_refs(&out))
}

pub(crate) fn parse_refs(out: &str) -> Vec<RefState> {
    records(out)
        .filter_map(|rec| {
            let f: Vec<&str> = rec.split(DELIM_FIELD).collect();
            if f.len() != 10 {
                return None;
            }
            let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());
            let num = |s: &str| s.parse::<u64>().ok();
            let remote = opt(f[1]);
            if remote.as_deref() == Some("git") {
                return None;
            }
            Some(RefState {
                name: f[0].to_string(),
                remote,
                present: f[2] == "1",
                tracked: f[3] == "1",
                conflict: f[4] == "1",
                commit_id: opt(f[5]),
                change_id: opt(f[6]),
                remote_ahead: num(f[7]),
                remote_behind: num(f[8]),
                added_change_ids: f[9]
                    .split(DELIM_LIST)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect(),
            })
        })
        .collect()
}

/// The local side of a bookmark row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalCell {
    pub commit_id: Option<String>,
    pub change_id: Option<String>,
    pub conflict: bool,
    /// Change ids of the conflicting targets when `conflict`.
    pub conflict_change_ids: Vec<String>,
}

/// One remote's view of a bookmark, relative to the local bookmark.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteCell {
    pub remote: String,
    pub commit_id: Option<String>,
    pub change_id: Option<String>,
    pub tracked: bool,
    /// Local commits that the remote lacks (`None` = unknown / no local).
    pub local_ahead: Option<u64>,
    /// Remote commits that the local bookmark lacks.
    pub local_behind: Option<u64>,
}

/// One row of `dojo branches`: a bookmark name and its state everywhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BookmarkRow {
    pub name: String,
    pub local: Option<LocalCell>,
    /// One cell per remote that has this bookmark.
    pub remotes: Vec<RemoteCell>,
    pub protected: bool,
}

impl BookmarkRow {
    pub fn remote(&self, name: &str) -> Option<&RemoteCell> {
        self.remotes.iter().find(|c| c.remote == name)
    }

    pub fn is_tracked_anywhere(&self) -> bool {
        self.remotes.iter().any(|c| c.tracked)
    }

    pub fn tracked_remotes(&self) -> Vec<&str> {
        self.remotes
            .iter()
            .filter(|c| c.tracked)
            .map(|c| c.remote.as_str())
            .collect()
    }

    pub fn conflicted(&self) -> bool {
        self.local.as_ref().is_some_and(|l| l.conflict)
    }
}

/// Group refs into one row per bookmark name (sorted by name). Ahead/behind
/// for tracked pairs comes from the template; untracked pairs with both
/// sides present are filled in by `count_between`.
pub fn rows(
    refs: &[RefState],
    protected: &[String],
    mut count_between: impl FnMut(&str, &str) -> Option<u64>,
) -> Vec<BookmarkRow> {
    let mut by_name: BTreeMap<&str, Vec<&RefState>> = BTreeMap::new();
    for r in refs {
        by_name.entry(r.name.as_str()).or_default().push(r);
    }
    by_name
        .into_iter()
        .map(|(name, entries)| {
            let local = entries
                .iter()
                .find(|r| r.remote.is_none() && r.present)
                .map(|r| LocalCell {
                    commit_id: r.commit_id.clone(),
                    change_id: r.change_id.clone(),
                    conflict: r.conflict,
                    conflict_change_ids: if r.conflict {
                        r.added_change_ids.clone()
                    } else {
                        Vec::new()
                    },
                });
            let local_commit = local.as_ref().and_then(|l| l.commit_id.clone());
            let remotes = entries
                .iter()
                .filter(|r| r.present)
                .filter_map(|r| {
                    let remote = r.remote.clone()?;
                    let (ahead, behind) = if r.tracked && local.is_some() {
                        (r.remote_behind, r.remote_ahead)
                    } else {
                        match (&local_commit, &r.commit_id) {
                            (Some(l), Some(c)) if l == c => (Some(0), Some(0)),
                            (Some(l), Some(c)) => (count_between(c, l), count_between(l, c)),
                            _ => (None, None),
                        }
                    };
                    Some(RemoteCell {
                        remote,
                        commit_id: r.commit_id.clone(),
                        change_id: r.change_id.clone(),
                        tracked: r.tracked,
                        local_ahead: ahead,
                        local_behind: behind,
                    })
                })
                .collect();
            BookmarkRow {
                name: name.to_string(),
                local,
                remotes,
                protected: protected.iter().any(|p| p == name),
            }
        })
        .collect()
}

/// Number of commits in `from..to` (reachable from `to`, not from `from`).
pub fn count_range(repo: &Path, from_commit: &str, to_commit: &str) -> Option<u64> {
    let revset = format!("{from_commit}..{to_commit}");
    let out = run_jj_query(
        repo,
        &[
            "log",
            "--no-graph",
            "--revisions",
            &revset,
            "--template",
            r#"commit_id.short() ++ "\x1e""#,
        ],
    )
    .ok()?;
    Some(records(&out).count() as u64)
}

/// All bookmark rows of the repo, with ahead/behind filled in.
pub fn bookmark_rows(repo: &Path, protected: &[String]) -> Result<Vec<BookmarkRow>> {
    let refs = list_refs(repo)?;
    Ok(rows(&refs, protected, |a, b| count_range(repo, a, b)))
}

/// Revset for a bookmark name as the user would type it: the bare name when
/// it is a plain jj symbol, else a quoted string symbol.
pub fn symbol(name: &str) -> String {
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_/.-".contains(c))
        && !name.starts_with(['-', '.', '/'])
        && !name.ends_with(['-', '.', '/'])
        && !name.contains("..")
        && !name.contains("--")
        && !name.contains("//");
    if plain {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// Revset for `name@remote`.
pub fn remote_symbol(name: &str, remote: &str) -> String {
    format!("{}@{}", symbol(name), symbol(remote))
}

/// Argument for commands whose bookmark/remote option takes a string
/// pattern (glob by default): the bare name when it has no glob
/// metacharacters, else `exact:<name>`.
pub fn pattern_arg(name: &str) -> String {
    if name.contains(['*', '?', '[', ']', '{', '}', ':', '\\']) {
        format!("exact:{name}")
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(fields: [&str; 10]) -> String {
        format!("{}\x1e", fields.join("\x1f"))
    }

    #[test]
    fn template_uses_documented_delimiters() {
        assert!(REF_TEMPLATE.contains(r#""\x1f""#));
        assert!(REF_TEMPLATE.ends_with(r#""\x1e""#));
        assert!(REF_TEMPLATE.contains(r#""\x01""#));
        assert_eq!(REF_TEMPLATE.matches(r#""\x1f""#).count(), 9);
    }

    #[test]
    fn parses_and_groups_refs() {
        let out = [
            rec(["fix/goals", "", "1", "0", "0", "c2", "ch2", "", "", ""]),
            rec(["fix/goals", "git", "1", "1", "0", "c2", "ch2", "0", "0", ""]),
            rec([
                "fix/goals",
                "origin",
                "1",
                "1",
                "0",
                "c1",
                "ch1",
                "0",
                "1",
                "",
            ]),
            rec([
                "fix/goals",
                "sundae",
                "1",
                "0",
                "0",
                "c0",
                "ch0",
                "",
                "",
                "",
            ]),
            rec(["main", "", "1", "0", "0", "m", "chm", "", "", ""]),
            rec(["main", "origin", "1", "1", "0", "m", "chm", "0", "0", ""]),
            rec([
                "only-remote",
                "sundae",
                "1",
                "0",
                "0",
                "x",
                "chx",
                "",
                "",
                "",
            ]),
        ]
        .concat();
        let refs = parse_refs(&out);
        assert_eq!(refs.len(), 6, "the @git ref is dropped");
        let rows = rows(&refs, &["main".to_string()], |from, to| match (from, to) {
            ("c0", "c2") => Some(2),
            ("c2", "c0") => Some(0),
            other => panic!("unexpected range {other:?}"),
        });
        assert_eq!(rows.len(), 3);
        let fg = &rows[0];
        assert_eq!(fg.name, "fix/goals");
        assert!(!fg.protected);
        let origin = fg.remote("origin").unwrap();
        assert!(origin.tracked);
        assert_eq!(
            (origin.local_ahead, origin.local_behind),
            (Some(1), Some(0))
        );
        let sundae = fg.remote("sundae").unwrap();
        assert!(!sundae.tracked);
        assert_eq!(
            (sundae.local_ahead, sundae.local_behind),
            (Some(2), Some(0))
        );
        assert!(rows[1].protected);
        assert!(rows[2].local.is_none());
        assert_eq!(rows[2].remote("sundae").unwrap().local_ahead, None);
    }

    #[test]
    fn conflicted_local_keeps_targets() {
        let out = rec(["b", "", "1", "0", "1", "", "", "", "", "aa\x01bb"]);
        let rows = rows(&parse_refs(&out), &[], |_, _| None);
        let local = rows[0].local.as_ref().unwrap();
        assert!(local.conflict);
        assert_eq!(local.conflict_change_ids, vec!["aa", "bb"]);
        assert!(rows[0].conflicted());
    }

    #[test]
    fn symbols_and_patterns() {
        assert_eq!(symbol("fix/goals"), "fix/goals");
        assert_eq!(symbol("feature-1.2"), "feature-1.2");
        assert_eq!(symbol("weird name"), "\"weird name\"");
        assert_eq!(symbol("a\"b"), "\"a\\\"b\"");
        assert_eq!(remote_symbol("fix/goals", "origin"), "fix/goals@origin");
        assert_eq!(pattern_arg("fix/goals"), "fix/goals");
        assert_eq!(pattern_arg("a*b"), "exact:a*b");
    }
}
