# glab-dash

Ultra-fast TUI for managing GitLab issues and merge requests across teams.

## Build & Run

```bash
cargo build --workspace          # dev build
cargo build --release            # release build
cargo run                        # run the TUI (requires config)
cargo run -- debug               # exercise fetch paths; output goes to the log file
cargo test --workspace           # run all tests
cargo fmt --all                  # format code
cargo clippy --workspace         # lint
typos                            # spell check
make lint                        # clippy with --fix
make all                         # format + lint + test
make install                     # cargo install --path crates/glab-dash
```

## Code Quality

CI enforces, and `make all` runs:

1. `cargo fmt --check`
2. `cargo clippy --workspace` — zero warnings; `clippy::pedantic` and `warnings = "deny"` live in the root `Cargo.toml` under `[workspace.lints]`, and every crate opts in with `[lints] workspace = true`
3. `cargo build --workspace` — zero warnings
4. `cargo test --workspace`
5. `typos` — `_typos.toml` holds the exceptions

Run `make lint` before committing.

Pedantic lint exceptions belong in `[workspace.lints.clippy]` in the root `Cargo.toml`, not in new `#[allow(...)]` attributes.

**No defensive serde parsing**: no `#[serde(default)]` on GraphQL response structs. `Option<T>` already covers nullable fields.

## Comments

Write each comment as if the file had none: would a competent reader get
something **wrong** without this line? Not slower — wrong. If not, there is no
comment.

That leaves four kinds:

- An external fact the code cannot show — a GitLab API behavior, a WCAG
  threshold, a schema quirk.
- A landmine — why the obvious simplification is wrong, so nobody "fixes" it
  back. (`palette`'s `from_color` silently clamps; its `Mix` rounds ties the
  other way.)
- An invisible contract — a bare tuple's field order and units, who must call
  what first, an ordering guarantee.
- A magic number that would otherwise be tuned by guesswork.

Not: restating the signature, narrating the next line, section banners, or the
reasoning behind a design. `ponytail:` markers stay.
