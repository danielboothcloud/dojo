# Changelog

## Unreleased (dojo)

dojo is a fork of [daeh/jujutsu-gi](https://github.com/daeh/jujutsu-gi) (`ji`). The entries below 0.1.6 are the history of `ji`.

### Changed

- Renamed `ji` to `dojo`: the binary, `.config/dojo.toml`, `DOJO_DIRECTIVE_FILE`, `DOJO_LOG`, `.dojo/`, and the shell wrappers. The config key `ji-author` still works as an alias of `dojo-author`.
- The workspace commands moved under `dojo ws` (`dojo ws switch`, `new`, `close`, `transfer`, `sync`, `ls`). The workspace TUI opens with `dojo ws tui`.
- `dojo` with no command opens the new home view.
- Linux support: the shell detection reads `/proc`, and the macOS-only dependencies (`libproc`, `xattr`) are only built on macOS.
- Bookmark completion lists local and remote bookmarks.

### Added

- Bookmark workflows that print each jj command before it runs: `dojo fetch`, `dojo branches`, `dojo switch`, `dojo push`, `dojo sync`, `dojo new-branch`, `dojo status`, and `dojo pr` (`gh` or `glab`).
- Global flags `--json`, `--dry-run`, and `-y/--yes`; exit code 2 when dojo needs a decision.
- Sundae support: explicit `--remote` on every fetch and push (no dependency on jj's `git.push` / `git.fetch`), explanations and one-step fixes for the "protected", "forge has commits you lack", and private-path refusals, and a fetch of `origin` after each push.
- After a push of the working-copy change, `dojo push` starts a new empty change on top of it, so the pushed commit stays unchanged.
- A user config file: `$XDG_CONFIG_HOME/dojo/config.toml`. New keys: `protected` (default `["main"]`), `push-remote`, and `upstream-remote`.
- Home TUI: bookmarks and log panes, the change detail with the coloured diff, and keys for fetch, switch, push, sync, pr, describe, new, edit, squash, abandon, split, undo, and the operation log. Each key shows a preview of the commands first.
- CI on Linux (latest jj and jj 0.47) and macOS; a release workflow with static musl binaries for x86_64 and aarch64 Linux and a `SHA256SUMS` file.


## 0.1.6 - 2026-07-28

### Changed

- add images to readme
- transfer: disable the merge-abandon-old method
- demo: record a VHS tour of the TUI against a fixed-shape fixture
- fidelity: promote tests to the default suite; changelog + docs
- fidelity: preserve Finder alias xattrs and warn on broken links
- plan: prepare release 0.1.5

## Unreleased

### Added

- macOS Finder-metadata fidelity: workspace operations (`new`, `close`, `transfer`, `sync`) now restore the Finder metadata (`com.apple.FinderInfo`, `com.apple.ResourceFork`) that jj strips whenever it materializes working-copy files, so Finder aliases survive workspace creation and merge-back.
The restore writes can be disabled with the new `preserve-finder-xattrs` config key (default on); see [docs/xattrs.md](docs/xattrs.md).
- Fidelity warnings: ji reports hard links broken by materialization (jj cannot represent them; content is preserved, the shared inode is not) and aliases it cannot restore (no on-disk copy to take metadata from), on CLI stderr and in the TUI status line.

## 0.1.5

### Fixed

- **fish:** `ji switch`/`sync`/`transfer <TAB>` no longer fall back to file completion when a stale pre-0.1.4 static completion at `~/.config/fish/completions/ji.fish` shadowed Homebrew's dynamic vendor completion.
`ji config shell install fish` now removes a redundant/stale user-dir completion when a vendor completion already provides one (and installs one only where nothing else does); `ji config shell status fish` reports the shadow.

## 0.1.4

### Added

- Dynamic, annotated shell tab-completion for workspace and bookmark arguments.
- `ji config shell install --all` installs integration for every shell that already has a config (zsh, bash, fish).
- `ji config shell status` now reports bypass aliases — a `ji` alias (or fish `function ji`) that shadows the wrapper, or a differently-named alias that runs the `ji` binary directly — and `install` warns about any it finds.

### Changed

- Shell completions are now generated dynamically (clap_complete's dynamic engine) rather than statically; `ji config shell install` and the packaged Homebrew completions emit the same dynamic registration, so completion always reflects the installed binary.

## 0.1.3

### Added

- Reason-aware shell-integration notes: when `ji switch`/`new`/`close` can't change the shell's directory (the wrapper isn't active), `ji` prints why and how to enable auto-cd, replacing the raw `JI_DIRECTIVE_FILE not set` error.
A `ji switch` that can't `cd` exits non-zero; `ji new` and a `close` that kept your directory exit zero.
Closing the workspace you're standing in *and removing its files* prints a `current directory was removed — run: cd <root>` escape and exits non-zero.
- `ji config shell install` previews its edits and prompts `[y/N/?]` before writing; `--yes`/`-y` skips it (a non-interactive run auto-proceeds).
- `ji close` refuses to close the `default` workspace, matching the TUI.

### Changed

- Homebrew caveats and docs no longer use the "Homebrew won't edit your startup files" framing; the install one-liner now chains `ji config shell install`.

## 0.1.2

### Added

- Homebrew installation path.

## 0.1.1

### Changed

- `ji config shell` now detects the active shell from the parent process (via libproc), stopping at the nearest recognized shell ancestor, instead of the `$SHELL` login shell. It falls back to `$SHELL` only when no shell can be identified. `init`, `install`, and `uninstall` error clearly when the active shell is recognized but unsupported; `status` instead reports an unsupported active shell non-fatally (alongside `$SHELL`).
- Internal refactoring only, with no change to existing behavior: TUI key dispatch now delegates to each dialog's own `handle_key`, single-line text fields share a common `LineEditor`, and the jj-subprocess snapshot policy was consolidated and documented.

### Fixed

- Harden guards for handling of pending working-copy edits made between opening a dialog for a mutating operation (`sync`, `close`, and `transfer`) and executing the command. Previously, a workspace's uncommitted working copy changes could be stranded in a dangling head node. While this resulted in a vocal warning, the new guards should handle the edge cases more robustly.

## 0.1.0

Initial public release.
