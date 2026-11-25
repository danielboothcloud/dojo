use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::hooks::{self, TemplateWarning};

const CONFIG_TEMPLATE: &str = include_str!("dojo.template.toml");

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(rename = "workspace-path", default = "default_workspace_path")]
    pub workspace_path: String,
    /// Override for `{{ repo }}` template variable. Falls back to directory name if unset/empty.
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(rename = "pre-start", default)]
    pub pre_start: BTreeMap<String, String>,
    #[serde(rename = "post-start", default)]
    pub post_start: BTreeMap<String, String>,
    /// File templates: key = relative path, value = template content.
    #[serde(default)]
    pub templates: BTreeMap<String, String>,
    /// Custom jj template for the displayed graph pane (passed to `jj log --template`).
    #[serde(rename = "log-template", default)]
    pub log_template: Option<String>,
    /// Override the author on every revision created by dojo via `jj metaedit --author`.
    /// `ji-author` is accepted for configs written for upstream `ji`.
    #[serde(rename = "dojo-author", alias = "ji-author", default)]
    pub dojo_author: Option<String>,
    /// Bookmarks the push remote refuses (Sundae protects `main` by default).
    /// dojo never moves or pushes these; it offers a new bookmark instead.
    #[serde(default = "default_protected")]
    pub protected: Vec<String>,
    /// Remote that `dojo push` pushes to. Default: `sundae` if it exists,
    /// else `origin`, else the only remote.
    #[serde(rename = "push-remote", default)]
    pub push_remote: Option<String>,
    /// Forge remote that `dojo sync` rebases onto. Default: `origin` if it
    /// exists, else the push remote.
    #[serde(rename = "upstream-remote", default)]
    pub upstream_remote: Option<String>,
    /// Restore Finder-relevant xattrs (macOS alias flag, resource fork) after
    /// operations that materialize working-copy files. Gates the xattr writes
    /// only — the fidelity warnings are always emitted.
    #[serde(
        rename = "preserve-finder-xattrs",
        default = "default_preserve_finder_xattrs"
    )]
    pub preserve_finder_xattrs: bool,
    /// Template validation warnings collected at config load time.
    #[serde(skip)]
    pub warnings: Vec<TemplateWarning>,
}

fn default_workspace_path() -> String {
    "../{{ repo }}.{{ bookmark }}".to_string()
}

fn default_preserve_finder_xattrs() -> bool {
    true
}

fn default_protected() -> Vec<String> {
    vec!["main".to_string()]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            workspace_path: default_workspace_path(),
            repo: None,
            pre_start: BTreeMap::new(),
            post_start: BTreeMap::new(),
            templates: BTreeMap::new(),
            log_template: None,
            dojo_author: None,
            protected: default_protected(),
            push_remote: None,
            upstream_remote: None,
            preserve_finder_xattrs: default_preserve_finder_xattrs(),
            warnings: Vec::new(),
        }
    }
}

/// Resolve the `{{ repo }}` variable: use the config override if non-empty,
/// otherwise fall back to the repository directory name.
pub fn resolve_repo_name(config: &Config, repo_root: &Path) -> String {
    if let Some(ref name) = config.repo {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    repo_root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Project config path inside a workspace root.
pub const PROJECT_CONFIG: &str = ".config/dojo.toml";

/// User config path: `$XDG_CONFIG_HOME/dojo/config.toml`, falling back to
/// `$HOME/.config/dojo/config.toml` (on macOS too, like jj's XDG support).
pub fn user_config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|h| PathBuf::from(h).join(".config"))
        })?;
    Some(base.join("dojo").join("config.toml"))
}

/// Merge `overlay` into `base`: tables merge key by key, other values
/// replace.
fn merge_toml(base: &mut toml::Table, overlay: toml::Table) {
    for (key, value) in overlay {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o)) => merge_toml(b, o),
            (_, v) => {
                base.insert(key, v);
            }
        }
    }
}

fn read_table(path: &Path) -> Result<Option<toml::Table>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let table: toml::Table =
        toml::from_str(&contents).with_context(|| format!("invalid TOML in {}", path.display()))?;
    Ok(Some(table))
}

/// Load the user config, then the project config on top of it.
pub fn load_config(repo_root: &Path) -> Result<Config> {
    load_config_from(user_config_path().as_deref(), repo_root)
}

pub fn load_config_from(user_path: Option<&Path>, repo_root: &Path) -> Result<Config> {
    let mut table = toml::Table::new();
    if let Some(user) = user_path
        && let Some(t) = read_table(user)?
    {
        merge_toml(&mut table, t);
    }
    if let Some(t) = read_table(&repo_root.join(PROJECT_CONFIG))? {
        merge_toml(&mut table, t);
    }
    if table.is_empty() {
        return Ok(Config::default());
    }
    let mut config: Config = toml::Value::Table(table)
        .try_into()
        .context("invalid dojo config")?;

    // Validate all template strings for unrecognized or malformed variables.
    let mut warnings = Vec::new();
    warnings.extend(hooks::validate_template(
        &config.workspace_path,
        "workspace-path",
        hooks::PATH_VARS,
    ));
    warnings.extend(hooks::validate_workspace_path_template(
        &config.workspace_path,
    ));
    let sections: &[(&str, &BTreeMap<String, String>)] = &[
        ("pre-start", &config.pre_start),
        ("post-start", &config.post_start),
        ("templates", &config.templates),
    ];
    for (section, map) in sections {
        for (key, value) in *map {
            warnings.extend(hooks::validate_template(
                value,
                &format!("{section}/{key}"),
                hooks::HOOK_VARS,
            ));
        }
    }
    config.warnings = warnings;

    Ok(config)
}

pub fn create_config(repo_root: &Path) -> Result<()> {
    let config_dir = repo_root.join(".config");
    let config_path = config_dir.join("dojo.toml");

    if config_path.exists() {
        anyhow::bail!("{} already exists", config_path.display());
    }

    std::fs::create_dir_all(&config_dir)
        .with_context(|| format!("failed to create {}", config_dir.display()))?;
    std::fs::write(&config_path, CONFIG_TEMPLATE)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    eprintln!("(dojo)::config created {}", config_path.display());
    Ok(())
}
