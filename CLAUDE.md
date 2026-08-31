# glab-dash

Ultra-fast TUI for managing GitLab issues and merge requests across teams.

## Build & Run

```bash
cargo build --workspace          # dev build
cargo build --release            # optimized release build
cargo run                        # run the TUI (requires config)
cargo run -- debug               # exercise fetch paths; output goes to the log file
cargo test --workspace           # run all tests
cargo fmt --all                  # format code
cargo clippy --workspace         # lint (pedantic, must pass with zero warnings)
typos                            # spell check
make lint                        # auto-fix clippy warnings (must run before committing)
make all                         # format + lint + test (full pre-commit check)
make install                     # cargo install --path crates/glab-dash
```

## Code Quality

All code must pass these checks before committing (enforced by CI):

1. **`cargo fmt --check`** — all code formatted
2. **`cargo clippy --workspace`** — zero warnings; `clippy::pedantic` and `warnings = "deny"` are set once in the root `Cargo.toml` under `[workspace.lints]`, and every crate opts in with `[lints] workspace = true`
3. **`cargo build --workspace`** — zero warnings (dead code, unused imports, etc.)
4. **`cargo test --workspace`** — all tests pass
5. **`typos`** — no spelling errors (`_typos.toml` has exceptions)

**Before committing, always run `make lint`** (`cargo clippy --workspace --all-targets --all-features --fix --allow-dirty -- -D warnings`) to auto-fix clippy warnings. This matches the CI clippy check and prevents pipeline failures.

Pedantic lint exceptions are configured once in `[workspace.lints.clippy]` in the root `Cargo.toml`. Do not add new `#[allow(...)]` attributes without good reason — prefer fixing the lint.

**No defensive serde parsing**: Do not use `#[serde(default)]` on GraphQL response structs. The GraphQL schema defines a fixed shape — trust it. `Option<T>` already handles nullable fields correctly without `default`.

## Tests

Run with `cargo test --workspace`.
