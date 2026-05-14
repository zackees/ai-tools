# ai-tools

[![CI](https://github.com/zackees/ai-tools/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/zackees/ai-tools/actions/workflows/ci.yml)

Native Rust binaries that fill in the rough edges of Claude Code and Codex.

The first (and currently only) binary is **`meta-hook`** — a hook delegator
that lets Claude Code / Codex run the hooks belonging to a *nested* git
sub-repo when a tool call touches a file inside it. See
[issue #1](https://github.com/zackees/ai-tools/issues/1) for the design.

This repo is a Cargo workspace; sibling single-purpose hook binaries can be
added under `crates/` later.

---

## Install

### From a release (recommended)

Grab the archive for your platform from
<https://github.com/zackees/ai-tools/releases/latest>, extract, and put
`meta-hook` somewhere on `PATH`. Each release ships a `.sha256` sidecar
next to every archive — verify it before extracting:

```bash
sha256sum -c meta-hook-v0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256
tar -xzf meta-hook-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
install meta-hook-v0.1.0-x86_64-unknown-linux-gnu/meta-hook ~/.local/bin/
```

Supported targets:

| OS              | x86_64                          | aarch64 / arm64                  |
|-----------------|---------------------------------|----------------------------------|
| Linux (glibc)   | `x86_64-unknown-linux-gnu`      | `aarch64-unknown-linux-gnu`      |
| Linux (musl)    | `x86_64-unknown-linux-musl`     | `aarch64-unknown-linux-musl`     |
| macOS           | `x86_64-apple-darwin`           | `aarch64-apple-darwin`           |
| Windows         | `x86_64-pc-windows-msvc`        | `aarch64-pc-windows-msvc`        |

The **musl** archives ship fully-statically-linked binaries that run on
Alpine and other distributions without glibc — pick those if your host
isn't a Debian/Ubuntu/Fedora-family system, or if you want a binary you
can drop into a minimal container image with no shared-library dance.

### From source

```bash
cargo install --git https://github.com/zackees/ai-tools meta-hook
```

---

## Hook registration

Drop into `~/.claude/settings.json` (or any scoped `settings.json`):

```json
{
  "hooks": {
    "PreToolUse": [
      { "matcher": "*", "hooks": [{ "type": "command", "command": "meta-hook --pre-tool" }] }
    ],
    "PostToolUse": [
      { "matcher": "*", "hooks": [{ "type": "command", "command": "meta-hook --post-tool" }] }
    ]
  }
}
```

`meta-hook` exits 0 transparently if there's no enclosing sub-repo with
hooks to delegate to — it's safe to register globally.

---

## Local development

This repo's build / lint / test all route through
[**soldr**](https://github.com/zackees/soldr) — a thin wrapper that pins
Rust toolchain calls to the `rustup`-managed install, sidestepping stale
PATH shims (e.g. chocolatey cargo on Windows). The same wrapper is used in
CI via the [`zackees/setup-soldr`](https://github.com/zackees/setup-soldr)
action.

```bash
# One-time per clone: drops soldr into .venv/{bin,Scripts}/.
./install

# Or, if you want it on PATH globally:
./install --global

bash build    # soldr cargo build --release --bin meta-hook
bash lint     # soldr cargo fmt --check + soldr cargo clippy -D warnings
bash test     # soldr cargo test --all
```

Run `bash lint` after **any** code edit — CI enforces it.

---

## Release flow

Releases are tagged from `main`:

```bash
git tag v0.1.0
git push --tags
```

The `release.yml` workflow then:

1. Cross-builds `meta-hook` for the eight target triples listed above
   (six glibc/macOS/Windows + two Linux musl variants).
2. Strips the binary (where applicable).
3. Packages each as `meta-hook-vX.Y.Z-<triple>.{tar.gz,zip}` with a
   SHA256 sidecar.
4. Publishes a single GitHub Release with all 16 assets (8 archives +
   8 `.sha256` sidecars) attached and auto-generated release notes.
   `make_latest: true`, no draft.

### Immutability rules

**Tags are write-once.** The `refs/tags/v*` ruleset on GitHub blocks
updates and deletions; pushing `git push --force origin v0.1.0` against
a published tag will be rejected by the server.

**Releases are write-once.** The publish job hard-fails if a release
already exists for the tag. To fix a broken release, **cut a new patch
version** — never edit the old one. (If GitHub's repo-level "immutable
releases" toggle is also enabled, the API will reject asset edits even
through the UI.)

**`main` is protected.** Direct pushes are blocked; changes land via
pull request. CI on all six platforms must pass before merge.

---

## License

[BSD-3-Clause](./LICENSE). Copyright (c) 2026, Zachary Vorhies.
