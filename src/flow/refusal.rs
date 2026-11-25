//! Parse Sundae push refusals from `jj git push` stderr.
//!
//! The Sundae server prints its rules as `remote: sundae: ...` lines. This
//! is the one place where dojo reads human text, because the server message
//! is the only source of the reason. Matching is on stable key phrases only.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrivatePath {
    pub commit: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Refusal {
    /// The bookmark is protected on the server (default: `main`).
    Protected { bookmark: Option<String> },
    /// The forge has commits on this bookmark that the session lacks.
    ForgeAhead { bookmark: Option<String> },
    /// Pushed commits add private paths (AGENTS.md, .pi/, ...).
    PrivatePaths { paths: Vec<PrivatePath> },
    /// Any other push failure; `message` is jj's stderr.
    Other { message: String },
}

/// The `sundae: ...` messages in jj's stderr, without the prefixes.
pub fn sundae_lines(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .filter_map(|line| {
            let l = line.trim();
            let l = l.strip_prefix("remote:").map_or(l, str::trim_start);
            l.strip_prefix("sundae:").map(|m| m.trim().to_string())
        })
        .collect()
}

/// Classify a failed push. Several Sundae messages can appear; private
/// paths win (they must be fixed first), then protection, then forge-ahead.
pub fn parse(stderr: &str) -> Refusal {
    let lines = sundae_lines(stderr);
    let paths: Vec<PrivatePath> = lines
        .iter()
        .filter(|l| l.contains("private path"))
        .filter_map(|l| parse_private(l))
        .collect();
    if !paths.is_empty() {
        return Refusal::PrivatePaths { paths };
    }
    if let Some(l) = lines.iter().find(|l| l.contains("is protected")) {
        return Refusal::Protected {
            bookmark: parse_protected(l),
        };
    }
    if let Some(l) = lines.iter().find(|l| l.contains("has commits you lack")) {
        return Refusal::ForgeAhead {
            bookmark: between(l, "forge ", " has commits you lack"),
        };
    }
    Refusal::Other {
        message: stderr.trim_end().to_string(),
    }
}

fn between(s: &str, start: &str, end: &str) -> Option<String> {
    let from = s.find(start)? + start.len();
    let to = from + s[from..].find(end)?;
    let v = s[from..to].trim();
    (!v.is_empty()).then(|| v.to_string())
}

fn parse_protected(line: &str) -> Option<String> {
    // "refusing to update main: main is protected" / "bookmark main is protected"
    let head = &line[..line.find(" is protected")?];
    head.rsplit(|c: char| c.is_whitespace() || c == ':')
        .find(|w| !w.is_empty())
        .map(|w| w.trim_matches(['\'', '"', '`']).to_string())
}

fn parse_private(line: &str) -> Option<PrivatePath> {
    // "commit <sha> adds private path <path>; ..."
    let commit = line
        .split_whitespace()
        .find(|w| w.len() >= 7 && w.chars().all(|c| c.is_ascii_hexdigit()))
        .unwrap_or("")
        .to_string();
    let after = &line[line.find("private path")? + "private path".len()..];
    let after = after.trim_start_matches([' ', ':']);
    let path = after
        .split([';', ','])
        .next()?
        .trim()
        .trim_matches(['\'', '"', '`'])
        .to_string();
    (!path.is_empty()).then_some(PrivatePath { commit, path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forge_ahead() {
        let err = "Changes to push to sundae:\n  bookmark: fix/goals [move forward]\n\
            remote: sundae: forge fix/goals has commits you lack; run jj git fetch, rebase onto fix/goals@origin, and push again        \n\
            Error: Failed to push some bookmarks\n";
        assert_eq!(
            parse(err),
            Refusal::ForgeAhead {
                bookmark: Some("fix/goals".into())
            }
        );
    }

    #[test]
    fn protected() {
        let err = "remote: sundae: refusing to update main: main is protected\n";
        assert_eq!(
            parse(err),
            Refusal::Protected {
                bookmark: Some("main".into())
            }
        );
        let err = "remote: sundae: bookmark 'release' is protected\n";
        assert_eq!(
            parse(err),
            Refusal::Protected {
                bookmark: Some("release".into())
            }
        );
    }

    #[test]
    fn private_paths() {
        let err = "remote: sundae: commit d4f1829e7932860ae88b19c6e47d772dcf50dac3 adds private path todo.txt; private files live in refs/sundae/private\n\
                   remote: sundae: commit d4f1829e7932860ae88b19c6e47d772dcf50dac3 adds private path .pi/settings.json\n";
        match parse(err) {
            Refusal::PrivatePaths { paths } => {
                assert_eq!(paths.len(), 2);
                assert_eq!(paths[0].path, "todo.txt");
                assert_eq!(paths[1].path, ".pi/settings.json");
                assert!(paths[0].commit.starts_with("d4f1829e"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn other() {
        let err = "Error: Won't push commit a87e140aa74a since it has no description\n";
        assert!(matches!(parse(err), Refusal::Other { .. }));
    }
}
