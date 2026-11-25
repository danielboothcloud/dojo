//! jj user identity check (`user.name` / `user.email`).
//!
//! In Sundae sessions the identity often lives in a per-repo config that the
//! shell cannot see, so commits can end up without an author. dojo shows one
//! clear fix: `jj config set --user ...`.

use serde::Serialize;
use std::path::Path;

use crate::jujutsu::changes::config_get;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Identity {
    pub name: Option<String>,
    pub email: Option<String>,
}

impl Identity {
    pub fn read(repo: &Path) -> Self {
        Self {
            name: config_get(repo, "user.name"),
            email: config_get(repo, "user.email"),
        }
    }

    pub fn is_complete(&self) -> bool {
        self.name.is_some() && self.email.is_some()
    }

    /// The commands that fix a missing identity (placeholders for the user
    /// to fill in).
    pub fn fix_commands(&self) -> Vec<String> {
        let mut cmds = Vec::new();
        if self.name.is_none() {
            cmds.push(r#"jj config set --user user.name "Your Name""#.to_string());
        }
        if self.email.is_none() {
            cmds.push(r#"jj config set --user user.email "you@example.com""#.to_string());
        }
        cmds
    }

    /// One message with the fix, or `None` when the identity is complete.
    pub fn hint(&self) -> Option<String> {
        if self.is_complete() {
            return None;
        }
        Some(format!(
            "jj does not know your name and email, so new commits have no author.\nSet them once for all repos:\n  {}",
            self.fix_commands().join("\n  ")
        ))
    }
}

/// The argument vectors of `jj config set --user` for real values.
pub fn set_commands(name: &str, email: &str) -> Vec<Vec<String>> {
    vec![
        vec![
            "config".into(),
            "set".into(),
            "--user".into(),
            "user.name".into(),
            name.into(),
        ],
        vec![
            "config".into(),
            "set".into(),
            "--user".into(),
            "user.email".into(),
            email.into(),
        ],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_lists_only_missing_keys() {
        let id = Identity {
            name: Some("A".into()),
            email: None,
        };
        let hint = id.hint().unwrap();
        assert!(hint.contains("user.email"));
        assert!(!hint.contains("user.name"));
        let ok = Identity {
            name: Some("A".into()),
            email: Some("a@x".into()),
        };
        assert!(ok.hint().is_none());
    }
}
