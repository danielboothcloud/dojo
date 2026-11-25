# TUI reference

dojo has two TUI views:

- The **home view** opens with `dojo` (no subcommand). It shows bookmarks and the log, and runs the bookmark workflows and common jj commands.
- The **workspace view** opens with `dojo ws tui`, or with `w` in the home view. It is the workspace switcher of the original `ji` tool.

Start both from any directory inside a [jj](https://jj-vcs.github.io) repository.

## Home view (`dojo`)

### Layout

- A header line: the repo name, the working-copy change (`@`), the current bookmark, and the push remote.
- The **bookmarks pane**: one row per bookmark. The columns are the local change, one column per remote (`@sundae`, `@origin`), and notes. A remote column shows ahead/behind against the local bookmark (`=`, `+2`, `-1`, `+2-1`), or the remote's change id when there is no local bookmark. A dim cell means that the local bookmark does not track that remote.
- The **log pane**: the `jj log` graph with jj's colours (or your `log-template`). `▶` marks the selected change.
- A status line and a line of key hints.

From 100 columns, the two panes are side by side. On a narrower terminal, dojo shows one pane at a time; `Tab` switches.

### Command preview

Every key that changes something first opens a preview box. The box lists the exact commands (`$ jj ...`) and why dojo runs each one. For the bookmark workflows the preview is a dry run of the same code that `dojo push`, `dojo sync`, and the other commands use. Press `Enter` to run, or `Esc` to cancel. If the action cannot run now (for example a push of a bookmark that exists only on the remote), the box says why.

After the run, a result box shows each command, jj's output, and dojo's message. If dojo has a follow-up (for example `dojo sync` after a "forge has commits you lack" refusal, then the push after the sync), the result box shows it with its commands. Press `Enter` to run it.

Commands that need the terminal (`jj describe`, `jj squash` when jj asks for a description, `jj split`) run outside the TUI. dojo gives the terminal to jj and comes back when jj ends.

### Keys

| Key | Action |
|---|---|
| `Tab`, `←`, `→` | Switch between the bookmarks pane and the log pane (`h` bookmarks, `l` log) |
| `j` `k` `↑` `↓` | Move the selection (`g` / `G`: first / last) |
| `Enter` | Change detail: description, files, and the diff (with colours). `j`/`k`, `Space`/`PgUp`, `g`/`G` scroll; `Esc` goes back |
| `f` | Fetch from all remotes (`dojo fetch`) |
| `s` | Switch to the selected bookmark (`dojo switch`) |
| `p` | Push the selected bookmark; in the log pane, push the current work (`dojo push`) |
| `y` | Sync the selected bookmark with the forge (`dojo sync`) |
| `P` | Open a pull request for the selected bookmark (`dojo pr`) |
| `d` | Describe the change (`jj describe <id>`, opens your editor) |
| `n` | New change on top of the change (`jj new <id>`) |
| `e` | Edit the change (`jj edit <id>`) |
| `S` | Squash the change into its parent (`jj squash --revision <id>`) |
| `a` | Abandon the change (`jj abandon <id>`) |
| `x` | Split the change (`jj split --revision <id>`, opens the diff editor) |
| `u` | Undo the last jj operation (`jj undo`) |
| `o` | Operation log (`Esc` goes back) |
| `w` | Leave the home view and open the workspace view |
| `r` | Refresh (dojo also refreshes when another jj command changes the repo) |
| `?` | Help |
| `q`, `Ctrl+c` | Quit |

The change keys (`d`, `n`, `e`, `S`, `a`, `x`) act on the selected change in the log pane, on the shown change in the change detail, or on the local change of the selected bookmark. The bookmark keys (`s`, `p`, `y`, `P`) act on the selected bookmark in the bookmarks pane, or on the current bookmark (see `dojo status`) in the other views.

## Workspace view (`dojo ws tui`)

The workspace view is an interactive workspace browser: pick a workspace, inspect its revisions, and perform workspace-level operations without dropping to the shell. Everything it does is also available via `dojo ws <subcommand>` — see [cli.md](cli.md).

### Terms used on this page

- **Default workspace** — the workspace at the repository root (`default@`). It serves the role of the `main`/`master` branch in git. dojo protects it from being closed or transferred from.
- **Stale workspace** — a workspace whose on-disk working copy no longer matches jj's recorded state, usually because another process (another shell, an editor, an agent) modified the repo underneath it.
- **Orphaned workspace** — a workspace whose directory has been deleted on disk but whose entry remains in jj's workspace list.
- **Singular bookmark** — the bookmark whose name matches the workspace's `{{ bookmark }}` template value. dojo creates it on workspace creation and manages it automatically on close/transfer.

### Layout

Two panes, side by side:

- **Left pane** — the workspace list, with a sort indicator, an info panel, and the current workspace's path at the bottom. The info panel shows the selected workspace's description by default; press `i` to toggle to the changed-files view.
- **Right pane** — the commit graph (from `jj log`, with dojo's default or a custom `log-template`). Press `?` or `/` to replace the graph with the help pane.

Focus is on either the workspace list or the graph pane. Press `Tab` to toggle; `Esc` returns focus from the graph to the list.

### Workspace list columns

```
@ default              abcd1234   main, (feature)   initial import
+ feature-login        efgh5678   login              wip: login UI
+ scratch              ijkl9012                      (no description)
```

- **Gutter** — `@` = current workspace, `+` = other
- **Name** — the jj workspace name
- **Change ID** — 8-character prefix of the workspace's working-copy commit change ID
- **Bookmarks** — bookmarks at the workspace head, followed by bookmarks *behind* the head in parentheses
- **Description** — first line of the working-copy commit description (or `(no description)`)

#### Colors

| Color | Meaning |
|---|---|
| Green | The current workspace |
| Red   | Stale — another process modified the repo since the workspace's last snapshot |
| Blue  | Orphaned — the workspace directory no longer exists on disk |

#### Sort modes

Press `r` to cycle between:

1. **Log order** (default) — graph position
2. **Alphabetical** — by name
3. **Last modified** — most recent activity first

The sort indicator in the left pane header shows the active mode.

### Global keybindings

These apply in the default view (not inside a dialog). The same list is available in the TUI by pressing `?` or `/`.

#### Navigation

| Key | Action |
|---|---|
| `↑` / `k` | Move up (workspace list or graph, depending on focus) |
| `↓` / `j` | Move down |
| `→` | When on a workspace, step through its revision chain |
| `←` | Step back through the revision chain; returns to workspace level |
| `Enter` | Switch to the selected workspace (writes `cd` to the shell wrapper) |
| `Tab` | Toggle focus between workspace list and graph pane |
| `Esc` | Return focus from graph to workspace list |

#### Workspace operations (list focused)

| Key | Opens | Description |
|---|---|---|
| `n` | Create dialog | Create a new workspace |
| `x` | Close dialog  | Close the selected workspace |
| `s` | Sync dialog   | Sync the selected workspace with another |
| `t` | Transfer dialog | Transfer changes between workspaces without closing |
| `v` | Revision picker | Pick a revision to split (queues a split action) |
| `b` | Bookmarks dialog | Manage bookmarks on the selected workspace |
| `c` | Copy dialog   | Copy workspace metadata (path, name, change ID, etc.) to clipboard |
| `i` | (toggle)      | Toggle description vs. changed-files view in the info panel |
| `r` | (toggle)      | Cycle sort mode |
| `u` | (action)      | Update a stale workspace (opens the update-stale dialog) |
| `o` | Op log pane   | Swap the left pane for the jj operation log |

The default workspace (`default@`) cannot be closed (`x`) or transferred from (`t`).

Orphaned workspaces (blue) are simply forgotten by `x` — no dialog.

#### Undo / redo

| Key | Action |
|---|---|
| `Z` | Undo the last dojo action by running `jj op restore` to the pre-action op |
| `Y` | Redo the most recently undone action |

dojo records its own actions in a persisted history (`.dojo/action-history.json`) so that `Z`/`Y` only traverse dojo-initiated operations, not every jj operation in the repo.

#### Status line

| Key | Action |
|---|---|
| `p` | Copy the current status message to the clipboard |
| `P` | Clear the status message |

#### Graph pane (when focused)

| Key | Action |
|---|---|
| `k` / `↑` | Scroll up one line |
| `j` / `↓` | Scroll down one line |
| `u` | Scroll up half a page |
| `d` | Scroll down half a page |

#### General

| Key | Action |
|---|---|
| `?` or `/` | Toggle the help pane (replaces the graph pane) |
| `q` | Quit |

### Dialogs

#### Create

Opens on `n`. Four fields: bookmark name, revision to branch from, workspace path, description.

| Key | Action |
|---|---|
| `Tab` / `↓` | Next field |
| `Shift+Tab` / `↑` | Previous field |
| `→` / `←` | In the revision field, cycle candidate revisions from the selected workspace's chain |
| `Enter` | Confirm — creates the workspace, writes file templates, runs hooks |
| `Esc` | Cancel |

Line-editing keys (inside any text field): `Ctrl+a` (start), `Ctrl+e` (end), `Ctrl+u` (delete to start), `Ctrl+k` (delete to end), `Ctrl+w` (delete word backward), `Alt+b` / `Alt+f` (word backward/forward), `Backspace` / `Ctrl+h` (delete character).

#### Close and Transfer

Both open the same dialog. `x` opens it in close mode (the source workspace is forgotten after the operation); `t` opens it in transfer mode (both workspaces stay open). The dialog lists the operations available for the current sync state between source and target, along with a live preview of the jj command sequence that will run.

The command preview is dojo's honest statement of what the operation will do. If you are learning jj, reading (and `c`-copying) the preview is a reliable way to see how fast-forward / merge / rebase / squash map to sequences of `jj new`, `jj rebase`, and friends.

| Key | Action |
|---|---|
| `↑` / `↓` | Navigate the operation list |
| `←` / `→` | Cycle the target workspace |
| `a` | Jump to Adaptive |
| `1`–`4` | Jump to a keyed operation |
| `d` | (Close only) Jump to Detach |
| `b` | (Close only, when the workspace has bookmarks other than its singular bookmark) Cycle bookmark action: NoAction → Advance → Delete |
| `k` | (Close only) Toggle "delete files after close" |
| `c` | Copy the planned jj command sequence to the clipboard |
| `y` / `Enter` | Execute the highlighted operation |
| `Esc` | Cancel |

After a close with delete-files toggled on, a confirmation prompt appears: press `y` to proceed, any other key to cancel.

#### Sync

Opens on `s`. Shows the target selection and the derived operation.

| Key | Action |
|---|---|
| `←` / `k` | Cycle to the previous target |
| `→` / `j` | Cycle to the next target |
| `Enter` / `y` | Execute the sync |
| `Esc` | Cancel |

If the two workspaces are already in sync, no operation is offered — the dialog displays `in sync` and the confirm keys do nothing.

#### Bookmarks

Opens on `b`. Lists the bookmarks on the selected workspace with a per-bookmark action selector.

| Key | Action |
|---|---|
| `n` | Open the new-bookmark popup |
| `t` / `←` | Set action: Tug (move the bookmark to the workspace head) |
| `x` / `→` | Set action: Delete |
| `↑` / `k`, `↓` / `j` | Navigate bookmarks |
| `Space` | Toggle selection for the current bookmark |
| `a` | Select all |
| `Enter` / `y` | Apply the chosen actions to all selected bookmarks |
| `Esc` | Cancel |

The new-bookmark popup has two fields (name and change ID). `Tab` / `↑` / `↓` switch fields; `→` / `←` cycle candidate change IDs; line-editing keys work as in the Create dialog; `Enter` confirms; `Esc` cancels.

#### Copy

Opens on `c`. Lists workspace fields available for copying (name, path, change ID, short change ID, etc.).

| Key | Action |
|---|---|
| `↑` / `↓` | Navigate |
| `Enter` | Copy the selected value and close the dialog |
| `Esc` | Cancel |

#### Split (revision picker)

Opens on `v`. Lets you pick a revision from the selected workspace's chain to split interactively.

| Key | Action |
|---|---|
| `↑` / `k`, `↓` / `j` | Navigate |
| `Enter` | Confirm — dojo temporarily drops out of the alternate screen, runs `jj split` on the chosen revision interactively, then returns to the TUI |
| `Esc` | Cancel without splitting |

Because `jj split` prompts you to choose hunks, dojo hands the terminal back to jj for the duration of the split. Your TUI state returns when the split finishes.

#### Update stale

Triggered by `u` on a stale workspace. Lets you inspect the pending working-copy changes before resolving the staleness with `jj workspace update-stale`.

The diff is loaded on a background thread on demand — press `d` to start it. The diff is not loaded automatically because a stale workspace may have a large working copy, and dojo does not want to block the TUI.

| Key | Action |
|---|---|
| `d` | Start loading the diff (only if not already loaded or in progress) |
| `↑` / `k`, `↓` / `j` | Scroll the diff |
| `Enter` | Save the currently selected diff to `.dojo/diffs/<name>-<change_id>.patch` |
| `a` | Save all diffs to `.dojo/diffs/` |
| `y` | Resolve staleness (calls `jj workspace update-stale`) |
| `Esc` | Back to the list |

### Stale alert

A modal that blocks the TUI when dojo detects the *repo itself* is stale (op head moved under it).

| Key | Action |
|---|---|
| `r` | Save all stale diffs to `.dojo/diffs/`, then call `jj workspace update-stale` |
| `q` / `Esc` | Quit the TUI |

Actions `n`, `s`, `t`, `x`, `v`, `b` all open the stale alert instead of their normal dialog while the repo is stale.

### Operation log pane

Press `o` to swap the left pane for the jj operation log. The graph pane continues to show the repo graph at the selected operation.

| Key | Action |
|---|---|
| `↑` / `k`, `↓` / `j` | Navigate operations |
| `s` | Toggle snapshot-op visibility |
| `R` | Open the op-restore confirmation (restore to the selected operation) |
| `Z` / `Y` | Undo / redo dojo-initiated actions |
| `Tab` | Toggle focus to the graph |
| `o` / `Esc` (list focus) | Close the op log pane |

### Config warning modal

If `.config/dojo.toml` has template warnings (unknown variables, malformed delimiters), pressing `n` (create) opens the `ConfigWarning` modal first. From the modal you can proceed to the Create dialog or back out. See [configuration.md](configuration.md#template-validation).

### Summary of dialogs

| Dialog | Opened by | Confirm | Cancel |
|---|---|---|---|
| Create | `n` | `Enter` | `Esc` |
| Close | `x` | `y` / `Enter` | `Esc` |
| Transfer | `t` | `y` / `Enter` | `Esc` |
| Sync | `s` | `y` / `Enter` | `Esc` |
| Bookmarks | `b` | `y` / `Enter` | `Esc` |
| Copy | `c` | `Enter` | `Esc` |
| Split (revision picker) | `v` | `Enter` | `Esc` |
| Update stale | `u` on stale workspace | `y` | `Esc` |
| Stale alert | auto, on stale repo | `r` to resolve | `q` / `Esc` quits the TUI |
| Config warning | `n` when the config has warnings | `Enter` to proceed | `Esc` |
| Op restore | `R` in the op log pane | `y` | `Esc` |
| Confirm remove files | after a close with delete-files | `y` | any other key |
