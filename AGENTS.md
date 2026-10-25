# AGENTS.md - dumper

## Purpose

A static Rust binary the shared workflows (`workflows` repo, via the `ci` harness) copy into a freshly built
image and run as its entrypoint before init. Each subcommand writes one kind of metadata for the hub and the
website. The output formats are read by the `hub` and `site` repos: changing one breaks them.

## Layout

| Path | What |
| --- | --- |
| `src/main.rs` | Subcommand dispatch. |
| `src/files.rs` | `dumper files`: the `lsio-files/1` file tree, its format documented at the top. |
| `CHANGELOG.md` | One section per version; the release workflow uses it as the release notes. |
| `.github/workflows/ci.yml` | fmt, clippy, tests and a release build on both architectures. |
| `.github/workflows/release.yml` | On a `v*` tag: static builds on native runners, `sha256sums.txt`, the GitHub Release. |

## What you may edit

Everything, by pull request. Keep the binary static, dependency-light and free of network access. A change to
an output format needs a new `format` value (`lsio-files/2`) and matching changes in `hub` and `site`.
`LICENSE` stays GPL-3.0.

## How to validate

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cargo build --release --target x86_64-unknown-linux-musl
```

## Releasing

Bump `version` in `Cargo.toml`, add its `CHANGELOG.md` section, merge, then push the tag `v<version>`. The
workflows always fetch the latest stable release, so a release is live for every image build at once.

## Pull request rules

Disclose AI use in the description. One change per PR. Commit subjects are short imperative sentences without
prefixes, emoji or trailers.
