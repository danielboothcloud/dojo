//! Bookmark names that `dojo new-branch` promised to create later.
//!
//! `dojo new-branch <name>` (without `--now`) starts a new change and
//! remembers `<name>` for it. The bookmark is created when there is content
//! to push (`dojo push`), so it never points at an empty change. The state
//! lives in `.jj/dojo-pending.json` of the workspace (never committed).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingBookmark {
    pub name: String,
    /// Change id of the change that `jj new` created for it.
    pub change_id: String,
}

fn path(root: &Path) -> PathBuf {
    root.join(".jj").join("dojo-pending.json")
}

pub fn load(root: &Path) -> Vec<PendingBookmark> {
    std::fs::read_to_string(path(root))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(root: &Path, items: &[PendingBookmark]) -> Result<()> {
    let p = path(root);
    if items.is_empty() {
        if p.exists() {
            std::fs::remove_file(&p).with_context(|| format!("remove {}", p.display()))?;
        }
        return Ok(());
    }
    let body = serde_json::to_string_pretty(items)?;
    std::fs::write(&p, body).with_context(|| format!("write {}", p.display()))
}

pub fn add(root: &Path, name: &str, change_id: &str) -> Result<()> {
    let mut items = load(root);
    items.retain(|p| p.name != name);
    items.push(PendingBookmark {
        name: name.to_string(),
        change_id: change_id.to_string(),
    });
    save(root, &items)
}

pub fn remove(root: &Path, name: &str) -> Result<()> {
    let mut items = load(root);
    items.retain(|p| p.name != name);
    save(root, &items)
}
