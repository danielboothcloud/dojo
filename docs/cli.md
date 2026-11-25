# CLI reference

Every dojo subcommand, its flags, and its behavior.

> dojo commands must be run from inside a [jj](https://jj-vcs.github.io) repository. Most subcommands expect to find a `.jj/` directory somewhere in the ancestor path.

## Terms used on this page

A few terms appear throughout. Full definitions live in [operations.md](operations.md#terminology); brief versions:

- **Default workspace** — the workspace at the repository root, referred to as `default@`. It serves the role of the `main`/`master` branch in git. dojo uses it as the default target for `transfer`/`sync` (and for `close` only when `--source` is given) when no positional target is supplied. A bare non-disposal `dojo ws close` requires an explicit target.
- **Effective head** — the tip of a workspace's line of work, skipping over dojo-inserted empty "trivial" commits used to keep workspaces from sharing `@`.
- **Adaptive** — a meta-method for close/transfer that inspects the graph relationship between source and target and picks a concrete method (fast-forward, merge, etc.).
- **Disposal methods** — close methods that do not require a target: `detach` and `abandon`.

## Commands

Bookmark workflows:

- [`dojo`](#dojo) — open the TUI (home view)
- [`dojo fetch`](#dojo-fetch) — fetch every remote and summarize the changes
- [`dojo branches`](#dojo-branches) — every bookmark, local and per remote
- [`dojo switch`](#dojo-switch) — start working on a bookmark
- [`dojo push`](#dojo-push) — push a bookmark to the push remote
- [`dojo sync`](#dojo-sync) — rebase a bookmark onto the forge's version
- [`dojo new-branch`](#dojo-new-branch) — start new work on a new bookmark
- [`dojo status`](#dojo-status) — where you are and the next command
- [`dojo pr`](#dojo-pr) — open a pull request or merge request

Workspace commands (the original `ji` commands):

- [`dojo ws switch`](#dojo-ws-switch) — switch to a workspace
- [`dojo ws new`](#dojo-ws-new) — create a workspace
- [`dojo ws close`](#dojo-ws-close) — close a workspace into a target
- [`dojo ws transfer`](#dojo-ws-transfer) — transfer changes between workspaces
- [`dojo ws sync`](#dojo-ws-sync) — sync two workspaces
- [`dojo ws ls`](#dojo-ws-ls--dojo-ws-list) — list workspaces
- `dojo ws tui` — the workspace TUI (see [tui.md](tui.md#workspace-view-dojo-ws-tui))
- [`dojo init`](#dojo-init) — generate project config
- [`dojo config`](#dojo-config) — configuration subcommands

## Global flags

| Flag | Effect |
|---|---|
| `--json` | Print a JSON report on stdout (for agents and scripts). The report has a `steps` list with each command, its result and jj's output. |
| `--dry-run` | Print the commands on stdout and run nothing. |
| `-y`, `--yes` | Run an offered follow-up without a question (never for `dojo pr`). |

The bookmark workflows print each jj command on stderr (`$ jj ...`) before it runs, then jj's own output, indented. They never use `git checkout`, and they never depend on jj's `git.push` / `git.fetch` settings: dojo finds the remotes with `jj git remote list` and always passes `--remote`.

### Remote roles

| Role | Default | Used by |
|---|---|---|
| push remote | `sundae` if it exists, else `origin`, else the only remote | `push`, `switch` (tracking) |
| fetch remotes | all remotes | `fetch`, `switch`, `sync` |
| upstream (forge) | `origin` if it exists, else the push remote | `sync`, `pr`, `status` |

Change the defaults with `push-remote` and `upstream-remote` in [configuration.md](configuration.md#push-remote--string-optional).

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Done (also "already up to date") |
| 1 | Error (jj failed, bad argument, not a jj repo) |
| 2 | dojo stopped and needs you: a refused push, a change without a bookmark, or conflicts after a sync. The message and the JSON `offer` give the next command. |

### Offers

After a refusal or a sync, dojo offers one follow-up (for example `dojo sync fix/goals` after a "forge has commits you lack" refusal). On a terminal, dojo asks `Run it now? [Y/n]`. With `--yes` it runs the follow-up at once. Otherwise it prints `Next: <command>` and the jj commands of that follow-up.

---

## `dojo`

```
dojo
```

Opens the TUI home view: bookmarks, the log, the change detail, and keys that run jj commands after a preview. See [tui.md](tui.md). `dojo ws tui` (or `w` in the home view) opens the workspace TUI.

Errors if the current directory is not inside a jj repository. In a repo without git remotes, dojo opens the workspace TUI.

---

## `dojo fetch`

```
dojo fetch
```

Runs:

```
jj git fetch --remote <remote> [--remote <remote> ...]     # every remote
```

Then prints, per remote, the bookmarks that are new, moved or deleted, and the local bookmarks that moved or became conflicted. dojo compares the bookmark state before and after the fetch to make the summary.

---

## `dojo branches`

```
dojo branches          # alias: dojo b
```

Read only. One row per bookmark: the local change, the change on each remote (`@sundae`, `@origin`), the remotes that the local bookmark tracks, ahead/behind per remote (`=` same, `+2` two local commits that the remote lacks, `-1` one remote commit that is not local, `+2-1` diverged), and notes (`protected`, `conflicted`, `remote only`). dojo reads `jj bookmark list --all-remotes` with a template, and counts commits with `jj log` revsets.

---

## `dojo switch`

```
dojo switch <bookmark>
```

Start working on a bookmark. Runs, as needed:

```
jj git fetch --remote ... --remote ...       # only if no local or remote bookmark has this name
jj bookmark track <bookmark> --remote <r>    # only if the local bookmark is missing or untracked
jj new <bookmark>                            # a new change on top of the bookmark
```

`<r>` is the push remote if it has the bookmark, else the only remote that has it, else the forge. Edits in the old working-copy change stay in that change; dojo prints its change id and `jj edit <id>` to go back. If `@` is already an empty change on top of the bookmark, dojo does nothing. This is a bookmark switch in the current directory. For workspaces, use `dojo ws switch`.

---

## `dojo push`

```
dojo push [<bookmark>] [-m <message>] [--change <rev>] [--as <name>]
```

Push your work. The change to push is `@`, or `@-` when `@` has no file changes. The bookmark is, in this order: the argument; a non-protected bookmark on `@` or `@-`; a pending name from `dojo new-branch`; a protected bookmark on `@` or `@-`.

Runs, as needed:

```
jj describe <id> --message <message>             # jj does not push changes without a description
jj bookmark create <b> --revision @              # a pending name from dojo new-branch
jj bookmark set <b> --revision @                 # move the bookmark forward to your latest change
jj bookmark track <b> --remote <push remote>     # an existing, untracked remote bookmark
jj git push --remote <push remote> --bookmark <b>
jj new <b>                                       # only when @ was the pushed change
jj git fetch --remote origin                     # only with a sundae remote (see below)
```

- **Description.** With `-m`, dojo uses that message. On a terminal without `-m`, dojo asks (Enter accepts `Update <files>`). Without a terminal, dojo uses `Update <files>`.
- **No bookmark.** dojo stops (exit 2) and offers `dojo push --change <rev>` (`jj git push --change`, a generated `push-*` name). Use `--as <name>` to choose the name.
- **After the push.** When the pushed change was the working copy, dojo starts a new empty change on top of it. Sundae refuses rewritten commits, so do not amend a pushed change. With a `sundae` remote, dojo then fetches `origin` to show the bookmark that Sundae forwarded to the forge.
- **Refusals.** See [Sundae refusals](#sundae-refusals).

---

## `dojo sync`

```
dojo sync [<bookmark>]
```

Bring the forge's new commits into your work. Runs:

```
jj git fetch --remote ... --remote ...
jj rebase --branch <b> --destination <b>@<upstream>     # your commits on top of the forge's commits
```

When the bookmark has no commits of its own, dojo moves it forward instead (`jj bookmark set <b> --revision <b>@<upstream>`) and rebases a working-copy change on top of it (`jj rebase --branch @ --destination <b>@<upstream>`). When the fetch left the bookmark conflicted, dojo rebases your side and points the bookmark at it (`jj bookmark set <b> --revision <id> --allow-backwards`). If the rebase makes conflicts, dojo lists the changes and how to resolve them, and exits 2. Otherwise it offers `dojo push <b>`.

---

## `dojo new-branch`

```
dojo new-branch <name> [--from <rev>] [--now] [-m <message>]
```

Start new work. Runs `jj new 'trunk()'` (or `jj new <rev>` with `--from`). With `--now`, dojo also runs `jj bookmark create <name> --revision @`. Without `--now`, dojo stores the name in `.jj/dojo-pending.json`, and the first `dojo push` creates the bookmark on the change to push. dojo refuses a name that exists locally or on a remote.

---

## `dojo status`

```
dojo status          # alias: dojo st
```

Read only. Shows the working-copy change and its parent, where the current bookmark is (`@`, `@-`, `pending`, or an older change), its state on each remote, changes with conflicts, a missing jj identity, and the next command (for example `dojo push fix/goals` or `dojo sync fix/goals`).

---

## `dojo pr`

```
dojo pr [<bookmark>] [--title <t>] [--body <b>] [--base <branch>] [--draft] [--checks]
```

Push the bookmark (see `dojo push`), then open a pull request on the forge (the upstream remote):

```
gh pr create --repo <owner/repo> --head <b> --title <t> --body <b> [--base <branch>] [--draft]
glab mr create --repo <group/project> --source-branch <b> --title <t> --description <b> --yes [--target-branch <branch>] [--draft]
```

dojo uses `gh` for github.com URLs and `glab` for gitlab.com URLs. `--head` / `--source-branch` is necessary because jj leaves git `HEAD` detached. Default title: the description of the only commit on the bookmark (not on `trunk()`), or the newest commit title when there are more; the body lists the commit titles. `--checks` shows CI status instead (`gh pr checks <b> --repo ...`, `glab ci status --branch <b> --repo ...`). dojo never runs `pr` as an automatic follow-up.

---

## Sundae refusals

dojo reads the `remote: sundae:` lines of a refused push and explains them:

| Refusal | What dojo does |
|---|---|
| `<b> is protected` | Offers to push the same change as a new bookmark (`dojo push --change <rev>`, or `--as <name>`). dojo also stops before the push for the names in `protected`. |
| `forge <b> has commits you lack` | Someone pushed to the forge from outside. Offers `dojo sync <b>`, which fetches, rebases your commits onto `<b>@origin`, and then offers the push. |
| private paths (`AGENTS.md`, `todo.txt`, `.pi/`, `_bmad/`, ...) | Lists the files and the commits that add them. Remove them from those commits, then push again. |

---

# Workspace commands (`dojo ws`)

The commands below manage jj workspaces. They are the commands of the original `ji` tool, under `dojo ws`.

---

## `dojo ws switch`

```
dojo ws switch <target>
```

Switch to an existing workspace. For each workspace, dojo checks whether `<target>`:

- exactly matches the workspace name, or
- is a prefix of the workspace's change ID, or
- matches any bookmark on the workspace (at the head or behind it)

All workspaces that satisfy any of those conditions are collected. If exactly one matches, dojo switches to it. If zero match, dojo errors with `no workspace matching '<target>'`. If more than one matches, dojo errors with the ambiguous candidate list.

On success, dojo writes a `cd <path>` directive for the [shell wrapper](shell-integration.md), which moves the parent shell into the target workspace directory.

---

## `dojo ws new`

```
dojo ws new <bookmark> [--revision <REVSET>] [--path <PATH>] [--message <MSG>] [--create-if-necessary]
```

Create a new workspace and switch to it.

### Arguments

- **`<bookmark>`** — required. The bookmark name for the new workspace. This name drives:
  - the `{{ bookmark }}` variable in the `workspace-path` template
  - a bookmark dojo creates pointing at the new workspace's `@`
  - forward slashes are replaced with hyphens when used in paths

### Flags

| Flag | Type | Default | Description |
|---|---|---|---|
| `-r`, `--revision <REVSET>` | [jj revset](https://docs.jj-vcs.dev/latest/revsets/) resolving to one revision | `@` | Revision the new workspace branches from |
| `-p`, `--path <PATH>` | string | — | Override the `workspace-path` template; path is relative to the repo root |
| `-m`, `--message <MSG>` | string | — | Description for the new workspace's working-copy commit |
| `--create-if-necessary` | flag | off | If a workspace already matches `<bookmark>`, switch to it instead of erroring |

### Branching behavior

If the revision passed to `--revision` is a head commit with real content ("non-trivial"), dojo inserts a new empty commit on the source workspace before branching so that the source and the new workspace do not share the same `@`. Branching from an empty (trivial) head skips that step-forward.

### Side effects

After branching, dojo:

1. Creates a jj bookmark named `<bookmark>` pointing at the new workspace's `@`
2. Writes any files defined in `[templates]` ([see configuration](configuration.md))
3. Runs `[pre-start]` hooks sequentially, blocking on each
4. Spawns `[post-start]` hooks in the background
5. Writes a `cd` directive for the shell wrapper

If `dojo-author` is set in the config, the new revision's author is rewritten via `jj metaedit --author`.

---

## `dojo ws close`

```
dojo ws close [target] [--source <NAME>] [--method <METHOD>] [--delete-files]
```

Close a workspace — integrating its work into a target, or (for the `detach`/`abandon` disposal methods) discarding it — and then forgetting it.

### Arguments and flags

| Argument / Flag | Required | Default | Description |
|---|---|---|---|
| `[target]` | yes, for non-disposal methods (unless `--source` is given) | `default@`, only with `--source` | Target workspace name (positional) |
| `--source <NAME>` | no | current workspace (by cwd) | Workspace to close |
| `--method <METHOD>` | no | `adaptive` | Close method (see table below) |
| `--delete-files` | no | off | Remove the source workspace directory after close |

> **The `default` workspace can't be closed.** `dojo ws close` refuses to close the default workspace (whether as the cwd source or via `--source default`), mirroring the TUI — closing it would remove the repo root.

### Methods

| Method | Target required | Description |
|---|---|---|
| `adaptive` | yes | Picks `fast-forward` or `merge` based on sync state — see [operations.md](operations.md#close-adaptive) |
| `merge` | yes | Creates a merge commit whose parents are the effective heads of both workspaces |
| `squash-merge` | yes | Squashes the source chain into one commit, then merges |
| `fast-forward` | yes | Fast-forwards the target to the source's effective head |
| `detach` | no | Forgets the workspace, keeping meaningful and explicitly bookmarked revisions while removing unreferenced trivial heads |
| `abandon` | no | Forgets the workspace and abandons all its revisions |

For full diagrams and the jj command sequences dojo runs for each method, see [operations.md](operations.md).

> **Bookmark handling.** When you close a workspace from the CLI, bookmarks on the source workspace are left alone — dojo does not advance or delete them. The singular bookmark (the one whose name matches the workspace's `{{ bookmark }}` value) is still managed automatically. If you want to advance or delete the other bookmarks as part of a close, use the TUI close dialog (`x`), which exposes a per-close bookmark-action toggle. See [operations.md](operations.md#bookmark-actions-on-close).

### Example

```sh
# Close the current workspace into the default workspace with adaptive merge
dojo ws close default

# Squash-merge a specific workspace into another, deleting its files
dojo ws close main --source feature-login --method squash-merge --delete-files

# Abandon a workspace and its revisions
dojo ws close --method abandon
```

---

## `dojo ws transfer`

```
dojo ws transfer [target] [--source <NAME>] [--method <METHOD>]
```

Transfer changes between two workspaces. Both workspaces remain open afterward — this is the difference from `dojo ws close`.

### Arguments and flags

| Argument / Flag | Required | Default | Description |
|---|---|---|---|
| `[target]` | no | `default@` | Target workspace name |
| `--source <NAME>` | no | current workspace (by cwd) | Source workspace |
| `--method <METHOD>` | no | `adaptive` | Transfer method (see table below) |

### Methods

| Method | Description |
|---|---|
| `adaptive` | Picks `fast-forward-target`, `fast-forward-source`, or `merge` based on sync state |
| `merge` | Merge commit with both effective heads as parents; both workspaces step forward past the merge |
| `fast-forward-target` | Fast-forward the target to the source's effective head |
| `fast-forward-source` | Fast-forward the source to the target's effective head |
| `rebase` | Rebase the source's unique chain onto the target — produces linear history |
| `merge-squash` | Squash the source's chain into one commit, then merge with target |

Full diagrams and jj command sequences: [operations.md](operations.md).

---

## `dojo ws sync`

```
dojo ws sync [target] [--source <NAME>]
```

Converge two workspaces. If one side is behind the other on the graph, dojo fast-forwards it. If both have new work, dojo creates a merge commit (with both effective heads as parents) that becomes the new tip of both workspaces.

Unlike `dojo ws close` and `dojo ws transfer`, `dojo ws sync` does **not** take a `--method` flag — the strategy is always decided from the sync state.

If the two workspaces are already in sync, dojo prints `(dojo)::sync already in sync` and exits 0.

Defaults: `target` is `default@`; `--source` is the workspace at the current working directory.

---

## `dojo ws ls` / `dojo ws list`

```
dojo ws ls
```

Prints one line per workspace:

```
@ default              abcd1234   main, (feature)   (no description)
+ feature-login        efgh5678   login              wip: login UI
```

Columns:

- **Gutter** — `@` = current workspace, `+` = other
- **Name** — workspace name
- **Change ID** — 8-character prefix of the working-copy commit's change ID
- **Bookmarks** — bookmarks at the workspace's head, followed by bookmarks behind the head in parentheses
- **Description** — first line of the working-copy commit's description, truncated to 60 chars

---

## `dojo init`

```
dojo init
```

Generates `.config/dojo.toml` in the repo root from the annotated template. Errors if the file already exists.

See [configuration.md](configuration.md) for the full field reference.

---

## `dojo config`

```
dojo config init                       # same as dojo init
dojo config shell init [SHELL]         # print shell integration to stdout
dojo config shell install [SHELL]      # install shell integration
dojo config shell uninstall [SHELL]    # remove shell integration
dojo config shell status [SHELL]       # report integration state
```

`[SHELL]` is one of `zsh`, `bash`, `fish`; it defaults to the active shell — the one that invoked `dojo`, found by walking the process tree — falling back to `$SHELL` when no shell can be identified.

### `dojo config init`

Alias for `dojo init`.

### `dojo config shell init [SHELL]`

Prints the shell wrapper function to stdout.

### `dojo config shell install [SHELL]`

Installs the shell wrapper. Idempotent. On a terminal it previews the changes and prompts `[y/N/?]` before writing (`?` re-shows the diff); a non-interactive run (piped/CI) writes directly. With no `[SHELL]` it targets the active shell; `--all` instead installs for **every shell with an existing config** (skipping shells with none, reported per shell). Flags: `--all`, `--dry-run` (print the diff without writing), `--force` (install even if integration is detected elsewhere, or overwrite non-dojo-managed files), `--yes`/`-y` (skip the prompt). `--all` and an explicit `[SHELL]` are mutually exclusive. For fish, when a dynamic completion is already provided on `$fish_complete_path` (e.g. Homebrew's `vendor_completions.d/dojo.fish`), install relies on it and removes a redundant/stale user-directory copy instead of writing one that would shadow it. See [shell-integration.md](shell-integration.md) for target locations and manual install.

When `dojo ws switch`/`new`/`close` can't change the directory because the wrapper isn't installed, on a terminal it offers to install integration right there (`[y/N/?]`); declining is remembered per shell.

### `dojo config shell uninstall [SHELL]`

Removes the wrapper files and rc stanza that `install` added. Flags: `--dry-run`, `--force` (also remove non-dojo-managed files).

### `dojo config shell status [SHELL]`

Reports install state, drift, any cross-file integration hits, and any bypass aliases — a `dojo` alias (or fish `function dojo`) that shadows the wrapper, or a differently-named alias that runs the `dojo` binary directly. For fish it also reports whether the user completion is shadowing (or being shadowed by) another `dojo.fish` on `$fish_complete_path`, and how to resolve it.

---

## Source and target resolution

`close`, `transfer`, and `sync` share the same source/target resolution rules:

1. **Source**
   - With `--source <NAME>`: looked up by exact workspace name.
   - Without `--source`: the workspace whose path matches the current working directory. Errors if cwd is not inside a workspace.
2. **Target**
   - With a positional `target`: looked up by exact workspace name.
   - Without a positional `target`: the default workspace (`default@`) — the workspace at the repository root, which serves the role of the `main`/`master` branch in git. Errors if it can't be found. **Exception:** for `close`, this fallback applies only when `--source` is given; a bare non-disposal `dojo ws close` (no `target`, no `--source`) errors and asks for a target.

## Stale working copy detection

Before a mutating operation, dojo detects whether the working copy is stale (another process modified the repo). On a stale error, dojo prints:

```
(dojo)::stale working copy is stale — another tool modified the repo.
Fix with: jj workspace update-stale
```

and exits 1. See [troubleshooting.md](troubleshooting.md).
