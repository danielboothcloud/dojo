# Installation

## Supported platforms

- **Linux** (x86_64 and aarch64). The release binaries are static (musl), so they run on any distribution.
- **macOS**. Build from source. The workspace commands also keep Finder metadata on macOS (see [xattrs.md](xattrs.md)).

CI tests both platforms with the latest jj release, and Linux also with jj 0.47.

## Prerequisites

- **[jj](https://jj-vcs.github.io) v0.42 or later** on `PATH`. Verify with `jj --version`.
- For `dojo pr`: [`gh`](https://cli.github.com) for GitHub or [`glab`](https://gitlab.com/gitlab-org/cli) for GitLab.
- For a build from source: a **Rust toolchain** (edition 2024). The repository's `rust-toolchain.toml` selects the stable channel (with `clippy` and `rustfmt`), and `rustup` installs it on the first build. In a repo that uses [mise](https://mise.jdx.dev), `mise install` installs Rust and jj from `mise.toml`.

## Install a release binary (Linux)

Each release on GitHub (tags `v*`) has static binaries for `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`, and a `SHA256SUMS` file.

1. Download the tarball for your machine and `SHA256SUMS` from the [releases page](https://github.com/danielboothcloud/dojo/releases).
2. Check the download:

   ```sh
   sha256sum --check --ignore-missing SHA256SUMS
   ```

3. Unpack it and put `dojo` on your `PATH`:

   ```sh
   tar -xzf dojo-v<version>-x86_64-unknown-linux-musl.tar.gz
   install -m 755 dojo-v<version>-x86_64-unknown-linux-musl/dojo ~/.local/bin/dojo
   ```

4. Verify the install:

   ```sh
   dojo --version
   ```

## Build from source

With cargo, from the repository:

```sh
cargo install --locked --git https://github.com/danielboothcloud/dojo dojo
```

Or from a checkout:

```sh
cargo build --release --locked
install -m 755 target/release/dojo ~/.local/bin/dojo
```

On macOS, sign the binary after you copy it: `codesign -f -s - ~/.local/bin/dojo`.

To build a static Linux binary yourself, add the musl target and build for it:

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --locked --target x86_64-unknown-linux-musl
```

`~/.local/bin` (or `~/.cargo/bin` for `cargo install`) must be on your `PATH`.

### macOS: `task release`

On macOS, `task release` (needs [Task](https://taskfile.dev)) builds a universal (Apple Silicon + Intel) binary, generates the `dojo(1)` man page and shell completions, installs the binary to `~/.local/bin/dojo` (with an ad-hoc signature) and the man page to `~/.local/share/man/man1/`, and installs the shell integration for the active shell.

### Homebrew

There is no Homebrew tap for dojo yet. The tap `daeh/jujutsu-gi` installs the original `ji` tool, not dojo.

## Shell integration

The workspace commands (`dojo ws switch`, `dojo ws new`, `dojo ws close`) change your shell's directory. For this, install the shell wrapper:

```sh
dojo config shell install [SHELL]
```

With no argument it targets the active shell (falling back to `$SHELL`); pass `zsh`, `bash`, or `fish` to install another, or `--all` to install for every shell that already has a config. The bookmark workflows (`dojo push`, `dojo sync`, ...) do not need the wrapper. See [shell-integration.md](shell-integration.md) for details and for manual installation.

## Upgrading

Install the new release binary, or pull the latest source and build again.

## Uninstall

Remove the shell integration first, then the binary:

```sh
dojo config shell uninstall
rm ~/.local/bin/dojo
```

`dojo config shell uninstall` removes the wrapper files and the rc stanza that `install` added. Pass a shell name to target one shell; otherwise it uses the active shell (falling back to `$SHELL`). See [shell-integration.md](shell-integration.md).
