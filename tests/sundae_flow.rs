//! End-to-end tests of the dojo workflows on a local copy of the Sundae
//! layout (see tests/fixtures/sundae_layout.sh): a bare forge, a bare
//! Sundae server with the refusal hooks, and a colocated jj clone with the
//! remotes `sundae` + `origin` and NO per-repo jj config.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sundae_layout.sh");
const OUTSIDE_PUSH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/outside_push.sh");

struct Layout {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    work: PathBuf,
    home: PathBuf,
    jj_config: PathBuf,
}

impl Layout {
    fn new() -> Self {
        Self::with_identity(true)
    }

    fn with_identity(identity: bool) -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = tmp.path().canonicalize().unwrap();
        let home = base.join("home");
        std::fs::create_dir_all(home.join(".config")).unwrap();
        let jj_config = base.join("jj-user.toml");
        let body = if identity {
            "[user]\nname = \"Test User\"\nemail = \"test@example.com\"\n"
        } else {
            ""
        };
        std::fs::write(&jj_config, body).unwrap();
        let root = base.join("layout");
        let layout = Self {
            work: root.join("work"),
            root,
            home,
            jj_config,
            _tmp: tmp,
        };
        let out = layout.env(Command::new("sh"))
            .arg(FIXTURE)
            .arg(&layout.root)
            .stdin(Stdio::null())
            .output()
            .expect("run fixture");
        assert!(
            out.status.success(),
            "fixture failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        layout
    }

    fn env(&self, mut cmd: Command) -> Command {
        cmd.env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("JJ_CONFIG", &self.jj_config)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("DOJO_DIRECTIVE_FILE")
            .env_remove("JJ_USER")
            .env_remove("JJ_EMAIL");
        cmd
    }

    fn dojo(&self, args: &[&str]) -> Output {
        self.env(Command::new(env!("CARGO_BIN_EXE_dojo")))
            .args(args)
            .current_dir(&self.work)
            .stdin(Stdio::null())
            .output()
            .expect("run dojo")
    }

