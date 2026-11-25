//! `dojo status`: "where am I" in one screen, plus the next command.

use anyhow::Result;
use serde::Serialize;

use super::identity::Identity;
use super::{Ctx, push};
use crate::jujutsu::bookmarks::BookmarkRow;
use crate::jujutsu::changes::{self, ChangeInfo};

#[derive(Debug, Clone, Serialize)]
pub struct RemoteState {
    pub remote: String,
    pub present: bool,
    pub tracked: bool,
    /// Local commits the remote lacks.
    pub ahead: Option<u64>,
    /// Remote commits the local bookmark lacks.
    pub behind: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusReport {
    pub working_copy: ChangeInfo,
    pub parent: Option<ChangeInfo>,
    /// The bookmark of the current work.
    pub bookmark: Option<String>,
    /// Where the bookmark is: `@`, `@-`, `pending` (dojo new-branch) or
    /// `below` (an older change).
    pub bookmark_at: Option<String>,
    pub protected: bool,
    pub remotes: Vec<RemoteState>,
    pub push_remote: String,
    pub conflicted_changes: Vec<ChangeInfo>,
    pub identity_hint: Option<String>,
    /// The next suggested command and why.
    pub next: Option<String>,
    pub next_reason: Option<String>,
}

pub fn status(ctx: &Ctx) -> Result<StatusReport> {
    let at = ctx.change("@")?;
    let parent = if at.parents.len() == 1 {
        ctx.change("@-").ok()
    } else {
        None
    };
    let rows = ctx.bookmark_rows()?;

    let (bookmark, bookmark_at) = locate_bookmark(ctx)?;
    let row = bookmark
        .as_deref()
        .and_then(|b| rows.iter().find(|r| r.name == b));
    let remotes = ctx
        .remotes
        .display_order()
        .into_iter()
        .map(|r| remote_state(row, &r))
        .collect::<Vec<_>>();
    let conflicted_changes =
        changes::changes(&ctx.root, "(::@ & mutable() & conflicts())").unwrap_or_default();
    let identity_hint = Identity::read(&ctx.root).hint();
    let protected = bookmark.as_deref().is_some_and(|b| ctx.is_protected(b));

    let mut report = StatusReport {
        working_copy: at,
        parent,
        bookmark,
        bookmark_at,
        protected,
        remotes,
        push_remote: ctx.remotes.push.clone(),
        conflicted_changes,
        identity_hint,
        next: None,
        next_reason: None,
    };
    let (next, why) = suggest(ctx, &report, row);
    report.next = next;
    report.next_reason = why;
    Ok(report)
}

fn locate_bookmark(ctx: &Ctx) -> Result<(Option<String>, Option<String>)> {
    if let Some((b, where_)) = push::locate_current_bookmark(ctx)? {
        return Ok((Some(b), Some(where_.as_str().to_string())));
    }
    let below = changes::changes(&ctx.root, "heads(::@ & bookmarks())").unwrap_or_default();
    if let Some(c) = below.into_iter().find(|c| !c.local_bookmarks.is_empty()) {
        let name = c
            .local_bookmarks
            .iter()
            .find(|b| !ctx.is_protected(b))
            .or_else(|| c.local_bookmarks.first())
            .cloned();
        return Ok((name, Some("below".to_string())));
    }
    Ok((None, None))
}

fn remote_state(row: Option<&BookmarkRow>, remote: &str) -> RemoteState {
    let cell = row.and_then(|r| r.remote(remote));
    RemoteState {
        remote: remote.to_string(),
        present: cell.is_some(),
        tracked: cell.is_some_and(|c| c.tracked),
        ahead: cell.and_then(|c| c.local_ahead),
        behind: cell.and_then(|c| c.local_behind),
    }
}

fn suggest(
    ctx: &Ctx,
    r: &StatusReport,
    row: Option<&BookmarkRow>,
) -> (Option<String>, Option<String>) {
    let s = |cmd: String, why: &str| (Some(cmd), Some(why.to_string()));
    if r.identity_hint.is_some() {
        return s(
            r#"jj config set --user user.name "Your Name""#.to_string(),
            "jj does not know who you are",
        );
    }
    if let Some(c) = r.conflicted_changes.first() {
        return s(
            format!("jj new {}", c.short_id),
            "a change has conflicts: edit the files on top of it, then `jj squash`",
        );
    }
    let has_work = !r.working_copy.empty || r.parent.as_ref().is_some_and(|p| !p.immutable);
    let Some(b) = &r.bookmark else {
        return if has_work {
            s(
                "dojo push".to_string(),
                "your change has no bookmark yet; dojo push offers a push-* name",
            )
        } else {
            s(
                "dojo switch <bookmark>".to_string(),
                "pick a bookmark to work on (see dojo branches)",
            )
        };
    };
    if r.protected && has_work {
        return s(
            "dojo push --as <new-name>".to_string(),
            "this bookmark is protected; push your work under a new name",
        );
    }
    let upstream = r.remotes.iter().find(|x| x.remote == ctx.remotes.upstream);
    if upstream.is_some_and(|u| u.behind.unwrap_or(0) > 0) {
        return s(
            format!("dojo sync {b}"),
            "the forge has commits that you do not have",
        );
    }
    let push = r.remotes.iter().find(|x| x.remote == ctx.remotes.push);
    let unpushed = match push {
        Some(p) if p.present => p.ahead.unwrap_or(0) > 0,
        _ => true,
    };
    let at_has_changes = !r.working_copy.empty;
    if (unpushed && row.is_some_and(|x| x.local.is_some()))
        || r.bookmark_at.as_deref() == Some("pending")
        || (r.bookmark_at.as_deref() == Some("@-") && at_has_changes)
    {
        return s(format!("dojo push {b}"), "you have work that is not pushed");
    }
    if ctx.remotes.forge().is_some() && push.is_some_and(|p| p.present) {
        return s(
            format!("dojo pr {b}"),
            "everything is pushed; open (or check) the pull request",
        );
    }
    (None, None)
}

pub fn render(r: &StatusReport) -> String {
    let mut out = String::new();
    let wc = &r.working_copy;
    let title = |c: &ChangeInfo| {
        if c.title().is_empty() {
            "(no description)".to_string()
        } else {
            c.title().to_string()
        }
    };
    let files = if wc.files.is_empty() {
        "no file changes".to_string()
    } else {
        format!(
            "{} file(s) changed, +{} -{}",
            wc.files.len(),
            wc.lines_added,
            wc.lines_removed
        )
    };
    out.push_str(&format!(
        "Working copy  {}  {}  ({files}){}\n",
        wc.short_id,
        title(wc),
        if wc.conflict { "  CONFLICT" } else { "" }
    ));
    if let Some(p) = &r.parent {
        out.push_str(&format!("Parent        {}  {}\n", p.short_id, title(p)));
    }
    match (&r.bookmark, r.bookmark_at.as_deref()) {
        (Some(b), Some(at)) => {
            let where_ = match at {
                "@" => "on the working copy".to_string(),
                "@-" => "on the parent".to_string(),
                "pending" => "not created yet; `dojo push` creates it".to_string(),
                _ => "on an older change (below)".to_string(),
            };
            out.push_str(&format!(
                "Bookmark      {b}  ({where_}){}\n",
                if r.protected { "  [protected]" } else { "" }
            ));
            for rs in &r.remotes {
                let state = if rs.present {
                    let ab = match (rs.ahead, rs.behind) {
                        (Some(0), Some(0)) => "in sync".to_string(),
                        (Some(a), Some(0)) => format!("{a} commit(s) to push"),
                        (Some(0), Some(b)) => format!("{b} commit(s) behind"),
                        (Some(a), Some(b)) => format!("{a} ahead, {b} behind"),
                        _ => "unknown".to_string(),
                    };
                    format!("{ab}{}", if rs.tracked { "" } else { " (not tracked)" })
                } else {
                    "not on this remote".to_string()
                };
                let role = if rs.remote == r.push_remote {
                    " (push)"
                } else {
                    ""
                };
                out.push_str(&format!("  @{}{role}: {state}\n", rs.remote));
            }
        }
        _ => out.push_str("Bookmark      (none)\n"),
    }
    if !r.conflicted_changes.is_empty() {
        out.push_str("Conflicts in:\n");
        for c in &r.conflicted_changes {
            out.push_str(&format!("  {}  {}\n", c.short_id, title(c)));
        }
    }
    if let Some(h) = &r.identity_hint {
        out.push_str(&format!("\n{h}\n"));
    }
    if let Some(n) = &r.next {
        out.push_str(&format!(
            "\nNext: {n}\n      ({})\n",
            r.next_reason.as_deref().unwrap_or("")
        ));
    }
    out
}
