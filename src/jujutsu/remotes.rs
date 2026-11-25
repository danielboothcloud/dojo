//! Git remotes of the repo, and which one dojo pushes to / fetches from.
//!
//! dojo never relies on jj's `git.push` / `git.fetch` settings (in Sundae
//! sessions the per-repo config is often invisible to the shell). It reads
//! the remotes itself and always passes explicit `--remote` flags.

use anyhow::{Result, bail};
use serde::Serialize;
use std::path::Path;

use super::run_jj_query;

/// Name of the Sundae server remote.
pub const SUNDAE: &str = "sundae";
/// Conventional name of the forge remote.
pub const ORIGIN: &str = "origin";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Remote {
    pub name: String,
    pub fetch_url: String,
    /// Separate push URL, when one is set (`git remote set-url --push`).
    pub push_url: Option<String>,
}

/// The remotes dojo uses for each job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteRoles {
    /// All remotes, in `jj git remote list` order.
    pub all: Vec<Remote>,
    /// Where `dojo push` pushes (`sundae` if present).
    pub push: String,
    /// Remotes `dojo fetch` fetches from (all of them).
    pub fetch: Vec<String>,
    /// The forge remote that `dojo sync` rebases onto (`origin` if present,
    /// else the push remote).
    pub upstream: String,
    /// True when a `sundae` remote exists (Sundae session layout).
    pub sundae: bool,
}

impl RemoteRoles {
    pub fn get(&self, name: &str) -> Option<&Remote> {
        self.all.iter().find(|r| r.name == name)
    }

    /// The forge remote (`origin`), if it exists.
    pub fn forge(&self) -> Option<&Remote> {
        self.get(ORIGIN)
    }

    /// Remote names in display order: push remote first, then the others.
    pub fn display_order(&self) -> Vec<String> {
        let mut names = vec![self.push.clone()];
        for r in &self.all {
            if r.name != self.push {
                names.push(r.name.clone());
            }
        }
        names
    }
}

/// List the remotes with `jj git remote list`.
///
/// That command has no template option. Its line format is fixed and
/// machine-stable: `<name> <fetch-url>[ (push: <push-url>)]`. Git refuses
/// whitespace in remote names, so the first word is always the full name.
pub fn list_remotes(repo: &Path) -> Result<Vec<Remote>> {
    let out = run_jj_query(repo, &["git", "remote", "list"])?;
    Ok(parse_remote_list(&out))
}

pub(crate) fn parse_remote_list(out: &str) -> Vec<Remote> {
    out.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (name, rest) = line.split_once(char::is_whitespace)?;
            let rest = rest.trim();
            let (fetch_url, push_url) = match rest.strip_suffix(')') {
                Some(head) => match head.rsplit_once(" (push: ") {
                    Some((f, p)) => (f.trim().to_string(), Some(p.trim().to_string())),
                    None => (rest.to_string(), None),
                },
                None => (rest.to_string(), None),
            };
            Some(Remote {
                name: name.to_string(),
                fetch_url,
                push_url,
            })
        })
        .collect()
}

/// Decide the remote roles. `push_override` / `upstream_override` come from
/// `dojo.toml` (`push-remote`, `upstream-remote`).
pub fn roles(
    remotes: Vec<Remote>,
    push_override: Option<&str>,
    upstream_override: Option<&str>,
) -> Result<RemoteRoles> {
    if remotes.is_empty() {
        bail!(
            "this repo has no git remotes. Add one with: jj git remote add origin <url>"
        );
    }
    let has = |n: &str| remotes.iter().any(|r| r.name == n);
    let sundae = has(SUNDAE);
    let push = if let Some(p) = push_override {
        if !has(p) {
            bail!("push-remote '{p}' (from dojo.toml) is not a remote of this repo");
        }
        p.to_string()
    } else if sundae {
        SUNDAE.to_string()
    } else if has(ORIGIN) {
        ORIGIN.to_string()
    } else if remotes.len() == 1 {
        remotes[0].name.clone()
    } else {
        bail!(
            "cannot choose a push remote (no 'sundae' or 'origin'). Set push-remote in .config/dojo.toml"
        );
    };
    let upstream = match upstream_override {
        Some(u) if has(u) => u.to_string(),
        Some(u) => bail!("upstream-remote '{u}' (from dojo.toml) is not a remote of this repo"),
        None if has(ORIGIN) => ORIGIN.to_string(),
        None => push.clone(),
    };
    let fetch = remotes.iter().map(|r| r.name.clone()).collect();
    Ok(RemoteRoles {
        all: remotes,
        push,
        fetch,
        upstream,
        sundae,
    })
}

