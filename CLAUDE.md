# glab-dash

Ultra-fast TUI for managing GitLab issues and merge requests across teams.

## Build & Run

```bash
cargo build              # dev build
cargo build --release    # optimized release build
cargo run                # run (requires config)
cargo test               # run all tests
cargo fmt                # format code
cargo clippy             # lint
```

## Config

Config file: `~/.config/glab-dash/config.yaml` (or set `GLAB_DASH_CONFIG` env var)

```yaml
gitlab_url: "https://gitlab.com"
token: "glpat-xxx"
me: "username"
tracking_project: "org/team-tracker"
teams:
  - name: "team1"
    members: ["alice", "bob"]
  - name: "team2"
    members: ["charlie", "dave"]
filters:
  - name: "My open MRs"
    kind: merge_request
    conditions:
      - { field: author, op: eq, value: "$me" }
      - { field: state, op: eq, value: opened }
```

Env var overrides: `GITLAB_URL`, `GITLAB_TOKEN`, `GITLAB_PROJECT`

## Architecture

- **Rust** + **ratatui** + **crossterm** + **tokio** + **reqwest**
- Edition 2024, rustfmt edition 2024
- Async event loop: crossterm events on a thread, API calls via tokio::spawn, messages over mpsc channels
- Views: Dashboard, IssueList, MrList, IssueDetail, MrDetail
- Overlays: Help, FilterEditor, Picker, ConfirmDialog, CommentInput
- Filter system: composable conditions applied client-side for instant filtering

## Key Directories

- `src/app.rs` — Main state machine, view routing, event handling
- `src/config.rs` — Config loading
- `src/event.rs` — Crossterm event handler
- `src/gitlab/` — API client and types
- `src/filter/` — Filter condition model, matching engine, tests
- `src/ui/views/` — View rendering (dashboard, issue_list, mr_list, etc.)
- `src/ui/components/` — Reusable UI components (status_bar, filter_bar, picker, etc.)
- `src/ui/styles.rs` — Color/style definitions
- `src/ui/keys.rs` — Key detection helpers

## Tests

Tests are in `src/filter/tests.rs` and `src/config_tests.rs`. Run with `cargo test`.
