# Installation

## Supported platform

macOS only. dojo uses macOS-specific behavior (codesigning on install, clipboard access via `pbcopy`) and is not tested elsewhere.

## Prerequisites

- **[jj](https://jj-vcs.github.io) v0.42 or later** on `PATH`. Verify with `jj --version`.
- **Rust toolchain** (edition 2024). The repository's `rust-toolchain.toml` selects the stable channel (with `clippy` and `rustfmt`); `rustup` installs it automatically on first build.
- **[Task](https://taskfile.dev)** (recommended). Every build and maintenance command in this project is a Task target.

The prerequisites above are for a from-source build. **Homebrew** (below) installs `dojo` and its `jj` dependency for you and needs none of them.

## Install with Homebrew (recommended)

```sh
brew tap daeh/jujutsu-gi
brew install dojo && dojo config shell install
```

Homebrew builds `dojo` from source (using its own `rust`) and installs `jj` as a runtime dependency. `dojo config shell install` adds the wrapper that lets `dojo` change your shell's directory on switch (see below). Verify:

```sh
dojo --version
```

To upgrade later: `brew update && brew upgrade dojo`.

## Build and install

```sh
task release
```

This builds a universal (Apple Silicon + Intel) release binary, generates the `dojo(1)` man page and shell completions, installs the binary to `~/.local/bin/dojo` (codesigned with an ad-hoc signature) and the man page to `~/.local/share/man/man1/`, and installs the shell integration for the active shell (falling back to `$SHELL`). `~/.local/bin` must be on your `PATH`.

Verify the install:

```sh
dojo --version
```

Manual alternative (without Task):

```sh
cargo build --release
cp target/release/dojo ~/.local/bin/
codesign -f -s - ~/.local/bin/dojo
```

## Shell integration

`task release` already installs the wrapper for the active shell (falling back to `$SHELL`). If you used the manual build above, or want integration for an additional shell, run:

```sh
dojo config shell install [SHELL]
```

With no argument it targets the active shell (falling back to `$SHELL`); pass `zsh`, `bash`, or `fish` to install another, or `--all` to install for every shell that already has a config. The wrapper lets `dojo` change your shell's current directory when you switch workspaces. See [shell-integration.md](shell-integration.md) for details and for manual installation.

## Upgrading

Pull the latest source and re-run `task release`. The build is incremental. To force a clean rebuild, run `task release:clean` (equivalent to `task clean && task release`).

## Uninstall

Remove the shell integration first, then the binary.

If you installed with Homebrew:

```sh
dojo config shell uninstall   # only if you enabled it
brew uninstall dojo
brew untap daeh/jujutsu-gi
```

If you built from source:

```sh
dojo config shell uninstall
rm ~/.local/bin/dojo
```

`dojo config shell uninstall` removes the wrapper files and the rc stanza that `install` added. Pass a shell name to target one shell; otherwise it uses the active shell (falling back to `$SHELL`). See [shell-integration.md](shell-integration.md).