/// Read the remotes and decide the roles in one call.
pub fn detect(
    repo: &Path,
    push_override: Option<&str>,
    upstream_override: Option<&str>,
) -> Result<RemoteRoles> {
    roles(list_remotes(repo)?, push_override, upstream_override)
}

/// Forge kind of a remote URL, for `dojo pr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Forge {
    GitHub,
    GitLab,
}

/// `(forge, host, "owner/repo")` parsed from an https or scp-like SSH URL.
pub fn forge_of_url(url: &str) -> Option<(Forge, String, String)> {
    let url = url.trim();
    let (host, path) = if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("ssh://"))
    {
        let rest = rest.rsplit_once('@').map_or(rest, |(_, r)| r);
        let (host, path) = rest.split_once('/')?;
        (host.split(':').next()?.to_string(), path.to_string())
    } else if let Some((user_host, path)) = url.split_once(':') {
        // scp-like: git@github.com:owner/repo.git
        let host = user_host.rsplit_once('@').map_or(user_host, |(_, h)| h);
        if host.contains('/') {
            return None;
        }
        (host.to_string(), path.to_string())
    } else {
        return None;
    };
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path).to_string();
    if path.is_empty() || !path.contains('/') {
        return None;
    }
    let host_l = host.to_ascii_lowercase();
    let forge = if host_l == "github.com" || host_l.ends_with(".github.com") {
        Forge::GitHub
    } else if host_l.contains("gitlab") {
        Forge::GitLab
    } else {
        return None;
    };
    Some((forge, host, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(name: &str) -> Remote {
        Remote {
            name: name.into(),
            fetch_url: format!("https://example.com/{name}.git"),
            push_url: None,
        }
    }

    #[test]
    fn parses_remote_list_with_push_url() {
        let out = "origin https://github.com/o/r.git (push: file:///sundae-forwards-pushes/use-jj-git-push-to-sundae)\n\
                   sundae http://sundae-http.ns.svc.cluster.local:23232/o/r.git\n";
        let rs = parse_remote_list(out);
        assert_eq!(rs.len(), 2);
        assert_eq!(rs[0].name, "origin");
        assert_eq!(rs[0].fetch_url, "https://github.com/o/r.git");
        assert_eq!(
            rs[0].push_url.as_deref(),
            Some("file:///sundae-forwards-pushes/use-jj-git-push-to-sundae")
        );
        assert_eq!(rs[1].name, "sundae");
        assert_eq!(rs[1].push_url, None);
    }

    #[test]
    fn sundae_is_push_remote_and_origin_is_upstream() {
        let roles = roles(vec![r("origin"), r("sundae")], None, None).unwrap();
        assert_eq!(roles.push, "sundae");
        assert_eq!(roles.upstream, "origin");
        assert_eq!(roles.fetch, vec!["origin", "sundae"]);
        assert!(roles.sundae);
        assert_eq!(roles.display_order(), vec!["sundae", "origin"]);
    }

    #[test]
    fn plain_repo_uses_origin_for_everything() {
        let roles = roles(vec![r("origin")], None, None).unwrap();
        assert_eq!(roles.push, "origin");
        assert_eq!(roles.upstream, "origin");
        assert!(!roles.sundae);
    }

    #[test]
    fn single_odd_remote_and_ambiguous_remotes() {
        assert_eq!(roles(vec![r("fork")], None, None).unwrap().push, "fork");
        assert!(roles(vec![r("a"), r("b")], None, None).is_err());
        assert_eq!(
            roles(vec![r("a"), r("b")], Some("b"), None).unwrap().push,
            "b"
        );
        assert!(roles(vec![], None, None).is_err());
    }

    #[test]
    fn forge_detection() {
        assert_eq!(
            forge_of_url("https://github.com/danielboothcloud/dojo.git"),
            Some((Forge::GitHub, "github.com".into(), "danielboothcloud/dojo".into()))
        );
        assert_eq!(
            forge_of_url("git@gitlab.com:group/sub/proj.git"),
            Some((Forge::GitLab, "gitlab.com".into(), "group/sub/proj".into()))
        );
        assert_eq!(
            forge_of_url("https://user:tok@gitlab.example.org/g/p"),
            Some((Forge::GitLab, "gitlab.example.org".into(), "g/p".into()))
        );
        assert_eq!(forge_of_url("/tmp/forge.git"), None);
        assert_eq!(forge_of_url("https://example.com/a/b.git"), None);
    }
}
