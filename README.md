# dojo — a friendly companion for jj

`dojo` is a TUI and CLI for [Jujutsu (jj)](https://jj-vcs.github.io). It helps you with the daily bookmark work (fetch, switch, push, sync, pull requests) and with jj workspaces. It runs on Linux and macOS.

Every dojo command is a short list of jj commands. dojo prints each command before it runs (`$ jj ...`), and `--dry-run` prints them without a run. The TUI shows the commands in a preview before you confirm. So dojo is also a way to learn jj: you see what each step does on your own repository.

> [!NOTE]
> **dojo is a fork of [daeh/jujutsu-gi](https://github.com/daeh/jujutsu-gi)** (`ji`, by Dae Houlihan, MIT license). The workspace commands (`dojo ws ...`), the workspace TUI, the configuration, hooks, and the shell integration come from `ji`. dojo adds the bookmark workflows, the home TUI, Linux support, and the [Sundae](#sundae-sessions) integration. Thank you to the `ji` authors.

## Contents

- [Requirements](#requirements)
- [Install](#install)
- [First use](#first-use)
- [Bookmark workflows](#bookmark-workflows)
- [The TUI](#the-tui)
- [Sundae sessions](#sundae-sessions)
- [Workspaces (`dojo ws`)](#workspaces-dojo-ws)
- [Configuration](#configuration)
- [Documentation](#documentation)
- [License](#license)

## Requirements

- [jj](https://jj-vcs.github.io) v0.42.0 or later (CI tests the latest release and 0.47).
- For `dojo pr`: [`gh`](https://cli.github.com) (GitHub) or [`glab`](https://gitlab.com/gitlab-org/cli) (GitLab).

## Install

Linux: download a static binary (x86_64 or aarch64) and `SHA256SUMS` from the [releases page](https://github.com/danielboothcloud/dojo/releases), check it with `sha256sum --check --ignore-missing SHA256SUMS`, and put `dojo` on your `PATH`.

From source (Linux and macOS):

```sh
cargo install --locked --git https://github.com/danielboothcloud/dojo dojo
```

For the workspace commands, install the shell wrapper, so that `dojo ws switch` can change your shell's directory:

```sh
dojo config shell install
```

Details, macOS notes, and uninstall: [docs/installation.md](docs/installation.md).

## First use

```sh
cd /path/to/your/jj-repo
dojo status            # where am I, and what next?
dojo                   # open the TUI
```

A typical day:

```sh
dojo fetch                       # what is new on the remotes?
dojo branches                    # all bookmarks, local and per remote
dojo switch fix/goals            # work on a bookmark
# ... edit files ...
dojo push                        # describe, move the bookmark, push
dojo pr                          # open a pull request
```

New work:

```sh
dojo new-branch feat/login       # new change on trunk(); the bookmark comes with the first push
# ... edit files ...
dojo push -m "Add the login form"
```

## Bookmark workflows

The table shows the jj commands that each command runs. Brackets mark commands that run only when they are necessary. `<push>` is the push remote (`sundae` if it exists, else `origin`). `<forge>` is the upstream remote (`origin`).

| Command | What it does | jj commands |
|---|---|---|
| `dojo fetch` | Fetch every remote. Show new, moved, and deleted bookmarks per remote. | `jj git fetch --remote origin --remote sundae` |
| `dojo branches` (`b`) | One row per bookmark: local, each remote, tracking, ahead/behind, protected. | read only (`jj bookmark list --all-remotes`) |
| `dojo switch <b>` | Start work on a bookmark. Your edits stay in the old change. | [`jj git fetch ...`] [`jj bookmark track <b> --remote <push>`] `jj new <b>` |
| `dojo push [<b>]` | Push `@` (or `@-` when `@` is empty) with its bookmark. | [`jj describe <id> --message ...`] [`jj bookmark set <b> --revision @`] `jj git push --remote <push> --bookmark <b>` [`jj new <b>`] [`jj git fetch --remote origin`] |
| `dojo sync [<b>]` | Bring the forge's new commits into your work, then offer the push. | `jj git fetch ...` `jj rebase --branch <b> --destination <b>@<forge>` |
| `dojo new-branch <b>` | New change on `trunk()`. The bookmark is created on the first push (or at once with `--now`). | `jj new 'trunk()'` [`jj bookmark create <b> --revision @`] |
| `dojo status` (`st`) | The current change, its bookmark, the remote state, and the next command. | read only |
| `dojo pr [<b>]` | Push, then open a pull request (`gh`) or merge request (`glab`). `--checks` shows CI. | the push commands, then `gh pr create --repo o/r --head <b> ...` or `glab mr create --repo g/p --source-branch <b> ...` |

Global flags: `--json` (a JSON report with every step, for agents and scripts), `--dry-run` (print the commands, change nothing), `-y`/`--yes` (run an offered follow-up without a question). Exit code 2 means that dojo stopped and needs a decision (for example a refused push). Full reference: [docs/cli.md](docs/cli.md).

## The TUI

`dojo` with no command opens the home view:

- the **log** graph (left) and the **bookmarks** (right, local and per remote), side by side on a wide terminal, one at a time (`Tab`) on a narrow one;
- `Enter` on a bookmark opens the **bookmark view**: its state on each remote and the log of its changes;
- `Enter` on a change opens the **change detail**: the description, the changed files, and the diff of each file with line numbers and the changed words marked (`J`/`K` file, `]`/`[` hunk);
- one key per action: `f` fetch, `s` switch, `p` push, `y` sync, `P` pull request, `d` describe, `n` new, `e` edit, `S` squash, `a` abandon, `x` split, `u` undo, `o` operation log, `w` workspaces, `?` help.

Each action first shows a **preview** of the exact commands; `Enter` runs them. When dojo has a follow-up (for example "sync, then push" after a refused push), the result box shows it, and `Enter` runs it. Full reference: [docs/tui.md](docs/tui.md).

## Sundae sessions

A Sundae session is a Linux container in which a coding agent works on repositories. Each repository in a session has two git remotes:

- `sundae`: the Sundae server. You fetch from it and push to it. Sundae forwards each pushed bookmark to the forge in the same push.
- `origin`: the forge (GitHub or GitLab). Fetch only. A push to `origin` fails on purpose.

dojo knows this layout:

- **Explicit remotes.** The per-repo jj config (which sets `git.push = "sundae"`) is not always visible to the shell, so a plain `jj git push` can pick the wrong remote. dojo never depends on `git.push` or `git.fetch`. It reads the remotes with `jj git remote list` and always passes `--remote`. It never pushes to `origin` when `sundae` exists.
- **Forwarding.** After a push to `sundae`, dojo fetches `origin` to show the bookmark that Sundae forwarded.
- **Protected `main`.** Sundae refuses pushes to `main`. dojo stops before the push and offers to push the same change as a new bookmark (`dojo push --change <rev>` or `dojo push --as <name>`). Then open a pull request from it.
- **"forge `<b>` has commits you lack".** Someone pushed to the forge from outside the session. dojo explains the refusal and offers `dojo sync <b>`: fetch, rebase your commits onto `<b>@origin`, then push. With `--yes`, `dojo sync <b> --yes` does all of it in one command.
- **Private paths.** Sundae refuses commits that add `AGENTS.md`, `todo.txt`, `.pi/`, `_bmad/`, or `_bmad-output/`. These files live in `refs/sundae/private`. dojo lists the files and the commits that add them. dojo never touches `refs/sundae/private`.
- **Pushed commits stay as they are.** Sundae refuses a rewritten commit. So after a push of the working-copy change, dojo starts a new empty change on top of it.
- **Pull requests.** jj leaves git `HEAD` detached, so dojo always passes the branch: `gh pr create --head <b>` or `glab mr create --source-branch <b>`.
- **Identity.** When jj does not know your name and email, dojo says so once and shows the fix: `jj config set --user user.name "Your Name"` and `jj config set --user user.email "you@example.com"`.

dojo also works in a plain repository with only `origin`: then `origin` is the push remote.

## Workspaces (`dojo ws`)

![The workspace TUI](docs/assets/ji-create.jpg)

The workspace commands come from `ji`. A jj workspace is like a git worktree.

```
dojo ws tui                          # the workspace TUI (also `w` in the home view)
dojo ws switch <target>              # switch to a workspace (by name, bookmark, or change ID)
dojo ws new <bookmark>               # create a new workspace
dojo ws close [target]               # close a workspace into a target
dojo ws transfer [target]            # transfer changes between workspaces (both stay open)
dojo ws sync [target]                # sync two workspaces
dojo ws ls                           # list workspaces
```

They create, switch, sync, merge, and clean up workspaces, with per-project hooks and file templates, graph-aware methods (fast-forward, merge, rebase, squash), undo/redo of dojo actions, and (on macOS) Finder-metadata fidelity. The close and transfer dialogs show the jj command sequence before you confirm. See [docs/operations.md](docs/operations.md) and [docs/cli.md](docs/cli.md#workspace-commands-dojo-ws).

## Configuration

Project config: `.config/dojo.toml` in the repository root (generate it with `dojo init`). User config: `$XDG_CONFIG_HOME/dojo/config.toml` (default `~/.config/dojo/config.toml`). The project file overrides the user file.

```toml
# Bookmarks that the push remote refuses (default: ["main"]).
protected = ["main"]
# Remotes (defaults: sundae, else origin; and origin).
# push-remote = "sundae"
# upstream-remote = "origin"

# Workspaces: where `dojo ws new` creates them, and their hooks.
workspace-path = "../{{ repo }}.{{ bookmark }}"

[pre-start]
deps = "uv sync"
```

Full reference: [docs/configuration.md](docs/configuration.md).

## Documentation

| Page | What it covers |
|---|---|
| [docs/installation.md](docs/installation.md) | Platforms, release binaries, build from source, uninstall |
| [docs/cli.md](docs/cli.md) | Every command and flag, with the jj commands it runs |
| [docs/tui.md](docs/tui.md) | The home view and the workspace view |
| [docs/configuration.md](docs/configuration.md) | Full `.config/dojo.toml` reference |
| [docs/shell-integration.md](docs/shell-integration.md) | The shell wrapper for the workspace commands |
| [docs/operations.md](docs/operations.md) | Workspace sync, transfer, close: diagrams and command sequences |
| [docs/xattrs.md](docs/xattrs.md) | macOS Finder aliases, xattrs, and hard links across workspace operations |
| [docs/troubleshooting.md](docs/troubleshooting.md) | Problem / solution pairs |

## License

MIT. See [LICENSE](LICENSE). dojo keeps the copyright notice of the original `ji` project.