    /// Run dojo with `--json`, assert the exit code, parse stdout.
    fn dojo_json(&self, args: &[&str], code: i32) -> Value {
        let mut full = vec!["--json"];
        full.extend_from_slice(args);
        let out = self.dojo(&full);
        assert_eq!(
            out.status.code(),
            Some(code),
            "dojo {args:?} exit code\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
            panic!(
                "dojo {args:?} printed invalid JSON ({e}):\n{}",
                String::from_utf8_lossy(&out.stdout)
            )
        })
    }

    fn dojo_ok(&self, args: &[&str]) -> String {
        let out = self.dojo(args);
        assert!(
            out.status.success(),
            "dojo {args:?} failed\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    }

    fn jj(&self, args: &[&str]) -> String {
        let out = self
            .env(Command::new("jj"))
            .args(args)
            .current_dir(&self.work)
            .stdin(Stdio::null())
            .output()
            .expect("run jj");
        assert!(
            out.status.success(),
            "jj {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = self
            .env(Command::new("git"))
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Commit id of a branch in a bare repo, or "" when missing.
    fn bare_head(&self, repo: &str, branch: &str) -> String {
        let out = self
            .env(Command::new("git"))
            .args(["--git-dir", repo, "rev-parse", "-q", "--verify"])
            .arg(format!("refs/heads/{branch}"))
            .current_dir(&self.root)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn outside_push(&self, branch: &str, file: &str, line: &str) {
        let out = self
            .env(Command::new("sh"))
            .args([OUTSIDE_PUSH])
            .arg(&self.root)
            .args([branch, file, line])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "outside push failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn write(&self, rel: &str, content: &str) {
        let p = self.work.join(rel);
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(p, content).unwrap();
    }

    fn append(&self, rel: &str, line: &str) {
        let p = self.work.join(rel);
        let mut s = std::fs::read_to_string(&p).unwrap_or_default();
        s.push_str(line);
        s.push('\n');
        std::fs::write(p, s).unwrap();
    }

    fn commit_of(&self, rev: &str) -> String {
        self.jj(&[
            "log",
            "--no-graph",
            "--ignore-working-copy",
            "-r",
            rev,
            "-T",
            "commit_id",
        ])
    }
}

fn row<'a>(branches: &'a Value, name: &str) -> &'a Value {
    branches["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .unwrap_or_else(|| panic!("no row {name} in {branches:#}"))
}

fn remote_cell<'a>(row: &'a Value, remote: &str) -> Option<&'a Value> {
    row["remotes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["remote"] == remote)
}

fn commands(report: &Value) -> Vec<String> {
    report["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["command"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn layout_has_no_per_repo_jj_config() {
    let l = Layout::new();
    let out = l
        .env(Command::new("jj"))
        .args(["config", "list", "--repo"])
        .current_dir(&l.work)
        .output()
        .unwrap();
    let listed = String::from_utf8_lossy(&out.stdout);
    assert!(
        !listed.contains("git.push") && !listed.contains("git.fetch"),
        "{listed}"
    );
}

#[test]
fn fetch_reports_new_moved_and_deleted_per_remote() {
    let l = Layout::new();
    let outside = l.root.join("outside");
    // New + soon-deleted branch on the forge, and a moved fix/goals.
    l.git(&outside, &["push", "-q", "origin", "origin/main:refs/heads/tmp"]);
    l.outside_push("fix/goals", "goals.txt", "outside edit");
    let r = l.dojo_json(&["fetch"], 0);
    assert_eq!(
        commands(&r),
        vec!["jj git fetch --remote origin --remote sundae"]
    );
    let origin = r["remote_changes"]["origin"].as_array().unwrap();
    assert!(origin.iter().any(|c| c["name"] == "tmp" && c["kind"] == "new"));
    assert!(origin.iter().any(|c| c["name"] == "fix/goals" && c["kind"] == "moved"));
    assert!(r["remote_changes"]["sundae"].as_array().unwrap().is_empty());

    l.git(&outside, &["push", "-q", "origin", "--delete", "tmp"]);
    let r = l.dojo_json(&["fetch"], 0);
    let origin = r["remote_changes"]["origin"].as_array().unwrap();
    assert_eq!(origin.len(), 1);
    assert_eq!(origin[0]["name"], "tmp");
    assert_eq!(origin[0]["kind"], "deleted");

    let text = l.dojo_ok(&["fetch"]);
    assert!(text.contains("Nothing new on the remotes."), "{text}");
}

#[test]
fn branches_table_shows_remotes_tracking_and_protection() {
    let l = Layout::new();
    let b = l.dojo_json(&["branches"], 0);
    assert_eq!(b["remotes"], serde_json::json!(["sundae", "origin"]));
    let fix = row(&b, "fix/goals");
    assert!(fix["local"].is_null());
    assert!(remote_cell(fix, "sundae").is_some());
    assert!(remote_cell(fix, "origin").is_some());
    let main = row(&b, "main");
    assert_eq!(main["protected"], true);
    assert_eq!(remote_cell(main, "origin").unwrap()["local_ahead"], 0);

    let text = l.dojo_ok(&["branches"]);
    let header = text.lines().next().unwrap();
    for col in ["BOOKMARK", "LOCAL", "@sundae", "@origin", "TRACKED", "±sundae", "±origin"] {
        assert!(header.contains(col), "missing {col}: {header}");
    }
    assert!(text.contains("protected"), "{text}");
    assert!(text.contains("remote only"), "{text}");
}

#[test]
fn switch_tracks_untracked_remote_bookmark_and_keeps_edits() {
    let l = Layout::new();
    // Edits in the current change must survive the switch.
    l.append("README.md", "local edit before switch");
    let r = l.dojo_json(&["switch", "fix/goals"], 0);
    assert_eq!(
        commands(&r),
        vec![
            "jj bookmark track fix/goals --remote sundae",
            "jj new fix/goals"
        ]
    );
    assert_eq!(r["tracked_remote"], "sundae");
    let kept = &r["kept_change"];
    assert!(kept["back"].as_str().unwrap().starts_with("jj edit "));
    // @- is fix/goals; the old change still has the edit.
    assert_eq!(l.commit_of("@-"), l.commit_of("fix/goals"));
    let kept_id = kept["change_id"].as_str().unwrap();
    let diff = l.jj(&["diff", "--ignore-working-copy", "-r", kept_id, "--name-only"]);
    assert_eq!(diff, "README.md");

    let b = l.dojo_json(&["branches"], 0);
    let fix = row(&b, "fix/goals");
    assert!(!fix["local"].is_null());
    assert_eq!(remote_cell(fix, "sundae").unwrap()["tracked"], true);

    // A second switch is a no-op.
    let r = l.dojo_json(&["switch", "fix/goals"], 0);
    assert_eq!(r["already_there"], true);
    assert!(commands(&r).is_empty());

    // Unknown bookmarks fetch first, then fail with a hint.
    let out = l.dojo(&["switch", "no-such"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("dojo new-branch no-such"), "{err}");
}

#[test]
fn switch_fetches_a_bookmark_that_is_new_on_the_forge() {
    let l = Layout::new();
    let outside = l.root.join("outside");
    l.git(&outside, &["push", "-q", "origin", "origin/main:refs/heads/from-forge"]);
    let r = l.dojo_json(&["switch", "from-forge"], 0);
    let cmds = commands(&r);
    assert_eq!(cmds[0], "jj git fetch --remote origin --remote sundae");
    assert_eq!(cmds[1], "jj bookmark track from-forge --remote origin");
    assert_eq!(cmds[2], "jj new from-forge");
}

#[test]
fn push_success_moves_bookmark_and_reaches_sundae_and_forge() {
    let l = Layout::new();
    l.dojo_ok(&["switch", "fix/goals"]);
    l.append("goals.txt", "my change");
    // No flags: the undescribed change gets a generated description.
    let r = l.dojo_json(&["push"], 0);
    assert_eq!(r["outcome"], "pushed");
    let cmds = commands(&r);
    assert!(
        cmds[0].starts_with("jj describe ") && cmds[0].ends_with(" --message 'Update goals.txt'"),
        "{cmds:?}"
    );
    assert!(cmds.contains(&"jj bookmark set fix/goals --revision @".to_string()));
    assert!(cmds.contains(&"jj git push --remote sundae --bookmark fix/goals".to_string()));
    let local = l.commit_of("fix/goals");
    assert_eq!(l.bare_head("sundae.git", "fix/goals"), local);
    assert_eq!(l.bare_head("forge.git", "fix/goals"), local, "Sundae forwards to the forge");
    // Pushing again is a no-op.
    let r = l.dojo_json(&["push"], 0);
    assert_eq!(r["outcome"], "up-to-date");
}

#[test]
fn push_refused_when_forge_has_commits_then_sync_fixes_it() {
    let l = Layout::new();
    l.dojo_ok(&["switch", "fix/goals"]);
    l.append("goals.txt", "first");
    l.dojo_ok(&["push", "-m", "first"]);
    l.outside_push("fix/goals", "goals.txt", "outside");
    l.jj(&["new", "-m", "second"]);
    l.write("second.txt", "two\n");

    let r = l.dojo_json(&["push"], 2);
    assert_eq!(r["outcome"], "refused");
    assert_eq!(r["refusal"]["kind"], "forge-ahead");
    assert_eq!(r["refusal"]["bookmark"], "fix/goals");
    assert_eq!(r["offer"]["action"]["kind"], "sync");
    assert_eq!(r["offer"]["dojo"], "dojo sync fix/goals");

    // The follow-up: sync rebases onto the forge's commit, then push works.
    let s = l.dojo_json(&["sync", "fix/goals"], 0);
    assert_eq!(s["outcome"], "rebased");
    assert!(commands(&s).contains(
        &"jj rebase --branch fix/goals --destination fix/goals@origin".to_string()
    ));
    assert_eq!(s["offer"]["action"]["kind"], "push");
    let forge_before = l.bare_head("forge.git", "fix/goals");
    let p = l.dojo_json(&["push", "fix/goals"], 0);
    assert_eq!(p["outcome"], "pushed");
    let forge_after = l.bare_head("forge.git", "fix/goals");
    assert_ne!(forge_before, forge_after);
    // The forge's outside commit is an ancestor of the new head.
    let ok = l
        .env(Command::new("git"))
        .args(["--git-dir", "forge.git", "merge-base", "--is-ancestor"])
        .args([&forge_before, &forge_after])
        .current_dir(&l.root)
        .status()
        .unwrap();
    assert!(ok.success());
}

#[test]
fn sync_with_yes_pushes_in_one_command() {
    let l = Layout::new();
    l.dojo_ok(&["switch", "fix/goals"]);
    l.append("goals.txt", "mine");
    l.dojo_ok(&["push", "-m", "mine"]);
    l.outside_push("fix/goals", "goals.txt", "theirs");
    l.append("goals.txt", "more");
    let text = l.dojo_ok(&["sync", "fix/goals", "--yes"]);
    assert!(text.contains("jj rebase --branch"), "{text}");
    assert!(text.contains("Pushed fix/goals to sundae"), "{text}");
}

#[test]
fn push_protected_is_refused_before_contacting_the_server() {
    let l = Layout::new();
    l.jj(&["new", "main", "-m", "on main"]);
    l.write("m.txt", "m\n");
    l.jj(&["bookmark", "set", "main", "-r", "@"]);
    let r = l.dojo_json(&["push", "main"], 2);
    assert_eq!(r["refusal"]["kind"], "protected");
    assert_eq!(r["offer"]["action"]["kind"], "push-as");
    assert!(commands(&r).is_empty(), "no push was attempted");
}

#[test]
fn push_protected_refused_by_server_then_pushed_as_new_bookmark() {
    let l = Layout::new();
    // Not known to dojo: only the server refuses it.
    l.write(".config/dojo.toml", "protected = []\n");
    l.jj(&["new", "main", "-m", "on main"]);
    l.write("m.txt", "m\n");
    l.jj(&["bookmark", "set", "main", "-r", "@"]);
    let r = l.dojo_json(&["push", "main"], 2);
    assert_eq!(r["refusal"]["kind"], "protected", "{r:#}");
    assert_eq!(r["refusal"]["bookmark"], "main");
    let rev = r["offer"]["action"]["rev"].as_str().unwrap().to_string();

    let p = l.dojo_json(&["push", "--change", &rev], 0);
    assert_eq!(p["outcome"], "pushed");
    let b = l.dojo_json(&["branches"], 0);
    let pushed = b["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"].as_str().unwrap().starts_with("push-"))
        .expect("a push-* bookmark");
    assert!(remote_cell(pushed, "sundae").is_some());

    // --as <name>: same change, chosen name.
    let p = l.dojo_json(&["push", "--as", "fix/main-work"], 0);
    assert_eq!(p["outcome"], "pushed");
    assert!(!l.bare_head("sundae.git", "fix/main-work").is_empty());
}

#[test]
fn push_refused_for_private_path() {
    let l = Layout::new();
    l.dojo_ok(&["new-branch", "notes", "--now", "-m", "notes"]);
    l.write("todo.txt", "secret\n");
    let r = l.dojo_json(&["push"], 2);
    assert_eq!(r["refusal"]["kind"], "private-paths", "{r:#}");
    let paths = r["refusal"]["paths"].as_array().unwrap();
    assert_eq!(paths[0]["path"], "todo.txt");
    assert!(r["message"].as_str().unwrap().contains("must not be committed"));
    // AGENTS.md is excluded via .git/info/exclude and never committed.
    let files = l.jj(&["file", "list", "--ignore-working-copy"]);
    assert!(!files.contains("AGENTS.md"), "{files}");
}

#[test]
fn new_branch_creates_bookmark_on_first_push() {
    let l = Layout::new();
    let r = l.dojo_json(&["new-branch", "feat/a"], 0);
    assert_eq!(commands(&r), vec!["jj new 'trunk()'"]);
    let b = l.dojo_json(&["branches"], 0);
    assert!(b["rows"].as_array().unwrap().iter().all(|r| r["name"] != "feat/a"));
    // status knows the pending name.
    l.write("a.txt", "a\n");
    let s = l.dojo_json(&["status"], 0);
    assert_eq!(s["bookmark"], "feat/a");
    assert_eq!(s["bookmark_at"], "pending");
    assert_eq!(s["next"], "dojo push feat/a");

    let p = l.dojo_json(&["push", "-m", "add a"], 0);
    assert_eq!(p["outcome"], "pushed");
    assert!(commands(&p).contains(&"jj bookmark create feat/a --revision @".to_string()));
    assert!(!l.bare_head("sundae.git", "feat/a").is_empty());

    // --now creates at once, --from picks the base.
    let r = l.dojo_json(&["new-branch", "feat/b", "--now", "--from", "fix/goals@origin"], 0);
    assert_eq!(
        commands(&r),
        vec![
            "jj new fix/goals@origin",
            "jj bookmark create feat/b --revision @"
        ]
    );
    assert_eq!(l.commit_of("@-"), l.commit_of("fix/goals@origin"));
    // Existing names are refused.
    let out = l.dojo(&["new-branch", "fix/goals"]);
    assert!(!out.status.success());
}

#[test]
fn status_suggests_next_command() {
    let l = Layout::new();
    l.dojo_ok(&["switch", "fix/goals"]);
    l.append("goals.txt", "x");
    let s = l.dojo_json(&["status"], 0);
    assert_eq!(s["bookmark"], "fix/goals");
    assert_eq!(s["bookmark_at"], "@-");
    assert_eq!(s["next"], "dojo push fix/goals");
    let text = l.dojo_ok(&["status"]);
    assert!(text.contains("@sundae (push)"), "{text}");
    assert!(text.contains("Next: dojo push fix/goals"), "{text}");

    l.dojo_ok(&["push", "-m", "x"]);
    l.outside_push("fix/goals", "goals.txt", "y");
    l.dojo_ok(&["fetch"]);
    let s = l.dojo_json(&["status"], 0);
    assert_eq!(s["next"], "dojo sync fix/goals", "{s:#}");
}

#[test]
fn dry_run_prints_commands_and_changes_nothing() {
    let l = Layout::new();
    let before = l.jj(&["op", "log", "--no-graph", "-T", "id ++ \"\\n\"", "--limit", "1"]);
    let out = l.dojo(&["--dry-run", "switch", "fix/goals"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("jj bookmark track fix/goals --remote sundae"), "{stdout}");
    assert!(stdout.contains("jj new fix/goals"), "{stdout}");
    let after = l.jj(&["op", "log", "--no-graph", "-T", "id ++ \"\\n\"", "--limit", "1"]);
    assert_eq!(before, after, "dry run must not create operations");
}

#[test]
fn pr_dry_run_uses_head_for_gh_and_source_branch_for_glab() {
    let l = Layout::new();
    l.dojo_ok(&["switch", "fix/goals"]);
    l.append("goals.txt", "pr");
    l.dojo_ok(&["push", "-m", "Fix the goals"]);
    l.git(&l.work, &["remote", "set-url", "origin", "https://github.com/acme/widgets.git"]);
    let r = l.dojo_json(&["--dry-run", "pr"], 0);
    let cmds = commands(&r);
    let gh = cmds.iter().find(|c| c.starts_with("gh ")).expect("gh step");
    assert!(
        gh.starts_with("gh pr create --repo acme/widgets --head fix/goals --title 'Fix the goals'"),
        "{gh}"
    );
    l.git(&l.work, &["remote", "set-url", "origin", "git@gitlab.com:grp/proj.git"]);
    let r = l.dojo_json(&["--dry-run", "pr", "fix/goals", "--checks"], 0);
    assert_eq!(
        commands(&r),
        vec!["glab ci status --branch fix/goals --repo grp/proj"]
    );
    let r = l.dojo_json(&["--dry-run", "pr", "fix/goals"], 0);
    let cmds = commands(&r);
    assert!(
        cmds.iter()
            .any(|c| c.starts_with("glab mr create --repo grp/proj --source-branch fix/goals")),
        "{cmds:?}"
    );
}

#[test]
fn missing_identity_shows_one_fix() {
    let l = Layout::with_identity(false);
    let out = l.dojo(&["switch", "fix/goals"]);
    assert!(out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(err.matches("jj config set --user user.name").count(), 1, "{err}");
    assert!(err.contains("jj config set --user user.email"), "{err}");
}

#[test]
fn without_sundae_remote_dojo_behaves_like_plain_jj() {
    let l = Layout::new();
    l.jj(&["git", "remote", "remove", "sundae"]);
    let forge = l.root.join("forge.git");
    l.git(
        &l.work,
        &["remote", "set-url", "--push", "origin", forge.to_str().unwrap()],
    );
    l.dojo_ok(&["switch", "fix/goals"]);
    l.append("goals.txt", "plain");
    let r = l.dojo_json(&["push", "-m", "plain"], 0);
    assert!(commands(&r).contains(&"jj git push --remote origin --bookmark fix/goals".to_string()));
    assert_eq!(l.bare_head("forge.git", "fix/goals"), l.commit_of("fix/goals"));
}
